import { mkdirSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const root = resolve(import.meta.dirname, '..');
const destination = resolve(root, 'src-tauri/resources/strata.tar.gz');
mkdirSync(resolve(root, 'src-tauri/resources'), { recursive: true });
const revision = process.env.SOLITON_STRATA_SPLIT_CANDIDATE === '1'
  ? 'a34979fc94f3d126b9bb8210f4ad311345688b52'
  : readFileSync(resolve(root, 'src-tauri/strata-revision.txt'), 'utf8').trim();
if (!/^[0-9a-f]{40}$/.test(revision)) throw new Error('Invalid pinned Strata revision');
const result = spawnSync('git', ['-C', resolve(root, 'Strata'), 'archive', '--format=tar.gz', `--output=${destination}`, revision], { stdio: 'inherit' });
if (result.status !== 0) throw new Error('Cannot archive pinned Strata revision; initialize the Strata submodule first');
