"""Build the pinned engine on the distribution Mac, never on the user's Mac."""
import argparse
import hashlib
import importlib.util
import json
import os
import pathlib
import subprocess
import sys
import tarfile

root = pathlib.Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--verify-split-candidate', action='store_true',
                    help='Build Strata 0.2.20 without replacing packaged resources')
args = parser.parse_args()
split = args.verify_split_candidate or os.environ.get("SOLITON_STRATA_SPLIT_CANDIDATE") == "1"
revision = (
    'a34979fc94f3d126b9bb8210f4ad311345688b52'
    if split
    else (root / 'src-tauri/strata-revision.txt').read_text().strip()
)
requirements_path = 'terminal/requirements-build.txt' if split else 'requirements-build.txt'
setup_path = 'engine/metal/setup_mac.py' if split else 'metal/setup_mac.py'
if len(revision) != 40 or any(c not in '0123456789abcdef' for c in revision):
    raise RuntimeError('Invalid pinned Strata revision')

# Build tools belong to a disposable build environment, never to the relocatable
# runtime archive. In particular, do not disable a managed Python's PEP 668 guard.
build_python_root = root / '.soliton-runtime' / ('metal-build-python-' + '.'.join(map(str, sys.version_info[:3])))
build_python = build_python_root / 'bin/python3'
if pathlib.Path(sys.prefix).resolve() != build_python_root.resolve():
    if not build_python.is_file():
        subprocess.run([sys.executable, '-m', 'venv', str(build_python_root)], check=True)
    subprocess.run([str(build_python), str(pathlib.Path(__file__).resolve()), *sys.argv[1:]], check=True)
    raise SystemExit(0)

from importlib.metadata import PackageNotFoundError, version
requirements = subprocess.check_output(['git', '-C', str(root / 'Strata'), 'show',
                                       f'{revision}:{requirements_path}'], text=True)
pins = [line.strip() for line in requirements.splitlines() if line.strip() and not line.lstrip().startswith('#')]
def build_tools_current():
    try:
        return all(version(name) == expected for name, expected in (pin.split('==') for pin in pins))
    except PackageNotFoundError:
        return False
if not build_tools_current():
    pinned_requirements = build_python_root / 'requirements-build.txt'
    pinned_requirements.write_text(requirements)
    subprocess.run([sys.executable, '-m', 'pip', 'install', '--disable-pip-version-check',
                    '-r', str(pinned_requirements)], check=True)

stage = root / '.soliton-runtime' / 'metal-distribution'
source = stage / revision
if not source.exists():
    source.mkdir(parents=True)
    archive = stage / f'{revision}.tar'
    subprocess.run(['git', '-C', str(root / 'Strata'), 'archive', '--format=tar', f'--output={archive}', revision], check=True)
    with tarfile.open(archive) as handle:
        handle.extractall(source, filter='data')
    archive.unlink()
# Always restore the pinned build script before loading it. A previous distribution
# copy has runtime-only overrides that must never bypass a distribution rebuild.
setup = source / setup_path
original_setup = subprocess.check_output(['git', '-C', str(root / 'Strata'), 'show',
                                         f'{revision}:{setup_path}'], text=True)
flag = '"-DCMAKE_BUILD_TYPE=Release",'
if flag not in original_setup:
    raise RuntimeError('Pinned Metal setup CMake contract changed')
setup.write_text(original_setup.replace(flag, flag + ' "-DCMAKE_OSX_DEPLOYMENT_TARGET=15.0",', 1))
sys.path.insert(0, str(source))
spec = importlib.util.spec_from_file_location('bundled_metal_setup', setup)
metal = importlib.util.module_from_spec(spec)
spec.loader.exec_module(metal)
llama = None  # Build against the immutable, vendored Strata ggml source.
# Reconfigure even when upstream's source-only stamp matches: it does not bind
# the deployment target. Ninja still reuses objects whose flags have not changed.
metal.engine_current = lambda: False
engine = metal.build_engine(llama, jobs=2)
subprocess.run(['otool', '-L', str(engine)], check=True)
if args.verify_split_candidate:
    artifact = {
        'revision': revision,
        'engine': str(engine.relative_to(source)),
        'sha256': hashlib.sha256(engine.read_bytes()).hexdigest(),
        'deploymentTarget': '15.0',
    }
    (source / 'native-build-verification.json').write_text(json.dumps(artifact, indent=2) + '\n')
    print(json.dumps(artifact))
    raise SystemExit(0)
(stage / 'artifact.json').write_text(json.dumps({'revision': revision, 'engine': str(engine), 'llama': str(llama)}, indent=2))
print(f'Built engine for {revision}: {engine}')

# Only the distribution copy is adapted. The upstream checkout is untouched.
marker = source / '.bundled-metal.json'
marker.write_text(json.dumps({'revision': revision, 'sha256': hashlib.sha256(engine.read_bytes()).hexdigest()}))
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
def _soliton_dependencies(*args, **kwargs):
    import numpy, jinja2, regex, PIL, requests, yaml, psutil
S.pip_install = _soliton_dependencies
'''
if split:
    adapter = adapter.replace("(ROOT / '.bundled-metal.json')", "(ROOT.parent / '.bundled-metal.json')")
if '# Soliton distribution:' not in text:
    setup.write_text(text.replace('if __name__ == "__main__":', adapter + '\nif __name__ == "__main__":'))
gemma_spec = importlib.util.spec_from_file_location('bundled_gemma', root / 'scripts/build-bundled-gemma.py')
gemma = importlib.util.module_from_spec(gemma_spec)
gemma_spec.loader.exec_module(gemma)
gemma_source, gemma_engine, gemma_manifest = gemma.build(root)

# Keep the distribution small and exclude build caches and developer paths.
archive = root / 'src-tauri/resources/strata.tar.gz'
tracked = subprocess.check_output(['git', '-C', str(root / 'Strata'), 'ls-tree', '-r', '--name-only', revision], text=True).splitlines()
with tarfile.open(archive, 'w:gz') as handle:
    for name in tracked:
        path = source / name
        if path.is_file():
            handle.add(path, arcname=name, recursive=False)
    engine_source = source / 'engine' if split else source
    metal_build = 'engine/build-metal' if split else 'build-metal'
    quant_header = 'include/strata/quant/strata-quant-common.h' if split else 'third_party/strata-ggml/strata-quant-common.h'
    data = source / ('terminal/data' if split else 'data')
    handle.add(engine, arcname=f'{metal_build}/strata')
    handle.add(engine_source / 'metal/kernels', arcname=f'{metal_build}/metal/kernels')
    handle.add(engine_source / 'metal/src/engine/msl', arcname=f'{metal_build}/metal/engine-msl')
    handle.add(engine_source / quant_header, arcname=f'{metal_build}/metal/ggml-common.h')
    if data.is_dir():
        handle.add(data, arcname=f'{metal_build}/metal/data')
    handle.add(marker, arcname='.bundled-metal.json')
    handle.add(gemma_engine, arcname='build-metal/strata-gemma-server')
    handle.add(gemma_manifest, arcname='.bundled-gemma3.json')
    handle.add(gemma_source / 'gguf-py', arcname='third_party/gemma3-gguf', filter=lambda i: None if '__pycache__' in i.name else i)
    handle.add(gemma_source / 'LICENSE', arcname='third_party/gemma3-LICENSE')
print(f'Distribution archive: {archive}')
