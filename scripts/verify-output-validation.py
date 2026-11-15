"""Verify exact output validation via an isolated, qualified shared API."""
import argparse
import json
from pathlib import Path
import runpy
import statistics
import time
import urllib.error
import urllib.request

CASES = runpy.run_path(str(Path(__file__).with_name('validate-prefix-multiturn.py')))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--api', required=True, choices=['http://127.0.0.1:18099'])
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--retry-on-mismatch', action='store_true')
    parser.add_argument('--localized-retry', action='store_true')
    parser.add_argument('--acknowledgement-retry', action='store_true')
    args = parser.parse_args()
    assert not args.acknowledgement_retry or args.localized_retry
    assert not args.localized_retry or args.retry_on_mismatch
    headers = {'Origin': args.api, 'Content-Type': 'application/json'}

    def call(path, body=None):
        req = urllib.request.Request(args.api + path, None if body is None else
                                     json.dumps(body).encode(), headers)
        try:
            with urllib.request.urlopen(req, timeout=120) as response:
                return response.status, json.load(response)
        except urllib.error.HTTPError as error:
            return error.code, json.load(error)

    headers['x-studio-token'] = call('/studio/session', {})[1]['token']
    initial = call('/studio/state')[1]
    assert initial['status'] == 'offline'
    armed = False
    rows = []
    with args.output.open('x') as output:
        try:
            assert call('/studio/action', {'action': 'start'})[0] == 200
            armed = True
            for _ in range(300):
                state = call('/studio/state')[1]
                if state['status'] == 'ready':
                    break
                assert state['status'] != 'offline'
                time.sleep(.25)
            else:
                raise RuntimeError('Engine did not become ready')
            route = '/v1/chat/completions'
            for language, prompts, _ in CASES['CASES']:
                messages = []
                for turn, prompt in enumerate(prompts, 1):
                    messages.append(dict(role='user', content=prompt))
                    label = f'{language}-{turn}'
                    expected = CASES['format_checks']([dict(label=label, answer='')])[0]['expected']
                    body = dict(model='default_model', messages=messages, stream=False, temperature=0, max_tokens=64)
                    results = {}
                    for mode in (('baseline', 'validated') if turn % 2 else ('validated', 'baseline')):
                        rule = {'one_of': [expected]}
                        if args.retry_on_mismatch:
                            rule['retry_once'] = args.localized_retry or mode == 'validated'
                            if args.localized_retry and (mode == 'validated' or args.acknowledgement_retry):
                                rule['retry_language'] = 'zh-CN' if language == 'zh' else language
                                if args.acknowledgement_retry and mode == 'validated':
                                    rule['retry_acknowledgement'] = True
                        payload = (body if mode == 'baseline' and not args.retry_on_mismatch
                                   else dict(body, soliton_output_validation=rule))
                        start = time.monotonic()
                        status, reply = call(route, payload)
                        elapsed = (time.monotonic() - start) * 1000
                        assert status == 200 or ((mode == 'validated' or args.retry_on_mismatch) and status == 422 and 'choices' not in reply)
                        answer = reply['choices'][0]['message']['content'] if status == 200 else None
                        results[mode] = dict(status=status, responseMs=elapsed, answer=answer)
                    baseline, validated = results['baseline'], results['validated']
                    if args.retry_on_mismatch:
                        if baseline['status'] == 200 and not args.localized_retry:
                            assert validated['status'] == 200 and validated['answer'] == baseline['answer']
                        if validated['status'] == 200:
                            assert validated['answer'].strip() == expected
                        # Keep the conversation identical even when both API modes reject.
                        status, raw = call(route, body)
                        assert status == 200
                        history_answer = raw['choices'][0]['message']['content']
                        if not args.localized_retry:
                            assert (baseline['status'] == 200) == (history_answer.strip() == expected)
                        elif history_answer.strip() == expected:
                            assert baseline['status'] == validated['status'] == 200
                    else:
                        assert (validated['status'] == 200) == (baseline['answer'].strip() == expected)
                        if validated['status'] == 200:
                            assert validated['answer'] == baseline['answer']
                        history_answer = baseline['answer']
                    messages.append(dict(role='assistant', content=history_answer))
                    rows.append(dict(label=label, expected=expected, **results))
                    print(label, validated['status'], flush=True)
            assert call(route, dict(body, stream=True, soliton_output_validation={'one_of': ['OK']}))[0] == 400
            assert call(route, dict(body, soliton_output_validation={'one_of': []}))[0] == 400
            assert call('/studio/state')[1]['document']['messages'] == initial['document']['messages']
            report = {'cases': rows, 'requestsPerMode': len(rows), 'historyPreserved': True,
                      'retryOnMismatch': args.retry_on_mismatch,
                      'localizedRetry': args.localized_retry,
                      'acknowledgementRetry': args.acknowledgement_retry,
                      'baselineAccepted': sum(r['baseline']['status'] == 200 for r in rows),
                      'accepted': sum(r['validated']['status'] == 200 for r in rows),
                      'medianResponseMs': {mode: statistics.median(r[mode]['responseMs'] for r in rows)
                                           for mode in ('baseline', 'validated')}}
            json.dump(report, output, ensure_ascii=False, indent=2)
            output.write('\n')
        finally:
            if armed:
                assert call('/studio/action', {'action': 'stop'})[0] == 200


if __name__ == '__main__':
    main()
