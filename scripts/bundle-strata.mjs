import { mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const root = resolve(import.meta.dirname, '..');
const destination = resolve(root, 'src-tauri/resources/strata.tar.gz');
mkdirSync(resolve(root, 'src-tauri/resources'), { recursive: true });
const pinned = spawnSync('git', ['rev-parse', 'HEAD:Strata'], { cwd: root, encoding: 'utf8' });
if (pinned.status !== 0) throw new Error('Cannot find the pinned Strata revision');
const result = spawnSync('git', ['-C', resolve(root, 'Strata'), 'archive', '--format=tar.gz', `--output=${destination}`, pinned.stdout.trim()], { stdio: 'inherit' });
if (result.status !== 0) throw new Error('Cannot archive pinned Strata revision; initialize the Strata submodule first');
