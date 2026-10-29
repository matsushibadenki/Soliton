"""Sequential qualification-only prefill experiment; never adopts preferences."""
import hashlib
import fcntl
import importlib.util
import json
import os
import signal
from pathlib import Path
import subprocess
import sys
import time
from host_load_guard import HostLoadGuard

ROOT = Path(__file__).resolve().parents[1]
os.chdir(ROOT)
# The kernel releases this lock on normal exit, interruption or crash.
coordinator_lock = (ROOT / '.soliton-runtime/active-retention-comparison.lock').open('a')
try:
    fcntl.flock(coordinator_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
except BlockingIOError:
    raise SystemExit('another active-retention coordinator owns the experiment')
coordinator_lock.seek(0)
coordinator_lock.truncate()
coordinator_lock.write(str(os.getpid()))
coordinator_lock.flush()
expected_hashes = {
    key: hashlib.sha256((ROOT / name).read_bytes()).hexdigest()
    for key, name in [
        ('workloadSha256', 'scripts/active-retention-trial.py'),
        ('hostGuardSha256', 'scripts/host_load_guard.py'),
        ('fixturesSha256', '.soliton-runtime/allocator-fixtures-20261028.json'),
    ]
}
manifest = ROOT / 'docs/evaluation/active-retention-comparison-20261028.json'
assert not manifest.exists()
# The already running diagnostic owns the model until its report is written.
diagnostic = ROOT / 'docs/benchmarks/active-retention-baseline-20261028.json'
deadline = time.monotonic() + 1800
while not diagnostic.exists():
    assert time.monotonic() < deadline, 'diagnostic still running; no second model'
    time.sleep(5)
child = None
def interrupted(signum, frame):
    if child is not None and child.poll() is None:
        os.killpg(child.pid, signal.SIGTERM)
        try:
            child.wait(timeout=20)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait()
    raise KeyboardInterrupt('owned comparison interrupted')
signal.signal(signal.SIGTERM, interrupted)
signal.signal(signal.SIGINT, interrupted)
paths = []
for index, step in enumerate([2048, 512, 512, 2048]):
    valid = False
    existing = [p for p in (ROOT / 'docs/benchmarks').glob(f'active-retention-prefill-20261028-{index}-{step}-attempt*.json') if '-preflight' not in p.name]
    for p in existing:
        value = json.loads(p.read_bytes())
        if value.get('passed'):
            assert all(value[k] == h for k, h in expected_hashes.items()), 'frozen experiment inputs changed'
            assert value['prefillStepSize'] == step and value['allocatorCacheMiB'] == 512 and value['kvCacheMiB'] == 1024
            assert value['identityBefore'] == value['identityAfter']
            assert value['shutdownClean'] and value['counterStopped'] and value['userStatePreserved']
            paths.append(p)
            valid = True
            break
    if valid:
        continue
    first_attempt = max([int(p.stem.split('attempt')[-1]) for p in existing], default=0) + 1
    for attempt in range(first_attempt, first_attempt + 3):
        guard = HostLoadGuard()
        stable = 0
        deadline = time.monotonic() + 1800
        try:
            while stable < 6:
                cpu = guard.snapshot()['busyPercent']
                stable = stable + 1 if cpu <= 20 else 0
                print(f'quiet gate step={step} CPU={cpu:.1f}% stable={stable}/6', flush=True)
                assert time.monotonic() < deadline, 'busy host; no comparison claim'
                time.sleep(5)
        finally:
            guard.stop()
        out = ROOT / f'docs/benchmarks/active-retention-prefill-20261028-{index}-{step}-attempt{attempt}.json'
        assert not out.exists()
        child = subprocess.Popen([str(ROOT / 'vLLM-apple/.venv/bin/python'), '-u', str(ROOT / 'scripts/active-retention-trial.py'), '512', str(out), '--micro', f'--prefill-step-size={step}'], start_new_session=True)
        try:
            status = child.wait(timeout=900)
        except subprocess.TimeoutExpired:
            interrupted(signal.SIGTERM, None)
        finally:
            child = None
        if status == 0:
            value = json.loads(out.read_bytes())
            assert value['passed'] and value['prefillStepSize'] == step
            paths.append(out)
            valid = True
            break
        print(f'excluded failed trial {out.name}; cooling down before quiet gate', flush=True)
        time.sleep(60)
    assert valid, 'three rejected trials; preserve evidence, do not select'
spec = importlib.util.spec_from_file_location('pilot', ROOT / 'scripts/compare-allocator-budgets.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
reports = [json.loads(p.read_bytes()) for p in paths]
for key in ['identityBefore', 'workloadSha256', 'hostGuardSha256', 'fixturesSha256']:
    assert all(x[key] == reports[0][key] for x in reports), f'changed {key}; exclude cohort'
signature = lambda x: [(c['phase'], c['index'], c['promptTokens']) for c in x['cases']]
assert all(signature(x) == signature(reports[0]) for x in reports), 'different exact token workload'
runs = []
for p, value in zip(paths, reports):
    summary = module.summarize(value)
    assert max([c['cpuAveragePercent'] for c in value['cases'] if c['phase'] != 'prime'] + [s['cpuBusyPercent'] for s in value['samples']]) <= 50
    assert all(c['cpuSustainedHighSeconds'] < 1 for c in value['cases'] if c['phase'] != 'prime')
    runs.append(dict(path=str(p), sha256=hashlib.sha256(p.read_bytes()).hexdigest(), prefillStepSize=value['prefillStepSize'], activePeakBytes=max(s['kvCache']['allocator']['active_bytes'] for s in value['samples']), idleActiveBytes=value['samples'][-1]['kvCache']['allocator']['active_bytes'], **summary))
result = dict(passed=True, certification=False, scope='diagnostic prefill comparison; no startup approval', runs=runs, limitations=['Not shared Rust API latency.', 'Parallel desktop work remains running; thermal/load variation remains.', 'No adoption without fresh full qualification and Soliton integration.'])
with manifest.open('x') as f:
    manifest.chmod(0o600)
    json.dump(result, f, indent=2)
print(json.dumps(result), flush=True)
