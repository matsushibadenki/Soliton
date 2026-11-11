"""Compare two prefix-store builds on identical opaque SSD payloads."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--debug', type=Path, required=True)
parser.add_argument('--release', type=Path, required=True)
args = parser.parse_args()
binaries = {mode: path.resolve(strict=True)
            for mode, path in vars(args).items()}
samples = {mode: {action: [] for action in ('put', 'get')} for mode in binaries}
with tempfile.TemporaryDirectory(prefix='soliton-cli-compare-') as directory:
    root = Path(directory)
    payload = root / 'payload'
    payload_bytes = 27270312  # Size of the previous Gemma2 smoke snapshot.
    pattern = bytes(range(256))
    payload.write_bytes((pattern * ((payload_bytes + 255) // 256))[:payload_bytes])
    expected = hashlib.sha256(payload.read_bytes()).digest()
    manifest = root / 'identity.json'
    manifest.write_text(json.dumps(dict(model='a'*64, tokenizer='b'*64, runtime='c'*64,
                                        prefix='d'*64, format='opaque-test-v1')))
    for index, mode in enumerate(('debug','release','release','debug') * 3):
        store = root / f'store-{index}'
        restored = root / f'restored-{index}'
        receipts = []
        for action, path in [('put',payload),('get',restored)]:
            started = time.perf_counter_ns()
            result = subprocess.run([binaries[mode], store, '268435456', action, manifest, path],
                                    env=dict(os.environ, SOLITON_PREFIX_STORE_TIMINGS='1'),
                                    capture_output=True, check=True, timeout=20)
            wall = time.perf_counter_ns() - started
            timing = json.loads(result.stderr)
            row = dict(wallNs=wall, outsideMainNs=wall-timing['mainNs'], **timing)
            samples[mode][action].append(row)
            receipts.append(json.loads(result.stdout))
        assert receipts[0] == receipts[1]
        assert receipts[0]['bytes'] == payload.stat().st_size
        assert hashlib.sha256(restored.read_bytes()).digest() == expected
summary = {}
for mode, actions in samples.items():
    summary[mode] = {}
    for action, rows in actions.items():
        fields = [{**{k:v for k,v in row.items() if k!='phases'}, **row['phases']} for row in rows]
        summary[mode][action] = {k: round(statistics.median(row[k] for row in fields)/1e6,3)
                                 for k in fields[0]}
report = dict(payloadBytes=payload_bytes, trialsPerBuild=6, order='ABBA x3', medianMs=summary, samples=samples)
print(json.dumps(report, indent=2))
