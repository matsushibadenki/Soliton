"""Alternating full-byte fingerprint trials, never caches results or approvals."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import time

parser = argparse.ArgumentParser(__doc__)
for key in ['python', 'executable', 'before', 'after', 'output']:
    parser.add_argument('--'+key, type=Path, required=True)
parser.add_argument('--rounds', type=int, default=3, choices=range(1,6))
args = parser.parse_args()
if args.output.exists(): parser.error('output must be new')
if args.python.absolute().parent.resolve() != args.executable.absolute().parent.resolve():
    parser.error('use the executable adjacent Python to preserve the backend environment')
report = {'schema':1, 'passed':False, 'trials':[], 'sources':{}}
for label in ['before','after']:
    source = getattr(args,label).resolve()
    report['sources'][label] = hashlib.sha256(source.read_bytes()).hexdigest()
for trial in range(args.rounds):
    for label in (['before','after'] if trial % 2 == 0 else ['after','before']):
        load = os.getloadavg()
        start = time.perf_counter()
        # Keep the interpreter symlink: resolving it would discard the venv.
        result = subprocess.run([str(args.python.absolute()), '-I', str(getattr(args,label).resolve()), str(args.executable.absolute())], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=120, check=True)
        elapsed = time.perf_counter()-start
        digest = result.stdout.decode('ascii').strip()
        assert len(digest)==64 and all(c in '0123456789abcdef' for c in digest)
        report['trials'].append(dict(trial=trial, label=label, elapsedSeconds=elapsed, digest=digest, loadAverageAtStart=load))
        print(f'{label} {trial+1}/{args.rounds}: {elapsed:.3f}s',flush=True)
assert len({r['digest'] for r in report['trials']})==1, 'fingerprint changed'
for label in ['before','after']:
    assert hashlib.sha256(getattr(args,label).resolve().read_bytes()).hexdigest()==report['sources'][label], 'measurement source changed'
report['medianSeconds']={label:statistics.median(r['elapsedSeconds'] for r in report['trials'] if r['label']==label) for label in ['before','after']}
report['reductionPercent']=100*(1-report['medianSeconds']['after']/report['medianSeconds']['before'])
report['passed']=report['reductionPercent']>0
report['limitations']=['Complete content reread on every trial; OS caches retained, ordinary desktop load.', 'These are CPU fingerprint trials, not end-to-end engine startup measurements.']
with args.output.open('x') as file:
    os.chmod(args.output,0o600)
    json.dump(report,file,indent=2);file.write('\n')
assert report['passed'], 'no measured improvement'
