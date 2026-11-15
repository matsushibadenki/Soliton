"""Measure synthetic practical conversations through a running qualified desktop app."""
import argparse
import json
import os
from pathlib import Path
import runpy
import statistics
from uuid import uuid4
import urllib.request
from types import SimpleNamespace

scripts = Path(__file__).resolve().parent
benchmark = runpy.run_path(str(scripts / 'compare-shared-prefill.py'))
fixtures = runpy.run_path(str(scripts / 'validate-prefix-multiturn.py'))
API = 'http://127.0.0.1:8080'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--desktop-pid', required=True, type=int)
    parser.add_argument('--memory-probe', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--explicit-format', action='store_true')
    args = parser.parse_args()
    headers = {'Origin': API, 'Content-Type': 'application/json'}

    def call(path, method='GET'):
        req = urllib.request.Request(API + path, headers=headers, method=method)
        with urllib.request.urlopen(req, timeout=15) as response:
            return json.load(response)

    headers['x-studio-token'] = call('/studio/session', 'POST')['token']
    initial = call('/studio/state')
    assert initial['status'] == 'ready' and initial['settings']['engine'] == 'vllmApple'
    profile = initial['settings']['vllm']
    assert [profile[key] for key in ('context', 'kvCacheMiB', 'allocatorCacheMiB')] == [4096, 1536, 512]
    host = SimpleNamespace(pid=args.desktop_pid)
    helpers = benchmark['helpers']
    owned = helpers['process_tree'](host.pid)
    assert len(owned) >= 3
    guard = helpers['HostLoadGuard']()
    rows = []
    report = dict(passed=False, samples=rows, context=4096, workload='three practical six-turn conversations', explicitFormat=args.explicit_format)
    with args.output.open('x') as output:
        try:
            guard.set_owned([os.getpid(), *owned])
            run_id = uuid4().hex[:8]
            for language, prompts, _ in fixtures['CASES']:
                messages = []
                for turn, prompt in enumerate(prompts, 1):
                    if args.explicit_format and language == 'ja' and turn >= 4:
                        prompt += (' 単位（本、個）は書かないでください。'
                                   if turn < 6 else ' 数字の後に単位（本、個）は付けないでください。')
                    if args.explicit_format and language == 'zh' and turn <= 3:
                        prompt += ' 不要添加标点。'
                    content = f'[{run_id}-{language}]\n{prompt}' if turn == 1 else prompt
                    messages.append(dict(role='user', content=content))
                    benchmark['wait_for_quiet'](guard)
                    row = benchmark['measure'](API, headers, host, args.memory_probe,
                        dict(language=language, messages=messages), guard, fresh_long=False)
                    rows.append(dict(label=f'{language}-{turn}', turn=turn, **row))
                    messages.append(dict(role='assistant', content=row['answer']))
                    print(language, turn, row['cachedTokens'], row['promptTokens'],
                          round(row['responseMs']), flush=True)
            final = call('/studio/state')
            assert final['engineSession'] == initial['engineSession']
            assert final['document']['messages'] == initial['document']['messages']
            report['historyPreserved'] = True
            report['formatChecks'] = fixtures['format_checks'](rows)
            for name, group in [('all', rows), ('followUp', [r for r in rows if r['turn'] > 1])]:
                report[name] = dict(requests=len(group),
                    cachedInputRatio=sum(r['cachedTokens'] for r in group) / sum(r['promptTokens'] for r in group),
                    medianFirstContentMs=statistics.median(r['firstContentMs'] for r in group),
                    medianResponseMs=statistics.median(r['responseMs'] for r in group),
                    sampledPeakPhysicalBytes=max(r['sampledPeakPhysicalBytes'] for r in group),
                    latencyEligible=all(map(benchmark['latency_eligible'], group)))
            report['passed'] = True
        except BaseException as error:
            report['error'] = str(error)
            raise
        finally:
            json.dump(report, output, ensure_ascii=False, indent=2)
            output.write('\n')


if __name__ == '__main__':
    main()
