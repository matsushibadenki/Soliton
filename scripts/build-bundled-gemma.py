"""Build the opt-in Gemma Metal backend from Strata's pinned llama.cpp dependency."""
from pathlib import Path
import hashlib
import json
import subprocess
import tarfile
import urllib.request

COMMIT = '3cf03257f219afbe7334045ff7c6a06ac68c627d'
SHA256 = 'c076d7534afa0e5d0ec2a0d425b11e791c16f3de0d727221aea071cef156a280'


def build(root):
    cache = root / '.soliton-runtime'
    archive = cache / 'gemma3-llama-source.tar.gz'
    if not archive.exists():
        temp = archive.with_suffix('.partial')
        urllib.request.urlretrieve(f'https://codeload.github.com/ggml-org/llama.cpp/tar.gz/{COMMIT}', temp)
        temp.replace(archive)
    if hashlib.sha256(archive.read_bytes()).hexdigest() != SHA256:
        raise RuntimeError('Gemma backend source checksum mismatch')
    source = cache / ('llama.cpp-' + COMMIT)
    if not source.exists():
        with tarfile.open(archive) as handle:
            handle.extractall(cache, filter='data')
    dest = cache / 'gemma3-pinned-build'
    subprocess.run(['cmake','-S',str(source),'-B',str(dest),'-DCMAKE_BUILD_TYPE=Release',
        '-DLLAMA_BUILD_COMMIT='+COMMIT,'-DLLAMA_BUILD_NUMBER=0','-DCMAKE_OSX_DEPLOYMENT_TARGET=15.0','-DBUILD_SHARED_LIBS=OFF','-DGGML_NATIVE=OFF',
        '-DGGML_METAL=ON','-DGGML_METAL_EMBED_LIBRARY=ON','-DGGML_BLAS=OFF',
        '-DGGML_OPENMP=OFF','-DLLAMA_OPENSSL=OFF','-DLLAMA_CURL=OFF','-DLLAMA_BUILD_TESTS=OFF',
        '-DLLAMA_BUILD_EXAMPLES=OFF','-DLLAMA_BUILD_SERVER=ON'], check=True)
    subprocess.run(['cmake','--build',str(dest),'--target','llama-server','-j','4'],check=True)
    engine=dest/'bin/llama-server'
    deps=subprocess.check_output(['otool','-L',str(engine)],text=True)
    libraries = [line.strip().split(' (')[0] for line in deps.splitlines()[1:] if line.strip()]
    if any(not name.startswith(('/usr/lib/', '/System/Library/')) for name in libraries):
        raise RuntimeError('Gemma backend has non-system dynamic dependencies: '+deps)
    manifest=dest/'gemma3.json'
    manifest.write_text(json.dumps({'llama_commit':COMMIT,'source_sha256':SHA256,
        'sha256':hashlib.sha256(engine.read_bytes()).hexdigest(),'backend':'llama.cpp Metal',
        'model_layout':'gemma3_text 1B','minimum_macos':'15.0'},indent=2))
    return source,engine,manifest

if __name__=='__main__':
    build(Path(__file__).resolve().parents[1])
