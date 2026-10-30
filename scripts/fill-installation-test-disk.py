"""Fill only a small, separately mounted disposable disk; keep filler for GUI testing."""
import argparse
import errno
import json
import os
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('mount', type=Path)
args = parser.parse_args()
mount = args.mount.resolve()
if not mount.is_mount():
    raise RuntimeError('Refusing to fill a directory that is not a mount point')
if mount.stat().st_dev == mount.parent.stat().st_dev:
    raise RuntimeError('Refusing to fill the host volume')
stats = os.statvfs(mount)
if stats.f_blocks * stats.f_frsize >= 5 * 1024**3:
    raise RuntimeError('Test volume must be smaller than 5 GiB')
(mount / 'data').mkdir(exist_ok=True)
filler = mount / 'soliton-disposable-capacity-fill.bin'
written = 0
try:
    with filler.open('xb', buffering=0) as output:
        block = b'\x5a' * (8 * 1024**2)
        while written < 5 * 1024**3:
            written += output.write(block)
        raise RuntimeError('Capacity limit was not reached')
except OSError as error:
    if error.errno != errno.ENOSPC:
        raise
    print(json.dumps({'errno': error.errno, 'error': 'ENOSPC', 'writtenBytes': written, 'mount': str(mount), 'filler': str(filler)}))
