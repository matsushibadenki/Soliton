"""Verify freshly downloaded Q2_0 shards against the pinned upstream SHA-256 values."""
import argparse
import hashlib
import json
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('directory', type=Path)
parser.add_argument('--shard', type=int, choices=[1,2], help='Verify just one completed shard')
args = parser.parse_args()
expected = [
    ('Qwen3.8-Flash-Next-GSQ-RCO-Q2_0-00001-of-00002.gguf', 37623740192, '69820c02ec7d0b45ef2ebb19d6620299db749fe2aded7f39f93c6b88b199b720'),
    ('Qwen3.8-Flash-Next-GSQ-RCO-Q2_0-00002-of-00002.gguf', 28800138432, '316b46f3a2dbd68c900f43136ab9449f9dcc3725dfd8c794847c204bc161e113'),
]
report = {'repository': 'ISTA-DASLab/Qwen3.8-Flash-Next-GSQ-RCO-GGUF', 'revision': 'ed59f92082b1e93c0e96d60a8b11aab089b52f09', 'files': []}
for index, (name, size, digest) in enumerate(expected, 1):
    if args.shard and args.shard != index:
        continue
    path = args.directory / name
    if path.is_symlink() or path.stat().st_size != size:
        raise RuntimeError(f'Expected a new complete regular file: {name}')
    with path.open('rb') as source:
        actual = hashlib.file_digest(source, 'sha256').hexdigest()
    if actual != digest:
        raise RuntimeError(f'SHA-256 mismatch: {name}')
    report['files'].append({'name': name, 'bytes': size, 'sha256': actual})
print(json.dumps(report, indent=2))
