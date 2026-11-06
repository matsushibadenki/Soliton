"""Isolated real-model multi-turn/pressure test. Uses existing reviewed worker artifacts.
Capacity trim applies only to this run's generated cache. Never issues qualification evidence.
"""
import argparse
import hashlib
import http.client
import json
import os
import re
from pathlib import Path
import secrets
import signal
import socket
import statistics
import subprocess
import time
import urllib.error
import urllib.request

CASES = [
    ('en', [
        'We are planning a workshop. The room is Cedar. Remember this and reply only OK.',
        'The start time is 14:00. Reply only OK.',
        'Change the room from Cedar to Maple. Reply only OK.',
        'What is the room now? Answer with only its name.',
        'What start time did I set? Answer with only the time.',
        'Return one line with the current room and time: room | time.'], ['Maple', '14:00']),
    ('ja', [
        '買い物メモです。牛乳は2本です。確認としてOKだけ返してください。',
        'パンは3個です。確認としてOKだけ返してください。',
        '牛乳を2本から1本に変更します。確認としてOKだけ返してください。',
        '牛乳はいま何本ですか。数字だけ返してください。',
        'パンは何個ですか。数字だけ返してください。',
        '現在の買い物メモを「牛乳: 数字 / パン: 数字」の1行で返してください。'], ['1', '3']),
    ('zh', [
        '旅行安排：目的地是京都。请记住，只回复OK。',
        '出发时间是09:30。只回复OK。',
        '把目的地从京都改为大阪。只回复OK。',
        '现在的目的地是哪里？只回复地名。',
        '出发时间是什么？只回复时间。',
        '用一行返回当前目的地和时间：目的地 | 时间。'], ['大阪', '09:30']),
]


