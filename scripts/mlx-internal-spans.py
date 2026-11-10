"""Bounded scalar wall spans for an isolated, source-reviewed MLX candidate.

No prompts, tensors, token IDs or per-request samples are retained. Durations
include host scheduling and any synchronization already done by the original
method. No extra MLX evaluation, stream synchronization or cache mutation.
"""
from __future__ import annotations

from contextlib import contextmanager
from functools import wraps
import hashlib
import importlib
import importlib.metadata
import json
from pathlib import Path
import runpy
import threading
import time

NAMES = ('qualification.verify', 'qualification.validate', 'qualification.model_identity',
         'qualification.backend_identity', 'cache.fetch', 'cache.insert', 'cache.trim',
         'batch.prefill', 'batch.decode', 'sequential.prefill_first', 'sequential.decode')
SCHEMA = 'soliton-internal-wall-spans-v1'


class SpanLedger:
    def __init__(self, clock=time.perf_counter_ns):
        self._clock = clock
        self._lock = threading.Lock()
        self._rows = {name: dict(calls=0, failures=0, total_ms=0.0, last_ms=None,
                                max_ms=None) for name in NAMES}

    @contextmanager
    def measure(self, name):
        if name not in self._rows:
            raise ValueError('unknown internal span')
        start = self._clock()
        failed = False
        try:
            yield
        except BaseException:
            failed = True
            raise
        finally:
            elapsed = (self._clock() - start) / 1e6
            with self._lock:
                row = self._rows[name]
                row['calls'] += 1
                row['failures'] += int(failed)
                row['total_ms'] += elapsed
                row['last_ms'] = elapsed
                row['max_ms'] = max(row['max_ms'] or 0.0, elapsed)

    def wrap(self, name, fn=None):
        if fn is None:
            return lambda target: self.wrap(name, target)
        @wraps(fn)
        def call(*args, **kwargs):
            with self.measure(name):
                return fn(*args, **kwargs)
        return call

    def snapshot(self):
        with self._lock:
            return dict(schema=SCHEMA, basis='host_wall_no_added_sync',
                        consistency='atomic_ledger_not_request_atomic',
                        spans={name: dict(row) for name, row in self._rows.items()})


STARTUP = SpanLedger()
GENERATE_SHA256 = '9d46803799f99d6fc7c35fff2d4357c79b1a25ae98cc63efeaaebeb50a7b5f33'


def install(ledger, server, cache, generate):
    """Caller must verify all source bytes before installing any hook."""
    for name, method in [('cache.fetch', 'fetch_nearest_cache'),
                         ('cache.insert', 'insert_cache'), ('cache.trim', 'trim_to')]:
        setattr(cache.LRUPromptCache, method,
                ledger.wrap(name, getattr(cache.LRUPromptCache, method)))
    generate.PromptProcessingBatch.prompt = ledger.wrap(
        'batch.prefill', generate.PromptProcessingBatch.prompt)
    generate.GenerationBatch.next = ledger.wrap('batch.decode', generate.GenerationBatch.next)
    original = server.stream_generate

    def stream(*args, **kwargs):
        # Time iterator advancement only; do not include time spent suspended in
        # the server consumer. Closing a generator is not a completed generation.
        iterator = original(*args, **kwargs)
        phase = 'sequential.prefill_first'
        try:
            while True:
                # StopIteration is normal termination, not a failed span.
                # Counts include the final normal iterator exhaustion; they
                # are iterator advances, never generated-token counts.
                sentinel = object()
                with ledger.measure(phase):
                    item = next(iterator, sentinel)
                if item is sentinel:
                    return
                yield item
                phase = 'sequential.decode'
        finally:
            iterator.close()
    server.stream_generate = stream


def main():
    import os
    # Canonical budget wrapper stays self-contained and retains its complete
    # release/server/cache guards; this stricter profile additionally binds generate.
    budget = runpy.run_path(str(Path(__file__).with_name('mlx_kv_budget.py')))
    if importlib.metadata.version('mlx-lm') != '0.32.2':
        raise ValueError('internal spans require reviewed MLX-LM 0.32.2')
    from mlx_lm import server
    from mlx_lm.models import cache
    generate = importlib.import_module('mlx_lm.generate')
    if (not budget['reviewed_sources']('0.32.2',
            hashlib.sha256(Path(server.__file__).read_bytes()).hexdigest(),
            hashlib.sha256(Path(cache.__file__).read_bytes()).hexdigest())
            or hashlib.sha256(Path(generate.__file__).read_bytes()).hexdigest() != GENERATE_SHA256):
        raise ValueError('internal spans source mismatch')
    # Keep invalid policy rejection before instrumentation/model startup as well.
    raw = os.environ.get('VLLM_APPLE_KV_CACHE_MIB', '')
    if not raw.isascii() or not raw.isdecimal() or not 1 <= len(raw) <= 4 or not 64 <= int(raw) <= 4096:
        raise ValueError('explicit reviewed retained budget required')
    ledger = SpanLedger()
    install(ledger, server, cache, generate)
    budget['main'](diagnostics=ledger.snapshot)


if __name__ == '__main__':
    main()
