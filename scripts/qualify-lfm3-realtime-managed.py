"""Qualify the modified Omni runtime through the authenticated Studio WebSocket route."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
PORT = 18099
URL = f'http://127.0.0.1:{PORT}'
MODES = ('text', 'audio', 'interleaved')
DURATION = 1800


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('media-proof', 'runtime', 'audio', 'output'):
        parser.add_argument('--' + name, required=True, type=Path)
    args = parser.parse_args()
    proof, runtime, audio, out = [p.resolve() for p in
                                  (args.media_proof, args.runtime, args.audio, args.output)]
    media = json.loads(proof.read_bytes())
    assert media['passed'] and media['shutdownClean'] and media['elapsedSeconds'] >= 1800
    assert media['failures'] == 0 and all(media['exactCountParity'][mode]
                                         for mode in ('text', 'image', 'audio', 'video'))
    out.mkdir(mode=0o700)
    assets = out / 'assets'
    assets.mkdir(mode=0o700)
    gguf = ROOT / '.soliton-runtime/lfm3-omni-2b'
    evidence = out / 'realtime-qualification.json'
    settings = dict(engine='lfm3Omni', omni=dict(
        runtimeDir=str(runtime), model=str(gguf / 'LFM3-Omni-2B-Q4_K_M.gguf'),
        projector=str(gguf / 'mmproj-LFM3-Omni-2B-Q8_0.gguf'), mediaRoot=str(assets),
        evidence=str(proof), realtimeEvidence=str(evidence), context=4096))
    state = out / 'state.json'
    state.write_text("{}\n")
    report = dict(passed=False, boundaries={}, failures=0, successes=0, shutdownClean=False,
                  mediaEvidenceSha256=hashlib.sha256(proof.read_bytes()).hexdigest())
    headers = {'Origin': URL, 'Content-Type': 'application/json'}

    def api(path, payload=None):
        request = urllib.request.Request(URL + path,
            None if payload is None else json.dumps(payload).encode(), headers)
        try:
            with urllib.request.urlopen(request, timeout=120) as response:
                return json.load(response)
        except urllib.error.HTTPError as error:
            raise RuntimeError(f'{path} HTTP {error.code}: {error.read(4096).decode(errors="replace")}') from error

    def status():
        return api('/studio/state')

    def action(name):
        return api('/studio/action', {'action': name})

    def ready(host):
        for _ in range(100):
            assert host.poll() is None
            try:
                token = api('/studio/session', {})['token']
                headers['x-studio-token'] = token
                break
            except OSError:
                time.sleep(.15)
        else:
            raise RuntimeError('Studio server unavailable')
        existing = status()['settings']
        desired = dict(existing, omni=settings["omni"])
        if desired != existing:
            api('/studio/action', {'action': 'settings', 'settings': desired})
        if existing["engine"] != settings["engine"]:
            api("/studio/action", {"action": "switchEngine", "engine": settings["engine"]})

        assert status()['settings']['engine'] == 'lfm3Omni'
        action('start')
        for _ in range(300):
            snapshot = status()
            if snapshot['status'] == 'ready':
                return token
            if snapshot['status'] == 'offline':
                raise RuntimeError('Managed Omni startup refused: ' + str(snapshot['logs'][-3:]))
            time.sleep(.25)
        raise RuntimeError('Managed Omni startup timed out')

    def probe(folder, token, mode, cap, *, boundary=False, cancel=False, unauthorized=False):
        folder.mkdir(mode=0o700)
        env = dict(os.environ, OMNI_PROBE_STUDIO='1', OMNI_PROBE_KEY=token,
                   OMNI_PROBE_WS=f'ws://127.0.0.1:{PORT}/v1/realtime',
                   OMNI_PROBE_OUTPUT=str(folder), OMNI_PROBE_AUDIO_DIR=str(audio),
                   OMNI_PROBE_MODE=mode, OMNI_PROBE_MAX_OUTPUT=str(cap))
        if boundary:
            env['OMNI_PROBE_BOUNDARY'] = '1'
        if cancel:
            env['OMNI_PROBE_CANCEL'] = '1'
        run = subprocess.run([ROOT / 'target/debug/examples/omni_realtime_probe'],
                             env=env, capture_output=True, timeout=180)
        if unauthorized:
            assert run.returncode != 0 and b'401' in run.stderr
            return []
        if run.returncode:
            raise RuntimeError(run.stderr.decode(errors='replace'))
        return [json.loads(line) for line in (folder / f'{mode}.jsonl').read_text().splitlines()]

    def start(log, qualification):
        command = [ROOT / 'target/debug/studio-server']
        if qualification:
            command.append('--omni-realtime-qualification')
        return subprocess.Popen(command, stdout=log, stderr=log,
                                env=dict(os.environ, STUDIO_PORT=str(PORT), STUDIO_STATE_PATH=str(state)))

    def stop(host):
        try:
            action('stop')
            assert status()['status'] == 'offline'
        finally:
            if host.poll() is None:
                host.send_signal(signal.SIGINT)
                host.wait(timeout=30)
        assert host.returncode == 0

    def events_of(events, kind):
        return [event for event in events if event['type'] == kind]

    def verify_turns(events, cap, mode):
        admissions = events_of(events, 'soliton.realtime.admission')
        results = events_of(events, 'response.done')
        assert len(admissions) == len(results) == 2 and not events_of(events, 'error')
        for admission, result in zip(admissions, results):
            usage = result['response']['usage']
            assert admission['accepted'] and result['response']['status'] == 'completed'
            assert usage['input_tokens'] == admission['input_tokens'] > 0
            assert 0 < usage['output_tokens'] <= cap
            if mode != 'text':
                assert result['soliton_output']['sampleRate'] == 24000
                assert result['soliton_output']['audioBytes'] > 0
        if mode != 'audio':
            text = results[-1]['soliton_output']['text'].lower()
            assert any(value in text for value in ('42', 'forty-two', 'forty two'))
        return len(results)

    with (out / 'host.log').open('xb') as log:
        host = start(log, True)
        try:
            token = ready(host)
            initial = status()
            probe(out / 'unauthorized', 'invalid', 'text', 1, boundary=True, unauthorized=True)
            report['unauthorizedRejected'] = True
            for mode in MODES:
                available = None
                for name in ('discover', 'exact', 'overflow'):
                    cap = 1 if name == 'discover' else available + (name == 'overflow')
                    events = probe(out / f'{mode}-{name}', token, mode, cap, boundary=True)
                    [admission] = events_of(events, 'soliton.realtime.admission')
                    if name == 'discover':
                        available = admission['available_positions']
                    assert admission['available_positions'] == available
                    assert admission['context_positions'] + available + 1 == 4096
                    assert admission['input_tokens'] == admission['context_positions']
                    assert admission['output_reservation'] == cap
                    terminal = events_of(events, 'response.done')
                    if name == 'overflow':
                        assert not admission['accepted'] and not terminal and events_of(events, 'error')
                        assert not events_of(events, 'soliton.audio.buffered')
                        assert not events_of(events, 'response.output_text.delta')
                    else:
                        assert admission['accepted'] and len(terminal) == 1
                        assert terminal[0]['response']['usage']['input_tokens'] == admission['input_tokens']
                report['boundaries'][mode] = dict(exact=available, overflow=available+1)
            events = probe(out / 'cancel', token, 'audio', 512, cancel=True)
            [done] = events_of(events, 'response.done')
            assert done['response']['status'] == 'cancelled'
            assert done['soliton_output']['cancelled'] and done['soliton_output']['audioBytes'] == 0
            report['cancellationVerified'] = True
            report['lifecycleVerified'] = status()['engineSession'] == initial['engineSession']
            assert report['lifecycleVerified']
            for mode in MODES:
                verify_turns(probe(out / f'turns-{mode}', token, mode, 128), 128, mode)
            report['preflightPassed'] = True
            (out / 'preflight.json').write_text(json.dumps(report, indent=2) + '\n')
            started = time.monotonic()
            import tempfile
            while time.monotonic() - started < DURATION:
                mode = MODES[(report['successes'] // 2) % len(MODES)]
                with tempfile.TemporaryDirectory(dir=out) as temp:
                    rows = probe(Path(temp) / 'run', token, mode, 128)
                    report['successes'] += verify_turns(rows, 128, mode)
            report['elapsedSeconds'] = time.monotonic() - started
            assert status()['document'] == initial['document']
            stop(host)
            report['shutdownClean'] = True
            report['passed'] = True
            certificate = dict(schema=1, scope='omni_realtime_30min', passed=True,
                identity=media['identity'], exactCountParity=media['exactCountParity'],
                elapsedSeconds=report['elapsedSeconds'], successes=report['successes'], failures=0,
                shutdownClean=True, createdAt=int(time.time()), realtimeChecks={
                    'exactInputCountParity': True, 'outputReservationVerified': True,
                    'cancellationVerified': True, 'historyVerified': True, 'sampleRateVerified': True})
            evidence.write_text(json.dumps(certificate, indent=2) + '\n')
            evidence.chmod(0o600)
            host = start(log, False)
            token = ready(host)
            verify_turns(probe(out / 'ordinary', token, 'text', 128), 128, 'text')
            stop(host)
            report['ordinaryQualifiedRoutePassed'] = True
        except BaseException as error:
            report.update(passed=False, error=str(error), failures=1)
            raise
        finally:
            try:
                if host.poll() is None:
                    stop(host)
            finally:
                (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()
