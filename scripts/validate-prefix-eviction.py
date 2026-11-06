"""Isolated real-model pressure comparison; no managed settings/evidence are changed.
The candidate runs only in qualification-mode, which does not certify production adoption.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import signal
import subprocess
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
QUOTA = 128 * 1024**2


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--python', required=True)
    parser.add_argument('--model', required=True, type=Path)
    parser.add_argument('--port', type=int, default=18086)
    parser.add_argument('--output', type=Path, default=Path('/tmp/soliton-prefix-eviction.json'))
    args = parser.parse_args()
    model = args.model.resolve(strict=True)
    if not args.model.is_absolute() or not Path(args.python).is_absolute():
        parser.error('Use absolute local model/Python paths')
    executables = {name: ROOT / 'target/debug/examples' / binary for name, binary in
                   [('refuse', 'prefix-store'), ('evict', 'prefix-store-evict')]}
    for executable in executables.values():
        if not executable.is_file(): parser.error('Build the separate candidate executable first')
    assets = sorted(p for p in model.iterdir() if p.is_file() and p.suffix in {'.json', '.safetensors', '.model', '.txt'})
    def fingerprint():
        digest = hashlib.sha256()
        for path in assets:
            digest.update(path.name.encode())
            with path.open('rb') as source:
                for chunk in iter(lambda: source.read(4*1024**2), b''): digest.update(chunk)
        return digest.hexdigest()
    source_hash = fingerprint()
    prompts = [f'Document {i}. ' + 'The fictional code word is soliton. '*20 +
               'What is the code word? Answer briefly.' for i in range(10)]
    report = dict(scope='isolated qualification-mode pressure; not production qualification', quotaBytes=QUOTA,
                  sourceSha256=source_hash, trials={}, executableSha256={})
    for name, executable in executables.items():
        report['executableSha256'][name] = hashlib.sha256(executable.read_bytes()).hexdigest()
    for policy, executable in executables.items():
        with tempfile.TemporaryDirectory(prefix=f'soliton-eviction-{policy}-') as temp:
            folder = Path(temp)
            token = secrets.token_hex(24)
            process = None
            def request(path='/studio/ssd-cache', body=None):
                req = urllib.request.Request(f'http://127.0.0.1:{args.port}'+path,
                    headers={'Content-Type':'application/json', 'Authorization':'Bearer '+token},
                    data=None if body is None else json.dumps(body).encode())
                with urllib.request.urlopen(req, timeout=120) as response: return json.load(response)
            def start():
                nonlocal process
                log = (folder/'worker.log').open('a')
                process = subprocess.Popen([args.python, str(ROOT/'scripts/prefix-cache-http.py'),
                    '--model', str(model), '--cache-root', str(folder/'cache'), '--quota-mib','128',
                    '--port', str(args.port), '--qualification-mode', '--store-executable', str(executable)],
                    env=dict(os.environ, SOLITON_PREFIX_HTTP_TOKEN=token), stdout=log, stderr=log, start_new_session=True)
                log.close()
                for _ in range(300):
                    if process.poll() is not None: raise RuntimeError((folder/'worker.log').read_text()[-3000:])
                    try: request(); return
                    except (OSError, ValueError): time.sleep(.2)
                raise TimeoutError('Worker startup')
            def stop():
                nonlocal process
                if process is None: return
                if process.poll() is None:
                    os.killpg(process.pid, signal.SIGINT)
                    try: process.wait(timeout=15)
                    except subprocess.TimeoutExpired:
                        os.killpg(process.pid, signal.SIGKILL); process.wait(); raise
                process = None
            def infer(prompt):
                response = request('/v1/chat/completions', dict(model='default_model',
                    messages=[dict(role='user',content=prompt)], temperature=0, seed=0, max_tokens=16))
                return dict(text=response['choices'][0]['message']['content'], usage=response['usage'])
            trial = dict(answers=[], pressure=[], evictedNames=[])
            try:
                start()
                previous = set()
                for index, prompt in enumerate(prompts):
                    answer = infer(prompt)
                    assert 'soliton' in answer['text'].lower(), answer
                    store = folder/'cache/store'
                    if index == 0: (store/'protected.txt').write_text('Keep this unknown file')
                    names = {p.name for p in store.glob('*.cache')}
                    removed = sorted(previous-names)
                    trial['evictedNames'].extend(removed)
                    used = sum(p.stat().st_size for p in store.iterdir() if p.name != '.lock')
                    assert used <= QUOTA
                    trial['answers'].append(answer)
                    trial['pressure'].append(dict(index=index,usedBytes=used,entries=len(names),removed=len(removed)))
                    assert (store/'protected.txt').read_text() == 'Keep this unknown file'
                    previous = names
                    print(json.dumps(dict(policy=policy,index=index,entries=len(names),removed=len(removed))), flush=True)
                trial['stats'] = request()
                if policy == 'refuse':
                    assert trial['stats']['errors'] > 0
                    assert not trial['evictedNames']
                else:
                    assert trial['stats']['errors'] == 0
                    assert trial['evictedNames']
                    assert trial['stats']['writes'] == len(prompts)
                    assert [a['text'] for a in trial['answers']] == [a['text'] for a in report['trials']['refuse']['answers']]
                    # Recently retained state restores after process restart; oldest evicted one recomputes.
                    stop(); start()
                    restored = infer(prompts[-1]); assert restored['text'] == trial['answers'][-1]['text']
                    assert request()['hits'] == 1
                    # Clear retained RAM prefixes before requiring a genuine cold miss.
                    stop(); start()
                    revisited = infer(prompts[0]); assert revisited['text'] == trial['answers'][0]['text']
                    assert request()['misses'] == 1 and request()['writes'] == 1
                    repeated = infer(prompts[0]); assert repeated['text'] == revisited['text']
                    assert request()['skippedWrites'] >= 1
                    trial['restartStats'] = request()
                    assert (store/'protected.txt').read_text() == 'Keep this unknown file'
            finally: stop()
            report['trials'][policy] = trial
    assert fingerprint() == source_hash, 'Source model changed'
    report['sourceUnchanged'] = True
    args.output.write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(dict(passed=True,output=str(args.output),refusedWrites=report['trials']['refuse']['stats']['errors'],
        candidateWrites=report['trials']['evict']['stats']['writes'],candidateEvictions=len(report['trials']['evict']['evictedNames']))),flush=True)


if __name__ == '__main__':
    main()
