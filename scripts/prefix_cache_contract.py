"""Identity-bound qualification contract for Soliton's MLX SSD engine."""
import hashlib
import json
import math
import os
from pathlib import Path
import stat
import time

PROFILE = dict(schema=1, engine='mlxPrefix', context=512, maximum_output_tokens=128,
               concurrency=1, temperature=0, seed=0, memory_cache_bytes=64*1024**2,
               allocator_cache_bytes=256*1024**2)
REQUIRED_GATES = {'authentication', 'request_rejection', 'cold', 'restart_hit', 'corruption_fallback',
                  'disconnect_recovery', 'quota_fallback', 'context_boundary'}


def identity_digest(identity, root, quota):
    bound = dict(identity=identity, profile=PROFILE, cache_root=str(root.resolve(strict=True)),
                 cache_device=root.stat().st_dev, quota_bytes=quota)
    return hashlib.sha256(json.dumps(bound, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


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
