"""Identity-bound qualification contract for Soliton's MLX SSD engine."""
import hashlib
import json
import math
import os
import plistlib
import re
import selectors
import subprocess
import sys
import uuid
from pathlib import Path
import stat
import time

PROFILE = dict(schema=2, engine='mlxPrefix', context=512, maximum_output_tokens=128,
               concurrency=1, temperature=0, seed=0, memory_cache_bytes=64*1024**2,
               allocator_cache_bytes=256*1024**2)
REQUIRED_GATES = {'authentication', 'request_rejection', 'cold', 'restart_hit', 'corruption_fallback',
                  'disconnect_recovery', 'quota_fallback', 'context_boundary'}


BINDING_SCHEMA = 2


def binding_digest(bound):
    return hashlib.sha256(json.dumps(bound, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def _diskutil_info(node):
    # Fixed OS executable/read-only verb; bounded output and deadline, no shell expansion.
    process = subprocess.Popen(['/usr/sbin/diskutil', 'info', '-plist', node],
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    output = bytearray(); deadline = time.monotonic()+3
    try:
        while True:
            remaining = deadline-time.monotonic()
            if remaining <= 0: raise ValueError('Volume identity probe timed out')
            events = selector.select(remaining)
            if not events: raise ValueError('Volume identity probe timed out')
            chunk = os.read(process.stdout.fileno(), min(8192, 65537-len(output)))
            if not chunk: break
            output.extend(chunk)
            if len(output)>65536: raise ValueError('Volume identity response exceeds64KiB')
        process.wait(timeout=max(.01, deadline-time.monotonic()))
        if process.returncode: raise ValueError('Volume identity unavailable from DiskManagement')
        data=plistlib.loads(output)
        if not isinstance(data,dict): raise ValueError('Invalid volume identity response')
        return data
    finally:
        selector.close();process.stdout.close()
        if process.poll() is None:
            try: process.kill()
            except ProcessLookupError: pass
            process.wait(timeout=3)


def _device_node(device):
    matches=[]
    for index,path in enumerate(Path('/dev').glob('disk*')):
        if index>=512: raise ValueError('Device inventory exceeds reviewed bound')
        try: info=path.lstat()
        except FileNotFoundError: continue
        if stat.S_ISBLK(info.st_mode) and info.st_rdev==device:
            matches.append(str(path))
    if len(matches)!=1: raise ValueError('Cannot uniquely resolve cache volume device')
    return matches[0]


def _volume_info(data,node,device):
    if data.get('DeviceNode')!=node: raise ValueError('Volume response device mismatch')
    mount=data.get('MountPoint')
    if not isinstance(mount,str) or not mount.startswith('/') or len(mount)>4096:
        raise ValueError('Invalid volume mount point')
    if os.stat(mount).st_dev!=device: raise ValueError('Volume mount does not contain cache')
    raw=data.get('VolumeUUID')
    if not isinstance(raw,str): raise ValueError('Volume UUID unavailable')
    parsed=uuid.UUID(raw)
    if str(parsed)!=raw.lower() or parsed.int==0: raise ValueError('Invalid volume UUID')
    filesystem=data.get('FilesystemType');protocol=data.get('BusProtocol')
    if filesystem not in {'apfs','hfs'} or not isinstance(protocol,str) or not 1<=len(protocol)<=128:
        raise ValueError('Unreviewed cache volume metadata')
    if type(data.get('Internal')) is not bool or type(data.get('SolidState')) is not bool or data.get('WritableVolume') is not True:
        raise ValueError('Cache volume must report storage kind and be writable')
    return dict(volume_uuid_sha256=hashlib.sha256(str(parsed).encode()).hexdigest(),filesystem=filesystem,
                internal=data['Internal'],solid_state=data['SolidState'],bus_protocol=protocol)


def volume_snapshot(root):
    if sys.platform!='darwin': raise ValueError('Volume binding v2 requires reviewed macOS storage APIs')
    root=Path(root)
    if not root.is_absolute(): raise ValueError('Select an absolute cache root')
    before=root.lstat();canonical=root.resolve(strict=True)
    if not stat.S_ISDIR(before.st_mode) or stat.S_ISLNK(before.st_mode) or before.st_uid!=os.getuid() or before.st_mode&0o077:
        raise ValueError('Cache root must be a real owner-only directory')
    node=_device_node(before.st_dev)
    data=_diskutil_info(node)
    volume=_volume_info(data,node,before.st_dev)
    after=root.lstat()
    if (before.st_dev,before.st_ino,before.st_mode,before.st_uid)!=(after.st_dev,after.st_ino,after.st_mode,after.st_uid) or root.resolve(strict=True)!=canonical:
        raise ValueError('Cache root changed during volume probe; retry')
    # Check that a renamed/replaced device node did not redirect the query.
    node_info=os.lstat(node)
    if not stat.S_ISBLK(node_info.st_mode) or node_info.st_rdev!=before.st_dev:
        raise ValueError('Volume device changed during probe; retry')
    return volume,before.st_dev


def identity_binding(identity, root, quota):
    volume,device=volume_snapshot(root)
    bound=dict(binding_schema=BINDING_SCHEMA,identity=identity,profile=PROFILE,
               cache_root=str(Path(root).resolve(strict=True)),cache_volume=volume,quota_bytes=quota)
    return bound,device


def identity_digest(identity, root, quota):
    return binding_digest(identity_binding(identity,root,quota)[0])


def verify_evidence(path, identity, quota):
    flags = os.O_RDONLY | getattr(os, 'O_NOFOLLOW', 0) | getattr(os, 'O_NONBLOCK', 0)
    fd = os.open(path, flags)
    with os.fdopen(fd, 'rb') as file:
        info = os.fstat(file.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077 or info.st_size > 65536:
            raise ValueError('Qualification evidence must be a bounded, owner-only regular file')
        data = file.read(65537)
        if len(data)>65536: raise ValueError("Evidence grew beyond its bound")
        report = json.loads(data)
    if report.get('binding_schema')!=BINDING_SCHEMA or not isinstance(report.get('cache_binding'),dict):
        raise ValueError('Legacy SSD qualification binding; requalify with volume binding v2')
    bound=report['cache_binding']
    if bound.get('binding_schema')!=BINDING_SCHEMA or bound.get('profile')!=PROFILE or bound.get('quota_bytes')!=quota or binding_digest(bound)!=identity:
        raise ValueError('SSD qualification volume binding mismatch; requalify')
    now = time.time()
    created = report.get('created_at', 0)
    checks = report.get('gates', {})
    if report.get('identity') != identity or report.get('profile') != PROFILE or report.get('quota_bytes') != quota:
        raise ValueError('SSD qualification identity/profile mismatch; requalify this configuration')
    if type(created) not in (int, float) or not math.isfinite(created) or not now-7*86400 <= created <= now+60:
        raise ValueError('SSD qualification evidence expired or has a future date')
    if report.get('result') != 'passed' or type(report.get('duration_seconds')) not in (int,float) or not math.isfinite(report['duration_seconds']) or report['duration_seconds'] < 1800:
        raise ValueError('SSD qualification requires a completed 1800-second run')
    if type(report.get('successes')) is not int or report['successes'] < 100 or type(report.get('failures')) is not int or report['failures'] != 0:
        raise ValueError('SSD qualification request counts failed')
    if not isinstance(checks, dict) or not REQUIRED_GATES.issubset(checks) or any(checks[key] is not True for key in REQUIRED_GATES):
        raise ValueError('SSD qualification gates are incomplete')
    eviction = report.get('requires_eviction', False)
    if type(eviction) is not bool or (eviction and any(checks.get(key) is not True for key in ('automatic_eviction', 'eviction_restart_recompute'))):
        raise ValueError('Automatic eviction qualification gates are incomplete')
    growth = report.get('rss_growth_bytes')
    if type(growth) is not int or not 0 <= growth <= 128*1024**2:
        raise ValueError('SSD qualification memory growth failed')
    return report
