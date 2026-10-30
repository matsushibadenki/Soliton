"""macOS build-host preflight. Inspect the actual app and relocated bundled runtimes."""
import argparse
import plistlib
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_APP = ROOT / 'target/aarch64-apple-darwin/release/bundle/macos/Soliton.app'
MAGIC = {b'\xcf\xfa\xed\xfe', b'\xce\xfa\xed\xfe', b'\xfe\xed\xfa\xcf', b'\xfe\xed\xfa\xce', b'\xca\xfe\xba\xbe', b'\xbe\xba\xfe\xca', b'\xca\xfe\xba\xbf'}


def version(s):
    return tuple(int(p) for p in s.split('.')) + (0,) * (3-len(s.split('.')))


def load_versions(text):
    # Compiler/linker tool versions are not deployment targets.
    command = None
    found = []
    for line in text.splitlines():
        fields = line.split()
        if fields[:1] == ['cmd']:
            command = fields[1] if len(fields) > 1 else None
        elif len(fields) == 2 and ((command == 'LC_BUILD_VERSION' and fields[0] == 'minos')
                or (command == 'LC_VERSION_MIN_MACOSX' and fields[0] == 'version')):
            found.append(fields[1])
    return found


def dependency_errors(libraries, binary, boundary, rpaths=()):
    errors = []
    boundary = boundary.resolve()
    for name in libraries:
        if name.startswith(('/usr/lib/', '/System/Library/')):
            continue
        if name.startswith('@loader_path/'):
            candidates = [binary.parent / name.removeprefix('@loader_path/')]
        elif name.startswith('@rpath/'):
            candidates = []
            for rpath in rpaths:
                if rpath.startswith('@loader_path/'):
                    candidates.append(binary.parent / rpath.removeprefix('@loader_path/') / name.removeprefix('@rpath/'))
        else:
            errors.append('non-relocatable dependency: ' + name)
            continue
        if not any(p.resolve().is_relative_to(boundary) and p.is_file() for p in candidates):
            errors.append('unresolved bundled dependency: ' + name)
    return errors


def inspect_macho(binary, boundary, minimum='15.0'):
    def run(*args):
        return subprocess.check_output(args, text=True, stderr=subprocess.STDOUT)
    arch = run('/usr/bin/lipo', '-archs', str(binary)).strip().split()
    build = run('/usr/bin/vtool', '-show-build', str(binary))
    targets = load_versions(build)
    errors = []
    if 'arm64' not in arch:
        errors.append('arm64 architecture is missing')
    if not targets or any(version(v) > version(minimum) for v in targets):
        errors.append('minimum macOS exceeds app minimum or is unknown: ' + ','.join(targets))
    deps = run('/usr/bin/otool', '-L', str(binary))
    libraries = [line.strip().split(' (')[0] for line in deps.splitlines() if line.startswith('\t')]
    # The first dylib line is its own install name, not a dependency.
    install_ids = run('/usr/bin/otool', '-D', str(binary)).splitlines()[1:]
    libraries = [x for x in libraries if x not in install_ids]
    commands = run('/usr/bin/otool', '-l', str(binary))
    rpaths = re.findall(r'cmd LC_RPATH\n[^\n]+\n\s*path (.+?) \(offset', commands)
    errors += dependency_errors(libraries, binary, boundary, rpaths)
    return {'architectures':arch, 'minimumMacOS':targets, 'errors':errors}


def sha(path):
    with path.open('rb') as handle:
        return hashlib.file_digest(handle, 'sha256').hexdigest()


