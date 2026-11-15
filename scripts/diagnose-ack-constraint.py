"""Loopback-only diagnostic for a one-token Gemma 2 acknowledgement constraint."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import runpy
import statistics
import subprocess
import sys
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
CASES = runpy.run_path(str(ROOT / 'scripts/validate-prefix-multiturn.py'))
SENTINEL = {4083: 100.0}
INSTRUCTIONS = {
    'en': 'Copy this answer exactly:',
    'ja': '次の答えだけをそのまま出力：',
    'zh': '只原样输出以下答案：',
}


def launch_backend(wrapper, model, port):
    import importlib.metadata
    import mlx.core as mx
    from mlx_lm import server
    from transformers import AutoTokenizer

    assert importlib.metadata.version('mlx-lm') == '0.32.2'
    assert hashlib.sha256(Path(server.__file__).read_bytes()).hexdigest() == (
        '8d475058966cf90afcf8cb857f218c2b9dbc01ad1c506d9726de655a90d80723'
    )
    tokenizer = AutoTokenizer.from_pretrained(model, local_files_only=True)
    assert tokenizer.encode('OK', add_special_tokens=False) == [4083]
    assert tokenizer.decode([4083]) == 'OK' and tokenizer.eos_token_id == 1
    original = server._make_logits_processors

    def processors(args):
        if args.logits.logit_bias != SENTINEL:
            return original(args)
        next_token = 4083

        def force_token(_tokens, logits):
            nonlocal next_token
            allowed = mx.arange(logits.shape[-1]) == next_token
            next_token = 1
            return mx.where(allowed, mx.zeros_like(logits), -float('inf'))

        return [force_token]

    server._make_logits_processors = processors
    sys.argv = [str(wrapper), '--model', model, '--host', '127.0.0.1', '--port', str(port),
                '--prefill-step-size', '512', '--prompt-cache-bytes', str(1536 * 1024**2)]
    runpy.run_path(str(wrapper), run_name='__main__')


def compare(api, replay_mismatch):
    def complete(messages, constrained=False):
        body = dict(model='default_model', messages=messages, temperature=0, max_tokens=64)
        if constrained:
            body.update(seed=0, logit_bias=SENTINEL)
        req = urllib.request.Request(api + '/v1/chat/completions', json.dumps(body).encode(),
                                     {'Content-Type': 'application/json'})
        with urllib.request.urlopen(req, timeout=60) as response:
            choice = json.load(response)['choices'][0]
        return choice['message']['content'], choice['finish_reason']

    rows = []
    for language, prompts, facts in CASES['CASES']:
        histories = {'current': [], 'constrained': []}
        for turn, prompt in enumerate(prompts, 1):
            label = f'{language}-{turn}'
            target = CASES['format_checks']([dict(label=label, answer='')])[0]['expected']
            result = dict(label=label, expected=target)
            order = ('current', 'constrained') if turn % 2 else ('constrained', 'current')
            for mode in order:
                messages = histories[mode]
                messages.append(dict(role='user', content=prompt))
                began = time.monotonic()
                first, finish = complete(messages)
                injected = replay_mismatch and label in {'en-3', 'zh-3'}
                if injected:
                    assert finish == 'stop'
                policy_input = 'OK.' if injected else first
                answer = policy_input
                retried = finish == 'stop' and policy_input.strip() != target
                used_constraint = retried and mode == 'constrained' and target == 'OK'
                if retried:
                    retry = messages[:-1] + [dict(role='user',
                        content=f'{prompt}\n\n{INSTRUCTIONS[language]}\n{target}')]
                    answer, finish = complete(retry, constrained=used_constraint)
                elapsed = (time.monotonic() - began) * 1000
                accepted = finish == 'stop' and answer.strip() == target
                if used_constraint:
                    assert accepted and answer == 'OK'
                # A rejected answer does not enter the conversation history.
                messages.append(dict(role='assistant', content=answer if accepted else ''))
                result[mode] = dict(first=first, answer=answer, finish=finish,
                    retried=retried, usedConstraint=used_constraint, injectedMismatch=injected,
                    accepted=accepted, responseMs=elapsed)
            if turn == 4:
                assert target == facts[0]
                for mode in histories:
                    result[mode]['correctUpdatedFact'] = result[mode]['accepted']
            rows.append(result)
            print(label, *(result[m]['accepted'] for m in histories), flush=True)
    return dict(cases=rows, summary={
        mode: dict(accepted=sum(row[mode]['accepted'] for row in rows),
                   correctUpdatedFacts=sum(row[mode].get('correctUpdatedFact', False) for row in rows),
                   medianResponseMs=statistics.median(row[mode]['responseMs'] for row in rows))
        for mode in ('current', 'constrained')
    })


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for key in ('backend-python', 'wrapper', 'model', 'output'):
        parser.add_argument('--' + key, type=Path, required=True)
    parser.add_argument('--replay-mismatch', action='store_true')
    args = parser.parse_args()
    port = 19243
    api = f'http://127.0.0.1:{port}'
    with args.output.open('x') as output, args.output.with_suffix('.log').open('x') as log:
        child = subprocess.Popen([str(args.backend_python), '-I', '-u', str(Path(__file__).resolve()),
                                  str(args.wrapper.resolve()), str(args.model.resolve()), str(port)],
                                 env=dict(os.environ, VLLM_APPLE_KV_CACHE_MIB='1536',
                                          VLLM_APPLE_ALLOCATOR_CACHE_MIB='512'),
                                 stdout=log, stderr=log)
        try:
            for _ in range(240):
                assert child.poll() is None, 'Diagnostic backend exited'
                try:
                    with urllib.request.urlopen(api + '/v1/models', timeout=1):
                        break
                except OSError:
                    time.sleep(.25)
            else:
                raise RuntimeError('Diagnostic backend did not start')
            report = compare(api, args.replay_mismatch)
        finally:
            child.terminate()
            try:
                child.wait(timeout=15)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
        json.dump(report, output, ensure_ascii=False, indent=2)
        output.write('\n')


if __name__ == '__main__':
    if len(sys.argv) == 4:
        launch_backend(Path(sys.argv[1]), sys.argv[2], int(sys.argv[3]))
    else:
        main()