def format_checks(canonical):
    expected = {'en': ['OK', 'OK', 'OK', 'Maple', '14:00', 'Maple|14:00'],
                'ja': ['OK', 'OK', 'OK', '1', '3', '牛乳:1/パン:3'],
                'zh': ['OK', 'OK', 'OK', '大阪', '09:30', '大阪|09:30']}
    checks = []
    for item in canonical:
        language, turn = item['label'].split('-')
        index=int(turn)-1
        target = expected[language][index if index<6 else 3+(index-6)%3]
        raw = item['answer'].strip()
        one_line = len(raw.splitlines()) <= 1
        # Permit spacing around separators, not within factual values, and never hide line breaks.
        observed = re.sub(r'[^\S\r\n]*([|/:])[^\S\r\n]*', r'\1', raw)
        checks.append(dict(label=item['label'], expected=target, observed=observed, one_line=one_line,
                           passed=one_line and observed == target))
    return checks


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for key in ['python', 'model', 'store', 'output-dir']:
        parser.add_argument('--' + key, required=True, type=Path)
    parser.add_argument('--maintain', type=Path, help='Required for legacy manual-trim mode')
    parser.add_argument('--auto-eviction', action='store_true', help='Validate live eviction rather than manual trim')
    parser.add_argument('--near-limit', action='store_true', help='Nine real conversation turns per language, final prompt exactly at context boundary')
    parser.add_argument('--quota-mib', type=int, default=128)
    parser.add_argument('--port', type=int, default=0, help='0 selects a free loopback port for each owned worker')
    args = parser.parse_args()
    if not args.auto_eviction and args.maintain is None: parser.error('--maintain is required for manual-trim mode')
    if args.near_limit and (not args.auto_eviction or args.quota_mib != 256):
        parser.error('Near-limit automatic-eviction fixture requires --auto-eviction --quota-mib 256')
    if not 64 <= args.quota_mib <= 4096: parser.error('Quota must be 64–4096 MiB')
    args.output_dir.mkdir(mode=0o700, exist_ok=False)
    runtime = Path(__file__).resolve().parent
    artifacts = [runtime / 'prefix-cache-http.py', runtime / 'prefix_cache_contract.py', runtime / 'prefix_cache_roundtrip_support.py', runtime / 'prefix_cache_writes.py', args.store]
    hashes = lambda: {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in artifacts}
    before_hashes = hashes()
    token = secrets.token_hex(24)
    headers = {'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'}
    env = dict(os.environ, SOLITON_PREFIX_HTTP_TOKEN=token)
    for key in list(env):
        if key.startswith(('MLX_', 'METAL_', 'VECLIB_', 'OMP_', 'TOKENIZERS_')) or key in {'PYTORCH_ENABLE_MPS_FALLBACK', 'PYTHONPATH', 'PYTHONHOME', 'PYTHONEXECUTABLE', '__PYVENV_LAUNCHER__'}:
            env.pop(key, None)
    base = f'http://127.0.0.1:{args.port}'
    quota = args.quota_mib * 1024**2
    output_limit = 16 if args.near_limit else 32
    tokenizer = None
    if args.near_limit:
        from transformers import AutoTokenizer
        tokenizer = AutoTokenizer.from_pretrained(str(args.model), local_files_only=True, trust_remote_code=False)
    extras = {
        'en': ['Which room are we using? Name only.', 'What time does it begin? Time only.', 'Give the current room and time: room | time.'],
        'ja': ['牛乳の本数を数字だけで答えてください。', 'パンの個数を数字だけで答えてください。', '現在のメモを「牛乳: 数字 / パン: 数字」で返してください。'],
        'zh': ['当前目的地是什么？只回复地名。', '几点出发？只回复时间。', '返回当前目的地和时间：目的地 | 时间。'],
    }
    live_evictions = []
    boundary_checks = []
    worker = log = None
    samples, canonical, quality = [], [], []

    def request(path='/studio/ssd-cache', body=None):
        req = urllib.request.Request(base + path, headers=headers, data=None if body is None else json.dumps(body).encode())
        with urllib.request.urlopen(req, timeout=90) as reply:
            return json.load(reply)

    def start(cache, label):
        nonlocal worker, log, base
        if args.port == 0:
            with socket.socket() as reservation:
                reservation.bind(('127.0.0.1', 0))
                port = reservation.getsockname()[1]
        else: port = args.port
        base = f'http://127.0.0.1:{port}'
        cache.mkdir(mode=0o700, exist_ok=True)
        log = (args.output_dir / (label + '.log')).open('x')
        worker = subprocess.Popen([str(args.python), str(runtime / 'prefix-cache-http.py'), '--model', str(args.model),
            '--cache-root', str(cache), '--quota-mib', str(args.quota_mib), '--store-executable', str(args.store), '--port', str(port), '--qualification-mode'],
            env=env, stdout=log, stderr=log, start_new_session=True)
        for _ in range(300):
            if worker.poll() is not None:
                raise RuntimeError('Worker failed: ' + label)
            try:
                return request()['identity']
            except OSError:
                time.sleep(.3)
        raise RuntimeError('Readiness timeout')

    def stop():
        nonlocal worker
        if worker is not None:
            try:
                if worker.poll() is None:
                    os.killpg(worker.pid, signal.SIGINT)
                worker.wait(timeout=15)
            except subprocess.TimeoutExpired:
                os.killpg(worker.pid, signal.SIGKILL); worker.wait()
                raise RuntimeError('Unclean worker stop')
            finally:
                worker = None; log.close()

    def infer(query, phase, label, cache):
        counted = request('/tokenize', query)
        assert counted['count'] + query['max_tokens'] <= 512
        stats = request()
        files = set((cache / 'store').glob('*.cache'))
        conn = http.client.HTTPConnection('127.0.0.1', int(base.rsplit(':', 1)[1]), timeout=90)
        began = time.perf_counter(); first = None; answer = ''; usage = None; finish = None
        try:
            conn.request('POST', '/v1/chat/completions', json.dumps(dict(query, stream=True, stream_options={'include_usage': True})), headers)
            reply = conn.getresponse()
            assert reply.status == 200
            for line in reply:
                if not line.startswith(b'data:'): continue
                data = line[5:].strip()
                if data == b'[DONE]': break
                frame = json.loads(data)
                content = (frame.get('choices') or [{}])[0].get('delta', {}).get('content', '')
                if content:
                    if first is None: first = time.perf_counter() - began
                    answer += content
                if frame.get('choices') and frame['choices'][0].get('finish_reason'): finish=frame['choices'][0]['finish_reason']
                if frame.get('usage'): usage = frame['usage']
            elapsed = time.perf_counter() - began
        finally: conn.close()
        assert usage is not None and finish in {'stop', 'length'}
        assert counted['count'] == usage['prompt_tokens']
        after = request()
        current = set((cache / 'store').glob('*.cache'))
        removed = sorted(p.name for p in files - current)
        if phase == 'growing': live_evictions.extend(removed)
        used = sum(p.stat().st_size for p in (cache/'store').iterdir() if p.name != '.lock')
        assert used <= quota
        entry = sorted(p.name for p in current - files)
        delta = {k: after[k] - stats[k] for k in ['hits', 'misses', 'writes', 'errors', 'skippedWrites', 'serializedBytes']}
        samples.append(dict(phase=phase, label=label, answer=answer, usage=usage, counters=delta,
                            first_content_seconds=first, completion_seconds=elapsed, new_entries=entry, removed_entries=removed, store_bytes=used, finish_reason=finish, empty=not answer))
        (args.output_dir / 'progress.json').write_text(json.dumps(dict(samples=samples, canonical=canonical, quality=quality), ensure_ascii=False))
        return answer, entry

    def maintenance(cache, operation):
        result = subprocess.run([str(args.maintain), str(cache / 'store'), str(quota), operation], capture_output=True, text=True, timeout=15, check=True)
        return json.loads(result.stdout)

    try:
        cache = args.output_dir / 'conversation-cache'
        identity = start(cache, 'warm')
        for language, original_prompts, expected in CASES:
            prompts = original_prompts + extras[language] if args.near_limit else original_prompts
            messages = []
            for turn, prompt in enumerate(prompts):
                messages.append(dict(role='user', content=prompt))
                query = dict(model='default_model', messages=[dict(m) for m in messages], temperature=0, seed=0, max_tokens=output_limit)
                if args.near_limit and turn == len(prompts)-1:
                    # Only the terminal user turn is padded; all earlier actual turns/replies are retained.
                    target = 512-output_limit
                    found = False
                    for count in range(target+1):
                        for suffix in ['', '.', ' x', '!']:
                            query['messages'][-1]['content'] = 'Notes: ' + 'a '*count + suffix + '\n' + prompt
                            tokens = tokenizer.apply_chat_template(query['messages'],tokenize=True,add_generation_prompt=True,return_dict=False)
                            if len(tokens) == target:
                                found=True;break
                        if found:break
                    assert found, 'Cannot build exact boundary without dropping conversation turns'
                    counted = request('/tokenize',query)
                    assert counted['count'] == target
                    try:
                        request('/v1/chat/completions',dict(query,max_tokens=output_limit+1))
                        raise AssertionError('One-token-over continuation accepted')
                    except urllib.error.HTTPError as error: assert error.code == 422
                    boundary_checks.append(dict(language=language,input_tokens=target,output_tokens=output_limit,messages=len(query['messages']),one_token_over_rejected=True))
                label = f'{language}-{turn+1}'
                answer, entries = infer(query, 'growing', label, cache)
                canonical.append(dict(label=label, query=query, answer=answer, entries=entries))
                repeated, _ = infer(query, 'repeat', label, cache)
                assert repeated == answer, 'Repeated answer changed: ' + label
                if turn >= 3:
                    probe=(turn-3)%3
                    facts = [expected[probe]] if probe < 2 else expected
                    quality.append(dict(label=label, expected=facts, passed=all(fact in answer for fact in facts)))
                messages.append(dict(role='assistant', content=answer))
            print(json.dumps(dict(phase='growing', language=language, stats=request())), flush=True)
        # Exercise an oversized continuation without silently dropping conversation turns.
        oversized = dict(query, messages=query['messages'] + [dict(role='assistant', content=answer), dict(role='user', content=' a' * 600)])
        try:
            request('/v1/chat/completions', oversized)
            raise RuntimeError('Oversized conversation accepted')
        except urllib.error.HTTPError as e: assert e.code == 422
        stop()
        if args.auto_eviction:
            assert live_evictions, 'Workload did not exercise live eviction'
            assert all(s['counters']['errors']==0 for s in samples), 'Unexpected storage error'
            trimmed = None
        else:
            inventory = maintenance(cache, 'stats')
            assert inventory['usedBytes'] <= quota
            assert inventory['canTrim'], 'The workload did not reach the trim threshold'
            trimmed = maintenance(cache, 'trim')
            assert trimmed['targetMet'] and trimmed['removedBytes'] > 0
        remaining = {p.name for p in (cache / 'store').glob('*.cache')}
        evicted = next(c for c in canonical if c['entries'] and c['entries'][0] not in remaining)
        retained = next(c for c in reversed(canonical) if c['entries'] and c['entries'][0] in remaining)
        assert start(cache, 'after-trim') == identity
        answer, _ = infer(retained['query'], 'retained-after-eviction' if args.auto_eviction else 'retained-after-trim', retained['label'], cache)
        assert answer == retained['answer'], ('Retained answer mismatch', samples[-1])
        assert samples[-1]['counters']['hits'] == 1
        # Clear RAM between probes: a shared initial template prefix can otherwise bypass disk lookup.
        stop()
        assert start(cache, 'after-trim-evicted') == identity
        answer, _ = infer(evicted['query'], 'evicted-after-eviction' if args.auto_eviction else 'evicted-after-trim', evicted['label'], cache)
        assert answer == evicted['answer'], ('Evicted answer mismatch', samples[-1])
        assert samples[-1]['counters']['misses'] == 1
        infer(evicted['query'], 'repeat-after-repair', evicted['label'], cache)
        assert samples[-1]['counters']['skippedWrites'] == 1
        stop()
        # Fresh process/store for every canonical history: no RAM or disk reuse.
        for index, item in enumerate(canonical):
            fresh = args.output_dir / f'fresh-{index}'
            start(fresh, f'fresh-{index}')
            answer, _ = infer(item['query'], 'fresh', item['label'], fresh)
            assert answer == item['answer'], 'Fresh answer differs: ' + item['label']
            assert samples[-1]['usage']['prompt_tokens_details']['cached_tokens'] == 0
            stop()
            print(json.dumps(dict(phase='fresh', checked=index+1, total=len(canonical))), flush=True)
        assert hashes() == before_hashes, 'Worker artifacts changed during validation'
        summary = {}
        for phase in sorted({s['phase'] for s in samples}):
            subset = [s for s in samples if s['phase'] == phase]
            summary[phase] = dict(requests=len(subset), writes=sum(s['counters']['writes'] for s in subset),
                skipped_writes=sum(s['counters']['skippedWrites'] for s in subset), cache_errors=sum(s['counters']['errors'] for s in subset),
                first_content_median_ms=statistics.median(s['first_content_seconds']*1000 for s in subset if s['first_content_seconds'] is not None) if any(s['first_content_seconds'] is not None for s in subset) else None,
                completion_median_ms=statistics.median(s['completion_seconds']*1000 for s in subset))
        formats = format_checks(canonical)
        report = dict(result='passed', scope='Cache-functional validation with synthetic practical multi-turn EN/JA/ZH fixtures; model format adherence is reported separately',
            context=512, output_limit=output_limit, quota_bytes=quota, automatic_eviction=args.auto_eviction, live_evicted_entries=len(live_evictions), boundary_checks=boundary_checks, identity=identity, worker_hashes=before_hashes,
            quality_checks=quality, quality_passed=sum(q['passed'] for q in quality), quality_total=len(quality),
            fresh_text_parity=len(canonical), boundary_rejected=True, trim=trimmed, summary=summary, canonical=canonical, samples=samples,
            truncated_histories=[s['label'] for s in samples if s['phase']=='growing' and s['finish_reason']=='length'],
            empty_histories=[s['label'] for s in samples if s['phase']=='growing' and s['empty']],
            format_assessment='exact-single-line-v2', format_checks=formats, format_passed=sum(item['passed'] for item in formats), format_total=len(formats),
            qualificationEvidenceIssued=False)
        (args.output_dir / 'validation.json').write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n')
        print(json.dumps(dict(result='passed', summary=summary, quality=quality, trim=trimmed), ensure_ascii=False), flush=True)
    finally: stop()


if __name__ == '__main__': main()
