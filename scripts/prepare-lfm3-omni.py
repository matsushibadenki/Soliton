"""Verify the pinned official Omni runtime/assets; never authorize inference.

Run after fetching the reviewed assets into .soliton-runtime. No remote model
code, implicit dependency upgrade, or global installation is performed.
"""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
RUNTIME = ROOT / '.soliton-runtime/llama-b11960'
MODEL = ROOT / '.soliton-runtime/lfm3-omni-2b'
ARTIFACTS = [
    (RUNTIME / 'release.tar.gz', '58e5a5e1d7c2bf66cedd9c29e0323f3e570660a70a1e2156d32eed08f4cc1e3c'),
    (MODEL / 'LFM3-Omni-2B-Q4_K_M.gguf', '9f8d30e9d4fd7a9349d586e077988229b3ca0a0e4ad730897563d1eccc9893e2'),
    (MODEL / 'mmproj-LFM3-Omni-2B-Q8_0.gguf', '720ff4fc09df0e026c4ec641d0c698bf27b5bcd50561944c88eda5e3dffe1839'),
]

def inspect():
    checks = []
    for path, expected in ARTIFACTS:
        if not path.is_file() or path.is_symlink():
            checks.append(dict(name=path.name, passed=False, reason='missing_or_symlink'))
            continue
        with path.open('rb') as f:
            digest = hashlib.file_digest(f, 'sha256').hexdigest()
        checks.append(dict(name=path.name, passed=digest == expected, sha256=digest))
    binary = RUNTIME / 'llama-b11960/llama'
    version = None
    if binary.is_file():
        try:
            version = subprocess.check_output([str(binary), '--version'], stderr=subprocess.STDOUT, timeout=15, text=True).strip()
        except (OSError, subprocess.SubprocessError):
            pass
    return dict(model='LiquidAI/LFM3-Omni-2B', revision='2918dd47cfceb4f260cd1734cd1aac1b22bbe4b6', artifacts=checks, runtimeVersion=version, ffmpeg=shutil.which('ffmpeg'), ffprobe=shutil.which('ffprobe'), qualified=False, runnable=False, scope='asset verification only; not Soliton startup qualification')

if __name__ == '__main__':
    report = inspect()
    print(json.dumps(report, indent=2))
    raise SystemExit(0 if all(c['passed'] for c in report['artifacts']) and report['runtimeVersion'] and report['ffmpeg'] and report['ffprobe'] else 1)
