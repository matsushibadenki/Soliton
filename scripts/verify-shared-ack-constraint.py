"""Compare one retry on natural format mismatches through an isolated shared API."""
import argparse
import hashlib
import json
from pathlib import Path
import runpy
import statistics
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
diagnostic = runpy.run_path(str(ROOT / 'scripts/diagnose-ack-constraint.py'))
fixtures = diagnostic['CASES']


# Values vary while the six-turn update and recall sequence stays comparable.
VARIANTS = {
    'en': [('Cedar', 'Maple', '14:00', 'Birch', 'Oak', '10:15'),
           ('Cedar', 'Maple', '14:00', 'Pine', 'Elm', '16:45')],
    'ja': [('牛乳', 'パン', '3', '水', 'りんご', '4'),
           ('牛乳', 'パン', '3', 'お茶', 'バナナ', '5')],
    'zh': [('京都', '大阪', '09:30', '东京', '名古屋', '10:20'),
           ('京都', '大阪', '09:30', '上海', '杭州', '16:15')],
}


def conversations():
    for language, prompts, _ in fixtures['CASES']:
        expected = [fixtures['format_checks']([dict(label=f'{language}-{turn}', answer='')])[0]['expected']
                    for turn in range(1, 7)]
        yield language, 0, prompts, expected
        for index, variant in enumerate(VARIANTS[language], 1):
            replacements = list(zip(variant[:3], variant[3:]))

            def replace(text):
                for old, new in replacements:
                    text = text.replace(old, new)
                return text

            yield language, index, list(map(replace, prompts)), list(map(replace, expected))


def yes_no_conversations():
    variants = {
        'en': [('Cedar', 'Maple'), ('Birch', 'Oak'), ('Pine', 'Elm'), ('Ash', 'Willow')],
        'ja': [('松', '桜'), ('梅', '竹'), ('楓', '椿'), ('桃', '藤')],
        'zh': [('北厅', '南厅'), ('东厅', '西厅'), ('梅厅', '竹厅'), ('红厅', '蓝厅')],
    }
    for language, pairs in variants.items():
        for index, (old, new) in enumerate(pairs):
            if language == 'en':
                prompts = [
                    f'The meeting room is {old}. Is the room {old}? Answer YES or NO.',
                    f'The meeting room has changed to {new}. Is the room still {old}? Answer YES or NO.',
                    f'Is the current room {new}? Answer YES or NO.',
                    f'Is the current room {old}? Answer YES or NO.',
                    f'Was the original room {old}? Answer YES or NO.',
                    f'Was the original room {new}? Answer YES or NO.',
                ]
            elif language == 'ja':
                prompts = [
                    f'会議室は「{old}」です。会議室は「{old}」ですか？',
                    f'会議室を「{new}」に変更しました。会議室はまだ「{old}」ですか？',
                    f'現在の会議室は「{new}」ですか？',
                    f'現在の会議室は「{old}」ですか？',
                    f'最初の会議室は「{old}」でしたか？',
                    f'最初の会議室は「{new}」でしたか？',
                ]
                prompts = [prompt + ' YES または NO だけで答えてください。' for prompt in prompts]
            else:
                prompts = [
                    f'会议地点是{old}。会议在{old}吗？',
                    f'会议地点改为{new}。会议还在{old}吗？',
                    f'会议现在在{new}吗？',
                    f'会议现在在{old}吗？',
                    f'最初的会议地点是{old}吗？',
                    f'最初的会议地点是{new}吗？',
                ]
                prompts = [prompt + '只回答 YES 或 NO。' for prompt in prompts]
            yield language, index, prompts, ['YES', 'NO', 'YES', 'NO', 'YES', 'NO']


