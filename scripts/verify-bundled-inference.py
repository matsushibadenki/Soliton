"""Explicit real-model smoke test using only extracted distribution resources."""
import json
import os
from pathlib import Path
import subprocess
import tarfile

root = Path(__file__).resolve().parent.parent
work = root / '.soliton-runtime/bundled-inference-test'
work.mkdir(parents=True, exist_ok=True)
engine_root = work / 'distribution'
engine_root.mkdir(exist_ok=True)
with tarfile.open(root / 'src-tauri/resources/strata.tar.gz') as archive:
    archive.extractall(engine_root, filter='data')
models = root / 'Strata-data/models/Q2_0'
prepared = root / 'Strata-data/local-models/Q2_0-3bd3917524b12ead'
names = [f'Qwen3.8-Flash-Next-GSQ-RCO-Q2_0-{i:05d}-of-00002.gguf' for i in (1, 2)]
for name in names:
    link = work / name
    if not link.exists(): link.symlink_to(models / name)
experts = work / (names[0] + '.experts')
if not experts.exists(): experts.symlink_to(prepared / 'sources' / experts.name)
command = [str(engine_root / 'build-metal/strata'), '--pack', str(prepared / 'prepared/packs/q2_0'), '--native', str(work / names[0]), '--ple-gguf', str(work / names[1]), '--expert-profile', str(engine_root / 'data/expert-profile.bin'), '--expert-cache', '64', '--prefill', '64', '--spec', '2', '--mmap-experts', '--max-context', '512', '--tokens', '1,2,3', '--max-new', '1']
env = {**os.environ, 'PATH': '/usr/bin:/bin', 'STRATA_METAL_KEEP_FREE_GB': '7', 'STRATA_METAL_PROMPT_CHUNK': '64', 'STRATA_METAL_STREAM': '1'}
resources = engine_root / 'build-metal/metal'
for variable, relative in [('STRATA_METAL_KERNELS', 'kernels'), ('STRATA_METAL_ENGINE_KERNELS', 'engine-msl'), ('STRATA_GGML_COMMON', 'ggml-common.h')]:
    path = resources / relative
    assert path.exists(), path
    env[variable] = str(path)
with (work / 'engine.log').open('w') as log:
    result = subprocess.run(command, cwd=engine_root, env=env, stdout=log, stderr=log, timeout=300)
text = (work / 'engine.log').read_text()
if result.returncode == 0 and not any(line.startswith('output  :') for line in text.splitlines()):
    raise RuntimeError('Engine exited without generated token output')
report = {'exitCode': result.returncode, 'model': 'Q2_0', 'requestedTokens': 1, 'compilerRequired': False, 'log': str(work / 'engine.log')}
(work / 'result.json').write_text(json.dumps(report, indent=2))
print(json.dumps(report))
raise SystemExit(result.returncode)
