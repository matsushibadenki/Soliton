"""Compare prefill steps on isolated MLX-LM with three fresh long histories."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import runpy
import signal
import statistics
import subprocess
import time
import urllib.request

existing = runpy.run_path(str(Path(__file__).with_name('compare-kv-retention.py')))
BackendConfig = existing['BackendConfig']
BackendProcess = existing['BackendProcess']
HostLoadGuard = existing['HostLoadGuard']
capture_identity = existing['capture_identity']
detect_hardware = existing['detect_hardware']
detect_memory = existing['detect_memory']
exclusive = existing['exclusive']
inspect_model = existing['inspect_model']
process_tree = existing['process_tree']

ORDER = (2048, 512, 1024, 1024, 512, 2048)


class PrefillBackendConfig(BackendConfig):
    def command(self):
        command = super().command()
        wrapper = Path(command[5])
        assert wrapper.name == 'mlx_kv_budget.py'
        command[5] = str(Path(__file__).resolve().with_name('mlx-retained-kv.py'))
        return command


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('settings', 'fixtures', 'memory-probe', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    settings = json.loads(args.settings.read_bytes())['settings']['vllm']
    fixture_bytes = args.fixtures.read_bytes()
    fixtures = json.loads(fixture_bytes)
    assert [row['language'] for row in fixtures] == ['en', 'ja', 'zh']
    assert settings['context'] == 4096 and settings['kvCacheMiB'] == 1536
    assert settings['allocatorCacheMiB'] == 512 and settings['backend'] == 'mlx_lm'
    executable = Path(settings['executable'])
    model = Path(settings['model'])
    os.environ.update(VLLM_APPLE_KV_CACHE_MIB='1536', VLLM_APPLE_ALLOCATOR_CACHE_MIB='512',
                      PYTORCH_ENABLE_MPS_FALLBACK='1')
    report = dict(passed=False, certification=False, context=4096, kvCacheMiB=1536,
                  allocatorCacheMiB=512, order=ORDER, fixturesSha256=hashlib.sha256(fixture_bytes).hexdigest(),
                  trials=[])
    def identity():
        return capture_identity(inspect_model(model, backend='mlx_lm'), executable,
                                'mlx_lm', detect_hardware())

    def interrupted(*_):
        raise KeyboardInterrupt('Comparison interrupted')

    signal.signal(signal.SIGTERM, interrupted)
    with args.output.open('x') as output:
        try:
            baseline_identity = identity()
            expected = None
            for step in ORDER:
                exclusive()
                backend = BackendProcess(PrefillBackendConfig(str(model), executable, port=19233,
                    max_model_len=4096, backend_kind='mlx_lm',
                    extra_arguments=('--prefill-step-size', str(step))))
                trial = dict(step=step, cases=[])
                report['trials'].append(trial)
                guard = None
                try:
                    backend.start()
                    owned = process_tree(backend.pid)
                    guard = HostLoadGuard()
                    guard.set_owned([os.getpid(), *owned])
                    for fixture in fixtures:
                        assert detect_memory().available_bytes >= 4 * 1024**3
                        body = dict(model='default_model', messages=fixture['messages'],
                                    max_tokens=64, temperature=0, stream=False)
                        request = urllib.request.Request(backend.base_url + '/v1/chat/completions',
                            json.dumps(body).encode(), {'Content-Type': 'application/json'})
                        mark = guard.mark()
                        started = time.monotonic()
                        with urllib.request.urlopen(request, timeout=180) as response:
                            result = json.load(response)
                        elapsed = (time.monotonic() - started) * 1000
                        cpu = guard.interval(mark, guard.mark())
                        usage = result['usage']
                        answer = result['choices'][0]['message']['content']
                        assert answer.strip() == 'STRATA-7391'
                        prompt = usage['prompt_tokens']
                        cached = usage['prompt_tokens_details']['cached_tokens']
                        assert 3500 <= prompt <= 4032 and 0 <= cached <= 64
                        with urllib.request.urlopen(backend.base_url + '/vllm-apple/kv-cache', timeout=10) as response:
                            kv = json.load(response)
                        allocator = kv['allocator']
                        assert kv['max_bytes'] == 1536 * 1024**2 and kv['retained_bytes'] <= kv['max_bytes']
                        assert allocator['configured_limit_bytes'] == 512 * 1024**2 and allocator['peak_bytes'] > 0
                        physical = json.loads(subprocess.check_output([str(args.memory_probe), *map(str, owned)]))
                        assert process_tree(backend.pid) == owned and physical['processCount'] == len(owned)
                        memory = detect_memory()
                        row = dict(language=fixture['language'], answer=answer, promptTokens=prompt,
                            completionTokens=usage['completion_tokens'], cachedTokens=cached, responseMs=elapsed,
                            mlxPeakActiveBytes=allocator['peak_bytes'], postRequestPhysicalBytes=physical['physicalBytes'],
                            availableBytes=memory.available_bytes, backgroundCpu=cpu,
                            backgroundCpuBefore=mark['busyPercent'])
                        trial['cases'].append(row)
                        assert memory.available_bytes >= 4 * 1024**3 and memory.pressure.value != 'critical'
                        print(step, row['language'], round(elapsed), flush=True)
                finally:
                    if guard is not None:
                        guard.stop()
                    backend.stop()
                    trial['shutdownClean'] = not backend.running
                assert trial['shutdownClean'] and identity() == baseline_identity
                parity = [tuple(row[key] for key in ('language', 'answer', 'promptTokens',
                           'completionTokens', 'cachedTokens')) for row in trial['cases']]
                if expected is None:
                    expected = parity
                assert parity == expected, 'Answer or token count mismatch'
            report['summary'] = {}
            for step in sorted(set(ORDER)):
                rows = [row for trial in report['trials'] if trial['step'] == step for row in trial['cases']]
                report['summary'][step] = dict(requests=len(rows),
                    medianResponseMs=statistics.median(row['responseMs'] for row in rows),
                    mlxPeakActiveBytes=max(row['mlxPeakActiveBytes'] for row in rows),
                    maxPostRequestPhysicalBytes=max(row['postRequestPhysicalBytes'] for row in rows),
                    minimumAvailableBytes=min(row['availableBytes'] for row in rows),
                    latencyEligible=all(row['backgroundCpuBefore'] <= 50
                        and row['backgroundCpu']['averageBusyPercent'] <= 50
                        and row['backgroundCpu']['sustainedHighSeconds'] < 1 for row in rows))
            report['passed'] = True
        except BaseException as error:
            report['error'] = str(error)
            raise
        finally:
            json.dump(report, output, indent=2)
            output.write('\n')


if __name__ == '__main__':
    main()
