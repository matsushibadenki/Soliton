"""Serial ABBA comparison of explicit local SSD worker versions. No ordinary app state.
Measures client first-content latency and full SSE completion, not kernel/physical SSD I/O.
"""
import argparse
import hashlib
import http.client
import json
import math
import os
from pathlib import Path
import secrets
import signal
import statistics
import subprocess
import time
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for flag in ['python', 'model', 'baseline-runtime', 'baseline-store', 'runtime', 'store', 'output-dir']:
        parser.add_argument('--' + flag, required=True, type=Path)
    parser.add_argument('--port', type=int, default=18083)
    parser.add_argument('--repeats', type=int, default=20)
    args = parser.parse_args()
    if not 10 <= args.repeats <= 100:
        parser.error('Use 10–100 repetitions')
    args.output_dir.mkdir(mode=0o700, parents=False, exist_ok=False)
    token = secrets.token_hex(24)
    headers = {'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'}
    env = dict(os.environ, SOLITON_PREFIX_HTTP_TOKEN=token)
    for key in list(env):
        if key.startswith(('MLX_', 'METAL_', 'VECLIB_', 'OMP_', 'TOKENIZERS_')) or key in {'PYTORCH_ENABLE_MPS_FALLBACK', 'PYTHONPATH', 'PYTHONHOME', 'PYTHONEXECUTABLE', '__PYVENV_LAUNCHER__'}:
            env.pop(key, None)
    base = f'http://127.0.0.1:{args.port}'
    prompt = 'The fictional code word is soliton. ' * 20 + 'What is the code word? Answer briefly.'
    prompts = [prompt, 'What is 2 + 2? Answer with only the number.']
    answers = {}
    samples = []
    worker = None
    log = None

    def get():
        with urllib.request.urlopen(urllib.request.Request(base + '/studio/ssd-cache', headers=headers), timeout=5) as reply:
            return json.load(reply)

    def stop():
        nonlocal worker, log
        if worker is not None:
            try:
                if worker.poll() is None:
                    os.killpg(worker.pid, signal.SIGINT)
                worker.wait(timeout=15)
            except subprocess.TimeoutExpired:
                os.killpg(worker.pid, signal.SIGKILL)
                worker.wait()
                raise RuntimeError('Worker shutdown timed out')
            finally:
                worker = None
                log.close()

    def start(runtime, store, cache, label):
        nonlocal worker, log
        log = (args.output_dir / (label + '.log')).open('x')
        began = time.perf_counter()
        worker = subprocess.Popen([str(args.python), str(runtime / 'prefix-cache-http.py'), '--model', str(args.model),
                                   '--cache-root', str(cache), '--quota-mib', '256', '--store-executable', str(store),
                                   '--port', str(args.port), '--qualification-mode'], env=env, stdout=log, stderr=log, start_new_session=True)
        for _ in range(240):
            if worker.poll() is not None:
                raise RuntimeError('Benchmark worker failed: ' + label)
            try:
                result = get()
                return time.perf_counter() - began, result['identity']
            except OSError:
                time.sleep(.25)
        raise RuntimeError('Readiness timeout')

    def files(cache):
        return {p.name: (p.stat().st_ino, p.stat().st_mtime_ns, p.stat().st_size) for p in (cache / 'store').glob('*.cache')}

    def infer(version, run, cache, scenario, index):
        before, previous = get(), files(cache)
        query = dict(model='default_model', messages=[dict(role='user', content=prompts[index])], temperature=0, seed=0,
                     max_tokens=16, stream=True, stream_options={'include_usage': True})
        connection = http.client.HTTPConnection('127.0.0.1', args.port, timeout=60)
        began = time.perf_counter()
        first = None
        text = ''
        usage = None
        try:
            connection.request('POST', '/v1/chat/completions', json.dumps(query), headers)
            response = connection.getresponse()
            if response.status != 200:
                raise RuntimeError(f'Inference failed: {response.status}')
            for raw in response:
                if not raw.startswith(b'data:'):
                    continue
                data = raw[5:].strip()
                if data == b'[DONE]':
                    break
                frame = json.loads(data)
                content = (frame.get('choices') or [{}])[0].get('delta', {}).get('content', '')
                if content:
                    if first is None:
                        first = time.perf_counter() - began
                    text += content
                if frame.get('usage'):
                    usage = frame['usage']
            elapsed = time.perf_counter() - began
        finally:
            connection.close()
        if first is None or usage is None:
            raise RuntimeError('Missing content or usage')
        if index in answers and text != answers[index]:
            raise RuntimeError('Answer parity failed')
        answers[index] = text
        if (index == 0 and 'soliton' not in text.lower()) or (index == 1 and text.strip() != '4'):
            raise RuntimeError('Incorrect answer')
        after = get()
        current = files(cache)
        changed = [value[2] - 4104 for name, value in current.items() if previous.get(name) != value]
        delta = {k: after[k] - before.get(k, 0) for k in after if type(after[k]) in (int, float)}
        # A request may commit only its own prefix once. Payload bytes exclude the Rust header.
        assert delta['writes'] == len(changed)
        if 'committedBytes' in delta:
            assert delta['committedBytes'] == sum(changed)
        rss = int(subprocess.check_output(['/bin/ps', '-o', 'rss=', '-p', str(worker.pid)], text=True).strip()) * 1024
        samples.append(dict(version=version, run=run, scenario=scenario, prompt=index,
                            first_content_seconds=first, completion_seconds=elapsed, rss_bytes=rss,
                            committed_payload_bytes=sum(changed), counters=delta, usage=usage))

    runs = []
    try:
        for run, version in enumerate(['baseline', 'optimized', 'optimized', 'baseline']):
            runtime = args.baseline_runtime if version == 'baseline' else args.runtime
            store = args.baseline_store if version == 'baseline' else args.store
            cache = args.output_dir / ('cache-' + str(run))
            cache.mkdir(mode=0o700)
            startup, identity = start(runtime, store, cache, f'{run}-initial')
            infer(version, run, cache, 'cold', 0)
            for _ in range(args.repeats):
                infer(version, run, cache, 'repeated', 0)
            for i in range(args.repeats):
                infer(version, run, cache, 'alternating', i % 2)
            stop()
            restart, again = start(runtime, store, cache, f'{run}-restart')
            assert again == identity
            infer(version, run, cache, 'restart', 0)
            assert samples[-1]['usage']['prompt_tokens_details']['cached_tokens'] == 177
            stop()
            runs.append(dict(run=run, version=version, identity=identity, startup_seconds=startup, restart_seconds=restart,
                             worker_sha256=hashlib.sha256((runtime / 'prefix-cache-http.py').read_bytes()).hexdigest(),
                             store_sha256=hashlib.sha256(store.read_bytes()).hexdigest()))
            print(json.dumps(dict(completed_run=run, version=version)), flush=True)
    finally:
        stop()
    summary = {}
    for version in ['baseline', 'optimized']:
        summary[version] = {}
        for scenario in ['cold', 'repeated', 'alternating', 'restart']:
            subset = [s for s in samples if s['version'] == version and s['scenario'] == scenario]
            def latency(key):
                values = sorted(s[key] * 1000 for s in subset)
                return dict(median_ms=statistics.median(values), p95_ms=values[math.ceil(.95 * len(values)) - 1])
            summary[version][scenario] = dict(requests=len(subset), first_content=latency('first_content_seconds'),
                completion=latency('completion_seconds'), writes=sum(s['counters']['writes'] for s in subset),
                skipped_writes=sum(s['counters'].get('skippedWrites', 0) for s in subset),
                committed_payload_bytes=sum(s['committed_payload_bytes'] for s in subset),
                rss_sample_max_bytes=max(s['rss_bytes'] for s in subset))
    report = dict(result='passed', order='ABBA', repetitions_per_scenario_per_run=args.repeats,
                  scope='Direct authenticated isolated worker SSE; excludes Rust gateway and startup from request latency',
                  limits='One Mac, serial synthetic workload, OS page cache uncontrolled; committed payload is logical I/O, not physical NAND writes; RSS is sampled',
                  answers=answers, runs=runs, summary=summary, samples=samples)
    path = args.output_dir / 'comparison.json'
    path.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(summary, indent=2), flush=True)


if __name__ == '__main__':
    main()
