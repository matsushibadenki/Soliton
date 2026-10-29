"""Build the pinned engine on the distribution Mac, never on the user's Mac."""
import importlib.util
import json
import pathlib
import subprocess
import sys
import tarfile

root = pathlib.Path(__file__).resolve().parent.parent
stage = root / '.soliton-runtime' / 'metal-distribution'
revision = subprocess.check_output(['git', 'rev-parse', 'HEAD:Strata'], cwd=root, text=True).strip()
source = stage / revision
if not source.exists():
    source.mkdir(parents=True)
    archive = stage / 'source.tar'
    subprocess.run(['git', '-C', str(root / 'Strata'), 'archive', '--format=tar', f'--output={archive}', revision], check=True)
    with tarfile.open(archive) as handle:
        handle.extractall(source, filter='data')
    archive.unlink()
sys.path.insert(0, str(source))
spec = importlib.util.spec_from_file_location('bundled_metal_setup', source / 'metal/setup_mac.py')
metal = importlib.util.module_from_spec(spec)
spec.loader.exec_module(metal)
llama = metal.S.get_llama_cpp()
engine = metal.build_engine(llama, jobs=2)
subprocess.run(['otool', '-L', str(engine)], check=True)
(stage / 'artifact.json').write_text(json.dumps({'revision': revision, 'engine': str(engine), 'llama': str(llama)}, indent=2))
print(f'Built engine for {revision}: {engine}')

# Only the distribution copy is adapted. The upstream checkout is untouched.
import hashlib
marker = source / '.bundled-metal.json'
marker.write_text(json.dumps({'revision': revision, 'sha256': hashlib.sha256(engine.read_bytes()).hexdigest()}))
setup = source / 'metal/setup_mac.py'
text = setup.read_text()
adapter = '''
# Soliton distribution: prebuilt engine, runtime libraries and converter are bundled.
def _soliton_engine(_llama, jobs=None):
    meta = json.loads((ROOT / '.bundled-metal.json').read_text())
    if hashlib.sha256(EXE.read_bytes()).hexdigest() != meta['sha256']:
        raise RuntimeError('Bundled engine checksum mismatch')
    return EXE
build_engine = _soliton_engine
_original_mac_problems = mac_problems
mac_problems = lambda info: _original_mac_problems(info, need_compiler=False)
S.get_llama_cpp = lambda: ROOT / 'third_party/llama.cpp'
def _soliton_dependencies(*args, **kwargs):
    import numpy, jinja2, regex, PIL, requests, yaml, psutil
S.pip_install = _soliton_dependencies
'''
if '# Soliton distribution:' not in text:
    setup.write_text(text.replace('if __name__ == "__main__":', adapter + '\nif __name__ == "__main__":'))
# Keep the distribution small and exclude build caches and developer paths.
archive = root / 'src-tauri/resources/strata.tar.gz'
tracked = subprocess.check_output(['git', '-C', str(root / 'Strata'), 'ls-tree', '-r', '--name-only', revision], text=True).splitlines()
with tarfile.open(archive, 'w:gz') as handle:
    for name in tracked:
        path = source / name
        if path.is_file():
            handle.add(path, arcname=name, recursive=False)
    handle.add(engine, arcname='build-metal/strata')
    handle.add(source / 'metal/kernels', arcname='build-metal/metal/kernels')
    handle.add(source / 'metal/src/engine/msl', arcname='build-metal/metal/engine-msl')
    handle.add(llama / 'ggml/src/ggml-common.h', arcname='build-metal/metal/ggml-common.h')
    if (source / 'data').is_dir():
        handle.add(source / 'data', arcname='build-metal/metal/data')
    handle.add(marker, arcname='.bundled-metal.json')
    handle.add(llama / 'gguf-py', arcname='third_party/llama.cpp/gguf-py', filter=lambda i: None if '__pycache__' in i.name else i)
    handle.add(llama / 'LICENSE', arcname='third_party/llama.cpp/LICENSE')
print(f'Distribution archive: {archive}')
