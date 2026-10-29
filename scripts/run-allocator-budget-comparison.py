"""Load-guarded symmetric micro-comparison; never issues startup approval.

Short blocks accommodate concurrent work. The selected profile still needs the
original full 30-minute certification and all Soliton integration workloads.
"""
import json
import hashlib
import importlib.util
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
from host_load_guard import HostLoadGuard

root=Path(__file__).resolve().parents[1]
os.chdir(root)
python=root/'vLLM-apple/.venv/bin/python'
paths=[]
manifest=root/'.soliton-runtime/allocator-average-background-micro-manifest-20261028.json'
expected_hashes={
    'workloadSha256':hashlib.sha256((root/'scripts/allocator-budget-trial.py').read_bytes()).hexdigest(),
    'hostGuardSha256':hashlib.sha256((root/'scripts/host_load_guard.py').read_bytes()).hexdigest(),
    'fixturesSha256':hashlib.sha256((root/'.soliton-runtime/allocator-fixtures-20261028.json').read_bytes()).hexdigest(),
}
child=None
guard=HostLoadGuard()

def interrupted(signum,frame):raise KeyboardInterrupt('comparison interrupted')
signal.signal(signal.SIGTERM,interrupted)
signal.signal(signal.SIGINT,interrupted)
try:
    for index,budget in enumerate([0,256,512,768,768,512,256,0]):
        success=False
        prefix=f'allocator-average-background-micro-20261028-{index}-{budget}-attempt'
        existing=sorted((root/'docs/benchmarks').glob(prefix+'*.json'))
        existing=[p for p in existing if '-preflight' not in p.name]
        for previous in existing:
            value=json.loads(previous.read_bytes())
            if value.get('passed') and all(value.get(k)==v for k,v in expected_hashes.items()):
                paths.append(previous);success=True
                print(f'reusing complete immutable trial: {previous.name}',flush=True)
                break
        if success:continue
        first_attempt=max([int(p.stem.split('attempt')[-1]) for p in existing],default=0)+1
        for attempt in range(first_attempt,first_attempt+3):
            stable=0;deadline=time.monotonic()+1800
            while stable<3:
                load=os.getloadavg()[0];cpu=guard.snapshot()['busyPercent']
                stable=stable+1 if cpu<=40 else 0
                print(f'waiting for quiet host: step={index}, budget={budget}, cpu={cpu:.1f}%, load1m={load:.2f}, stable={stable}/3',flush=True)
                assert time.monotonic()<deadline,'host never became quiet; no performance claim permitted'
                time.sleep(5)
            out=root/f'docs/benchmarks/allocator-average-background-micro-20261028-{index}-{budget}-attempt{attempt}.json'
            assert not out.exists()
            child=subprocess.Popen([str(python),'-u',str(root/'scripts/allocator-budget-trial.py'),str(budget),str(out),'--micro'],start_new_session=True)
            try:status=child.wait(timeout=900)
            finally:
                if child.poll() is None:
                    child.send_signal(signal.SIGTERM)
                    try:child.wait(timeout=15)
                    except subprocess.TimeoutExpired:
                        os.killpg(child.pid,signal.SIGKILL);child.wait()
                child=None
            if status==0:
                value=json.loads(out.read_bytes());assert value['passed']
                cpu_values=[c['cpuAveragePercent'] for c in value['cases'] if c['phase']!='prime']+[s['cpuBusyPercent'] for s in value['samples']]
                if max(cpu_values)<=50:
                    paths.append(out);success=True;break
            print(f'trial excluded: step={index}, budget={budget}, attempt={attempt}',flush=True)
        assert success,'three invalid attempts; retain originals and do not choose a profile'
    if manifest.exists():assert json.loads(manifest.read_bytes())==list(map(str,paths))
    else:
        with manifest.open('x') as file:
            manifest.chmod(0o600);json.dump([str(p) for p in paths],file,indent=2)
    output=root/'docs/evaluation/allocator-tuning-comparison-20261028.json'
    if output.exists():
        spec=importlib.util.spec_from_file_location('comparison',root/'scripts/compare-allocator-budgets.py')
        module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
        replay=module.compare(paths);saved=json.loads(output.read_bytes())
        assert replay['groups']==saved['groups'] and replay['sha256']==saved['sha256']
        print(json.dumps(saved),flush=True)
    else:subprocess.run([sys.executable,str(root/'scripts/compare-allocator-budgets.py'),str(output),*map(str,paths)],check=True)
finally:
    guard.stop()
    if child is not None and child.poll() is None:
        child.send_signal(signal.SIGTERM)
        try:child.wait(timeout=15)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid,signal.SIGKILL);child.wait()
