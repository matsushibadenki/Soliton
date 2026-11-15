"""Compare two separately qualified profiles through the isolated Rust gateway."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import runpy
import signal
import statistics
import subprocess
import threading
import time
import urllib.request

helpers = runpy.run_path(str(Path(__file__).with_name('compare-kv-retention.py')))
ROOT = Path(__file__).resolve().parents[1]
ORDER = (2048, 512, 512, 2048)


class BackgroundLoadExceeded(RuntimeError):
    pass


def latency_eligible(row):
    cpu = row['backgroundCpu']
    return (row['backgroundCpuBefore'] <= 50 and cpu['averageBusyPercent'] <= 50
            and cpu['sustainedHighSeconds'] < 1)


def wait_for_quiet(guard):
    deadline = time.monotonic() + 90
    consecutive = 0
    while time.monotonic() < deadline:
        consecutive = consecutive + 1 if guard.snapshot()['busyPercent'] <= 35 else 0
        if consecutive == 4:
            return
        time.sleep(.5)
    raise BackgroundLoadExceeded('CPU load did not settle before request')


def measure(api, headers, host, probe, fixture, guard, *, fresh_long=True):
    request = urllib.request.Request(api + '/v1/chat/completions', json.dumps(dict(
        model='default_model', messages=fixture['messages'], max_tokens=64,
        temperature=0, stream=True, stream_options={'include_usage': True},
    )).encode(), headers)
    stop = threading.Event()

    def sample():
        physical = []
        missed = 0
        while not stop.is_set():
            owned = helpers['process_tree'](host.pid)
            try:
                row = json.loads(subprocess.check_output([str(probe), *map(str, owned)], timeout=5))
                if row['processCount'] == len(owned) and helpers['process_tree'](host.pid) == owned:
                    physical.append(row['physicalBytes'])
                else:
                    missed += 1
            except subprocess.CalledProcessError as error:
                if error.returncode != 3:
                    raise
                missed += 1
            stop.wait(.10 if fresh_long else .02)
        assert len(physical) >= 2 and min(physical) > 0 and missed <= max(0 if fresh_long else 1, len(physical) / 10)
        return max(physical), len(physical), missed

    assert helpers['detect_memory']().available_bytes >= 4 * 1024**3
    answer, first, usage, done = '', None, None, False
    with ThreadPoolExecutor(max_workers=1) as pool:
        memory = pool.submit(sample)
        mark = guard.mark()
        start = time.monotonic()
        try:
            with urllib.request.urlopen(request, timeout=180) as response:
                for line in response:
                    if not line.startswith(b'data:'):
                        continue
                    data = line[5:].strip()
                    if data == b'[DONE]':
                        done = True
                        continue
                    event = json.loads(data)
                    usage = event.get('usage') or usage
                    for choice in event.get('choices', []):
                        content = choice.get('delta', {}).get('content')
                        if content:
                            if first is None:
                                first = (time.monotonic() - start) * 1000
                            answer += content
            elapsed = (time.monotonic() - start) * 1000
        finally:
            stop.set()
        peak, count, missed = memory.result()
    cpu = guard.interval(mark, guard.mark())
    assert done and first is not None and answer.strip()
    prompt = usage['prompt_tokens']
    cached = usage['prompt_tokens_details']['cached_tokens']
    assert 0 < prompt <= 4032 and 0 <= cached <= prompt
    if fresh_long:
        assert prompt >= 3500 and cached <= 64 and answer.strip() == 'STRATA-7391'
    available = helpers['detect_memory']().available_bytes
    assert available >= 4 * 1024**3
    return dict(language=fixture['language'], answer=answer, promptTokens=prompt,
                completionTokens=usage['completion_tokens'], cachedTokens=cached,
                firstContentMs=first, responseMs=elapsed, sampledPeakPhysicalBytes=peak,
                samples=count, missedSamples=missed, availableBytes=available, backgroundCpu=cpu,
                backgroundCpuBefore=mark['busyPercent'])


def trial(args, step, index, rows):
    helpers['exclusive']()
    source = args.baseline if step == 2048 else args.candidate
    state_path = args.output.with_suffix(f'.{index}.state.json')
    with state_path.open('xb') as state_file:
        state_file.write(source.read_bytes())
    api = 'http://127.0.0.1:18099'
    headers = {'Content-Type': 'application/json'}

    def call(path, body=None):
        request = urllib.request.Request(api + path, None if body is None else
                                         json.dumps(body).encode(), headers)
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response)

    with args.output.with_suffix(f'.{index}.log').open('xb') as log:
        host = subprocess.Popen([ROOT / 'target/debug/studio-server'], stdout=log, stderr=log,
            env=dict(os.environ, STUDIO_PORT='18099', STUDIO_STATE_PATH=str(state_path)))
        started = False
        guard = None
        try:
            headers['Origin'] = api
            for _ in range(100):
                assert host.poll() is None
                try:
                    session = call('/studio/session', {})
                    break
                except OSError:
                    time.sleep(.1)
            else:
                raise RuntimeError('Isolated gateway unavailable')
            headers['x-studio-token'] = session['token']
            initial = call('/studio/state')
            assert initial['status'] == 'offline'
            call('/studio/action', {'action': 'start'})
            started = True
            for _ in range(300):
                ready = call('/studio/state')
                if ready['status'] == 'ready':
                    break
                if ready['status'] == 'offline':
                    raise RuntimeError('Qualified engine startup refused')
                time.sleep(.25)
            else:
                raise RuntimeError('Engine startup timed out')
            guard = helpers['HostLoadGuard']()
            guard.set_owned([os.getpid(), *helpers['process_tree'](host.pid)])
            for fixture in args.fixtures:
                if args.require_quiet:
                    wait_for_quiet(guard)
                row = measure(api, headers, host, args.memory_probe, fixture, guard)
                rows.append(row)
                if args.require_quiet and not latency_eligible(row):
                    raise BackgroundLoadExceeded(f"CPU baseline exceeded for {fixture['language']}")
                print(step, fixture['language'], round(row['firstContentMs']),
                      round(row['responseMs']), row['sampledPeakPhysicalBytes'], flush=True)
            final = call('/studio/state')
            assert final['engineSession'] == ready['engineSession']
            assert final['document']['messages'] == initial['document']['messages']
        finally:
            try:
                if guard is not None:
                    guard.stop()
                if started:
                    call('/studio/action', {'action': 'stop'})
                    assert call('/studio/state')['status'] == 'offline'
            finally:
                if host.poll() is None:
                    host.send_signal(signal.SIGINT)
                    host.wait(timeout=30)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for key in ('baseline', 'candidate', 'fixtures', 'memory-probe', 'output'):
        parser.add_argument('--' + key, type=Path, required=True)
    parser.add_argument('--require-quiet', action='store_true')
    args = parser.parse_args()
    args.output = args.output.resolve()
    args.fixtures = json.loads(args.fixtures.read_bytes())
    assert len(args.fixtures) == 6
    evidence_digests = {}
    for step, path in ((2048, args.baseline), (512, args.candidate)):
        profile = json.loads(path.read_bytes())['settings']['vllm']
        assert profile['context'] == 4096 and profile['kvCacheMiB'] == 1536
        assert profile['allocatorCacheMiB'] == 512
        evidence = Path(profile['evidence']).read_bytes()
        qualification = json.loads(evidence)
        assert qualification['passed'] and qualification['shutdown_clean']
        assert qualification['architecture_evidence']['context_tokens'] == 4096
        assert qualification['soak']['elapsed_seconds'] >= 1800
        evidence_digests[step] = hashlib.sha256(evidence).hexdigest()
    report = dict(passed=False, order=ORDER, context=4096, evidenceSha256=evidence_digests, trials=[], rejectedTrials=[])
    with args.output.open('x') as output:
        try:
            reference = None
            for index, step in enumerate(ORDER):
                for attempt in range(1, 9):
                    rows = []
                    try:
                        trial(args, step, f'{index}-{attempt}', rows)
                    except BackgroundLoadExceeded as error:
                        report['rejectedTrials'].append(dict(step=step, attempt=attempt,
                            reason=str(error), cases=rows))
                        print(f'{step} trial {index}, attempt {attempt}: {error}', flush=True)
                        if attempt == 8:
                            raise
                        time.sleep(5)
                    else:
                        report['trials'].append(dict(step=step, cases=rows))
                        break
                parity = [tuple(row[key] for key in ('language', 'answer', 'promptTokens',
                           'completionTokens', 'cachedTokens')) for row in rows]
                if reference is None:
                    reference = parity
                assert reference == parity, 'Answer or token count mismatch'
            report['summary'] = {}
            for step in sorted(set(ORDER)):
                rows = [row for trial_report in report['trials'] if trial_report['step'] == step
                        for row in trial_report['cases']]
                report['summary'][step] = dict(requests=len(rows),
                    medianFirstContentMs=statistics.median(row['firstContentMs'] for row in rows),
                    medianResponseMs=statistics.median(row['responseMs'] for row in rows),
                    peakPhysicalBytes=max(row['sampledPeakPhysicalBytes'] for row in rows),
                    latencyEligible=all(map(latency_eligible, rows)))
            report['passed'] = True
        except BaseException as error:
            report['error'] = str(error)
            raise
        finally:
            json.dump(report, output, indent=2)
            output.write('\n')


if __name__ == '__main__':
    main()
