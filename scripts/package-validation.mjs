// Build-host handoff only. The target Mac needs neither Node nor Python installed.
import { resolve } from 'node:path';
import { mkdirSync, copyFileSync, writeFileSync, createReadStream } from 'node:fs';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
const root = resolve(import.meta.dirname, '..');
const app = resolve(process.argv[2] || resolve(root, 'target/aarch64-apple-darwin/release/bundle/macos/Soliton.app'));
const out = resolve(root, '.soliton-runtime/distribution-handoff');
const python = resolve(root, '.soliton-runtime/python-build/cpython-3.12.15-macos-aarch64-none/bin/python3');
function run(command, args, capture = false) {
  const r = spawnSync(command, args, { cwd: root, encoding: 'utf8', stdio: capture ? 'pipe' : 'inherit' });
  if (r.status !== 0) throw new Error(`${command} failed: ${r.stderr || r.error || r.status}`);
  return r.stdout?.trim();
}
async function sha(path) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest('hex');
}
mkdirSync(out, { recursive: true });
run(python, ['scripts/check-distribution.py', '--app', app, '--smoke', '--output', resolve(out, 'preflight.json')]);
const zip = resolve(out, 'Soliton-Gemma3-AppleSilicon.zip');
run('/usr/bin/ditto', ['-c', '-k', '--sequesterRsrc', '--keepParent', app, zip]);
run('/usr/bin/unzip', ['-tq', zip]);
const digest = await sha(zip);
writeFileSync(resolve(out, 'SHA256SUMS.txt'), `${digest}  Soliton-Gemma3-AppleSilicon.zip\n`);
copyFileSync(resolve(root, 'docs/CLEAN_MAC_VALIDATION.md'), resolve(out, 'README.md'));
writeFileSync(resolve(out, 'validation-status.json'), JSON.stringify({
  status: 'local_preflight_passed_other_mac_pending',
  testedOnAnotherMac: false,
  repositoryHeadAtPackaging: run('git', ['rev-parse', 'HEAD'], true),
  workingTreeDirty: Boolean(run('git', ['status', '--porcelain'], true)),
  artifactSha256: digest,
  signingAndNotarization: 'Developer ID and notarization deferred; ad hoc integrity signature',
  releaseReady: false,
  weightsIncluded: false,
  tokensIncluded: false,
}, null, 2) + '\n');
console.log(`Validation handoff: ${out}`);
