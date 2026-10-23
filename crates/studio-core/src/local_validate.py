# Header/tensor-directory validation only: never read weight data or contact a network.
import sys, json, struct
from pathlib import Path
sys.path.insert(0, str(Path(sys.argv[1]) / 'tools'))
from gguf_reader import GGUFFile
class Limited:
    def __init__(self, f): self.f, self.used = f, 0
    def read(self, n):
        if n < 0 or n > 16*1024*1024 or self.used+n > 64*1024*1024:
            raise ValueError('GGUF header exceeds inspection limits')
        self.used += n
        data = self.f.read(n)
        if len(data) != n: raise ValueError('Truncated GGUF header')
        return data
    def tell(self): return self.f.tell()
class Header(GGUFFile):
    def _parse(self, f):
        magic, version, tensors, keys = struct.unpack('<IIQQ', f.read(24))
        if magic != 0x46554747 or version != 3 or not 0 < tensors <= 200000 or keys > 100000:
            raise ValueError('Unsupported or empty GGUF header')
        f.seek(0)
        super()._parse(Limited(f))
paths = [Path(p) for p in json.loads(sys.argv[2])]
files = [Header(p) for p in paths]
first = files[0].metadata
if first.get('general.architecture') != 'qwen4exp':
    raise ValueError('This Metal setup requires the qwen4exp architecture')
total = first.get('split.tensors.count')
if not isinstance(total, int) or total <= 0: raise ValueError('Missing split tensor count')
names = set()
for i, g in enumerate(files):
    if g.metadata.get('split.count') != len(files) or g.metadata.get('split.no') != i or g.metadata.get('split.tensors.count') != total:
        raise ValueError('Inconsistent GGUF shard metadata')
    arch = g.metadata.get('general.architecture')
    if arch is not None and arch != 'qwen4exp': raise ValueError('Mixed model architectures')
    if g.alignment <= 0 or g.alignment > 65536 or g.alignment & (g.alignment-1):
        raise ValueError("Invalid GGUF alignment")
    size = g.path.stat().st_size
    for t in g.tensors:
        expected = t.expected_bytes()
        if expected is None or expected <= 0 or g.data_start+t.offset+expected > size:
            raise ValueError('Unsupported or truncated tensor: ' + t.name)
        if t.name in names: raise ValueError('Duplicate tensor: ' + t.name)
        names.add(t.name)
if len(names) != total: raise ValueError('Incomplete tensor set')
print(json.dumps({'valid': True}))
