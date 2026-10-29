import { resolve } from 'node:path';
import { mkdirSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
const root = resolve(import.meta.dirname, '..');
const install = resolve(root, '.soliton-runtime/python-build');
const python = resolve(install, 'cpython-3.12.15-macos-aarch64-none');
function run(cmd, args, options = {}) {
  const result = spawnSync(cmd, args, { cwd: root, stdio: 'inherit', ...options });
  if (result.status !== 0) throw new Error(`${cmd} failed`);
}
if (process.platform !== 'darwin' || process.arch !== 'arm64') throw new Error('Python bundle currently requires Apple Silicon macOS');
mkdirSync(resolve(root, 'src-tauri/resources'), { recursive: true });
run('uv', ['python', 'install', '3.12.15'], { env: { ...process.env, UV_PYTHON_INSTALL_DIR: install } });
run('uv', ['pip', 'install', '--python', resolve(python, 'bin/python3'), '--break-system-packages', '-r', 'Strata/requirements.txt']);
run('/usr/bin/tar', ['-czf', resolve(root, 'src-tauri/resources/python.tar.gz'), '-C', python, '.']);
