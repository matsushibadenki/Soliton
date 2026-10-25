"""Backend-Python contract test with logical-byte fixtures, no model or tensors.
Run: BACKEND_PYTHON -I scripts/verify-kv-lru.py
"""
import hashlib
import importlib.metadata
import json
from pathlib import Path
import runpy
if not __debug__:
    raise RuntimeError('validation assertions must be enabled')
from mlx_lm.models import cache as module
root = Path(__file__).resolve().parents[1]
wrapper = runpy.run_path(str(root / 'vLLM-apple/vllm_apple/mlx_kv_budget.py'))
assert importlib.metadata.version('mlx-lm') == '0.32.0'
assert hashlib.sha256(Path(module.__file__).read_bytes()).hexdigest() == wrapper['CACHE_SHA256']
class KV:
    nbytes = 400 * 1024**2
    def is_trimmable(self): return False
before = module.LRUPromptCache(10)
for index in range(5): before.insert_cache('fixture', [index], [KV()])
assert before.nbytes == 2000 * 1024**2
wrapper['install_budget'](module.LRUPromptCache, 1024**3)
after = module.LRUPromptCache(10)
for index in range(5):
    after.insert_cache('fixture', [index], [KV()])
    assert after.nbytes <= 1024**3
assert len(after) == 2 and after.nbytes == 800 * 1024**2
assert after.fetch_nearest_cache('fixture', [0])[0] is None
print(json.dumps({'passed': True, 'realLruInsertions': 5, 'unboundedLogicalBytes': before.nbytes,
    'boundedLogicalBytes': after.nbytes, 'maxBytes': after.max_bytes,
    'logicalByteFixturesOnly': True, 'weightsLoaded': False}))