CHOICE_INSTRUCTIONS = {
    'en': 'Output one listed string only, without quotes:',
    'ja': '次の候補から1つだけ、引用符なしで出力：',
    'zh': '只输出一个候选字符串，不加引号：',
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--api', required=True, choices=['http://127.0.0.1:18099'])
    parser.add_argument('--evidence', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--gateway-option', action='store_true')
    parser.add_argument('--yes-no', action='store_true')
    args = parser.parse_args()
    assert not (args.yes_no and args.gateway_option)
    evidence = args.evidence.read_bytes()
    qualification = json.loads(evidence)
    assert qualification['passed'] and qualification['shutdown_clean']
    assert qualification['soak']['elapsed_seconds'] >= 1800
    headers = {'Origin': args.api, 'Content-Type': 'application/json'}

    def call(path, body=None):
        req = urllib.request.Request(args.api + path, None if body is None else
                                     json.dumps(body).encode(), headers)
        with urllib.request.urlopen(req, timeout=120) as response:
            return json.load(response)

    def complete(messages, constrained=False):
        body = dict(model='default_model', messages=messages, stream=False, temperature=0, max_tokens=64)
        if constrained:
            body.update(seed=0, logit_bias=diagnostic['YES_NO_SENTINEL' if args.yes_no else 'SENTINEL'])
        reply = call('/v1/chat/completions', body)
        choice = reply['choices'][0]
        return choice['message']['content'], choice['finish_reason'], reply['usage']

    def gateway_complete(messages, language, expected, use_constraint):
        rule = dict(one_of=[expected], retry_once=True,
                    retry_language='zh-CN' if language == 'zh' else language,
                    constrain_ok_retry=use_constraint and expected == 'OK')
        body = dict(model='default_model', messages=messages, stream=False, temperature=0,
                    max_tokens=64, soliton_output_validation=rule)
        req = urllib.request.Request(args.api + '/v1/chat/completions',
                                     json.dumps(body).encode(), headers)
        try:
            with urllib.request.urlopen(req, timeout=120) as response:
                retry = response.headers['x-soliton-output-retry']
                result = json.load(response)
        except urllib.error.HTTPError as error:
            assert error.code == 422 and 'choices' not in json.load(error)
            return None, None, False, False
        assert retry in {'none', 'instruction', 'constrained'}
        answer = result['choices'][0]['message']['content']
        assert answer.strip() == expected
        if retry == 'constrained':
            assert use_constraint and answer == 'OK' and result['usage']['completion_tokens'] <= 2
        return answer, result['usage'], retry != 'none', retry == 'constrained'

    headers['x-studio-token'] = call('/studio/session', {})['token']
    initial = call('/studio/state')
    assert initial['status'] == 'offline'
    settings = initial['settings']['vllm']
    assert Path(settings['evidence']).resolve() == args.evidence.resolve()
    assert [settings[k] for k in ('context', 'kvCacheMiB', 'allocatorCacheMiB')] == [4096, 1536, 512]
    report = dict(passed=False, evidenceSha256=hashlib.sha256(evidence).hexdigest(),
                  qualificationSeconds=qualification['soak']['elapsed_seconds'],
                  qualificationSuccesses=qualification['soak']['successes'], cases=[])
    order = ('current', 'constrained', 'constrained', 'current')
    report['order'] = order
    report['gatewayOption'] = args.gateway_option
    report['yesNo'] = args.yes_no
    report['warmup'] = []
    sessions = set()
    with args.output.open('x') as output:
        try:
            for run, mode in enumerate(order, 1):
                call('/studio/action', {'action': 'start'})
                try:
                    for _ in range(300):
                        state = call('/studio/state')
                        if state['status'] == 'ready':
                            break
                        assert state['status'] != 'offline', 'Qualified startup refused'
                        time.sleep(.25)
                    else:
                        raise RuntimeError('Qualified startup timed out')
                    session = state['engineSession']
                    assert session is not None and session not in sessions
                    sessions.add(session)
                    warmup = []
                    for language, text in [('en', 'Name a color.'), ('ja', '色を一つ答えて。'),
                                           ('zh', '说出一种颜色。')]:
                        _, _, usage = complete([dict(role='user', content=text)])
                        warmup.append(dict(language=language, usage=usage))
                    report['warmup'].append(dict(run=run, mode=mode, cases=warmup))
                    for language, case, prompts, targets in (yes_no_conversations() if args.yes_no else conversations()):
                        messages = []
                        for turn, (prompt, expected) in enumerate(zip(prompts, targets), 1):
                            messages.append(dict(role='user', content=prompt))
                            start = time.monotonic()
                            allowed = {'YES', 'NO'} if args.yes_no else {expected}
                            if args.gateway_option:
                                answer, usage, retry, forced = gateway_complete(
                                    messages, language, expected, mode == 'constrained')
                                elapsed = (time.monotonic() - start) * 1000
                                accepted = answer is not None
                                first, retry_usage = None, None
                            else:
                                first, finish, usage = complete(messages)
                                retry = finish == 'stop' and first.strip() not in allowed
                                forced = retry and mode == 'constrained' and (args.yes_no or expected == 'OK')
                                answer, retry_usage = first, None
                                if retry:
                                    instruction = (CHOICE_INSTRUCTIONS if args.yes_no
                                                   else diagnostic['INSTRUCTIONS'])[language]
                                    target = '["YES","NO"]' if args.yes_no else expected
                                    revised = messages[:-1] + [dict(role='user',
                                        content=f'{prompt}\n\n{instruction}\n{target}')]
                                    answer, finish, retry_usage = complete(revised, forced)
                                elapsed = (time.monotonic() - start) * 1000
                                accepted = finish == 'stop' and answer.strip() in allowed
                                if forced:
                                    assert accepted and answer in allowed and retry_usage['completion_tokens'] <= 2
                            # Use the same whitespace normalization as the output validator.
                            messages.append(dict(role='assistant', content=answer.strip() if accepted else ''))
                            report['cases'].append(dict(run=run, mode=mode, language=language,
                                case=case, turn=turn, expected=expected, first=first, answer=answer,
                                retried=retry, constrained=forced, accepted=accepted,
                                correct=accepted and answer.strip() == expected,
                                responseMs=elapsed, initialUsage=usage, retryUsage=retry_usage))
                    final = call('/studio/state')
                    assert final['engineSession'] == session
                    assert final['document']['messages'] == initial['document']['messages']
                    print(run, mode, len(report['cases']), flush=True)
                finally:
                    call('/studio/action', {'action': 'stop'})
                    assert call('/studio/state')['status'] == 'offline'
            report['summary'] = {}
            for mode in ('current', 'constrained'):
                rows = [r for r in report['cases'] if r['mode'] == mode]
                assert len(rows) == (144 if args.yes_no else 108)
                recalls = [r for r in rows if r['turn'] == 4]
                retries = [r for r in rows if r['retried']]
                ok_retries = [r for r in retries if r['expected'] == 'OK']
                summary = dict(
                    requests=len(rows), accepted=sum(r['accepted'] for r in rows),
                    naturalMismatches=len(retries), acknowledgementRetries=len(ok_retries),
                    acceptedAcknowledgementRetries=sum(r['accepted'] for r in ok_retries),
                    correct=sum(r['correct'] for r in rows),
                    correctUpdatedFacts=sum(r['correct'] for r in recalls), updatedFactRequests=len(recalls),
                    initialCachedTokens=None if args.gateway_option else sum(
                        r['initialUsage']['prompt_tokens_details']['cached_tokens'] for r in rows),
                    medianResponseMs=statistics.median(r['responseMs'] for r in rows),
                    medianAcknowledgementRetryResponseMs=statistics.median(r['responseMs'] for r in ok_retries)
                    if ok_retries else None)
                if args.gateway_option:
                    # Retry headers are only returned for accepted completions.
                    summary['successfulRetries'] = summary.pop('naturalMismatches')
                if args.yes_no:
                    summary.update(acceptedRetries=sum(r['accepted'] for r in retries),
                        correctRetries=sum(r['correct'] for r in retries),
                        constrainedRetries=sum(r['constrained'] for r in rows),
                        medianRetriedResponseMs=statistics.median(r['responseMs'] for r in retries)
                        if retries else None)
                report['summary'][mode] = summary
            report['passed'] = True
        finally:
            json.dump(report, output, ensure_ascii=False, indent=2)
            output.write('\n')


if __name__ == '__main__':
    main()