def check(app, smoke=False):
    report = {'schemaVersion':1, 'scope':'local distribution preflight; not another-Mac verification',
              'otherMacGui':'pending', 'signingAndNotarization':'Developer ID and notarization deferred; ad hoc integrity signature', 'errors':[], 'nativeFiles':[]}
    config = json.loads((ROOT/'src-tauri/tauri.conf.json').read_text())
    minimum = config['bundle']['macOS']['minimumSystemVersion']
    report['minimumMacOS'] = minimum
    try:
        report['sourceCommit'] = subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()
        report['workingTreeDirty'] = bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT,text=True).strip())
    except subprocess.SubprocessError:
        report['sourceCommit'] = None
    with tempfile.TemporaryDirectory(prefix='soliton-preflight-') as temp:
        work = Path(temp)
        main = app/'Contents/MacOS/soliton'
        if not main.is_file():
            raise ValueError('Soliton.app executable not found')
        report['appExecutableSha256'] = sha(main)
        integrity = subprocess.run(['/usr/bin/codesign','--verify','--deep','--strict',str(app)],capture_output=True,text=True)
        report['bundleIntegrity'] = 'verified' if integrity.returncode == 0 else 'failed'
        if integrity.returncode:
            report['errors'].append('app bundle integrity verification failed')
        with (app/'Contents/Info.plist').open('rb') as handle:
            plist = plistlib.load(handle)
        if plist.get('LSMinimumSystemVersion') != minimum:
            report['errors'].append('Info.plist minimum macOS does not match build configuration')
        roots = [('app',app)]
        report['archives'] = {}
        for kind in ['strata','python']:
            archive = app/f'Contents/Resources/{kind}.tar.gz'
            destination = work/kind
            destination.mkdir()
            with tarfile.open(archive) as handle:
                handle.extractall(destination, filter='data')
            report['archives'][kind] = {'sha256':sha(archive), 'bytes':archive.stat().st_size}
            roots.append((kind,destination))
        strata = work/'strata'
        for binary in [main,work/'python/bin/python3',strata/'build-metal/strata',strata/'build-metal/strata-gemma-server']:
            if not os.access(binary,os.X_OK):
                report['errors'].append('required runtime is not executable: '+binary.name)
        for marker, engine in [('.bundled-metal.json','build-metal/strata'),
                               ('.bundled-gemma3.json','build-metal/strata-gemma-server')]:
            meta = json.loads((strata/marker).read_text())
            if sha(strata/engine) != meta['sha256']:
                report['errors'].append('engine checksum mismatch: '+engine)
        for required in ['serve/server.py','metal/setup_mac.py','build-metal/metal/kernels',
                         'build-metal/metal/engine-msl','build-metal/metal/ggml-common.h',
                         'third_party/gemma3-gguf/gguf/__init__.py','third_party/gemma3-LICENSE',
                         'third_party/llama.cpp/LICENSE']:
            if not (strata/required).exists():
                report['errors'].append('missing bundled resource: '+required)
        for label, root in roots:
            for path in sorted(root.rglob('*')):
                if path.is_symlink() or not path.is_file():
                    continue
                with path.open('rb') as handle:
                    magic = handle.read(4)
                if magic not in MAGIC:
                    continue
                item = {'path':label+'/'+str(path.relative_to(root)), **inspect_macho(path,root,minimum)}
                report['nativeFiles'].append(item)
                report['errors'] += [item['path']+': '+e for e in item['errors']]
        if smoke:
            env = dict(os.environ, PATH='/usr/bin:/bin:/usr/sbin:/sbin', PYTHONNOUSERSITE='1')
            for key in ['PYTHONHOME','PYTHONPATH','VIRTUAL_ENV','DYLD_LIBRARY_PATH','DYLD_FALLBACK_LIBRARY_PATH']:
                env.pop(key,None)
            code = 'import ssl,numpy,jinja2,regex,PIL,requests,yaml,psutil,sentencepiece; import sys,json; print(json.dumps({"python":sys.version.split()[0],"sentencepiece":sentencepiece.__version__}))'
            result = subprocess.run([str(work/'python/bin/python3'),'-I','-c',code],env=env,
                                    capture_output=True,text=True,timeout=45)
            if result.returncode:
                report['errors'].append('relocated Python imports failed: '+result.stderr[-1000:])
            else:
                report['relocatedPython'] = json.loads(result.stdout)
                report['relocatedPython']['restrictedPath'] = True
        report['runtimeSmoke'] = 'python imports only' if smoke else 'not run'
    report['passed'] = not report['errors']
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app',type=Path,default=DEFAULT_APP)
    parser.add_argument('--output',type=Path,default=ROOT/'.soliton-runtime/distribution-preflight.json')
    parser.add_argument('--smoke',action='store_true',help='run extracted Python with restricted PATH')
    args = parser.parse_args()
    try:
        report = check(args.app.resolve(),args.smoke)
    except Exception as exc:
        report = {'passed':False,'otherMacGui':'pending','errors':[str(exc)]}
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    print(json.dumps({'passed':report['passed'],'nativeFiles':len(report.get('nativeFiles',[])),
                      'errors':report['errors'],'output':str(args.output)},ensure_ascii=False))
    return 0 if report['passed'] else 1

if __name__=='__main__':
    raise SystemExit(main())
