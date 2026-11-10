"""Serial old/candidate real-model parity and internal wall-span probe.

Requires dedicated controllers, a local model and new output; does not issue
qualification evidence or edit settings. Never run beside another model job.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import signal
import sys
import urllib.request


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--isolated', action='store_true', required=True)
    for name in ('before', 'after', 'executable', 'model', 'fixtures', 'output'):
        p.add_argument('--'+name, type=Path, required=True)
    p.add_argument('--port', type=int, default=19235)
    a = p.parse_args()
    if (not 1024 <= a.port <= 65535 or a.port in (8080,8081)
            or not all(x.is_absolute() for x in [a.before,a.after,a.executable,a.model,a.fixtures,a.output])):
        p.error('Use absolute paths and a dedicated loopback port')
    # Reserve output immediately; retain a failed report rather than overwrite it.
    with a.output.open('x') as f:
        f.write('{}\n')
    report = dict(passed=False, certification=False, scope='Serial direct-backend parity; no controlled speedup claim', runs=[])
    try:
        fixtures = json.loads(a.fixtures.read_text())
        assert all(x.is_dir() for x in (a.before,a.after,a.model)) and a.executable.is_file()
        assert len(fixtures) == 6 and {f['language'] for f in fixtures} == {'en','ja','zh'}
        report['fixturesSha256'] = hashlib.sha256(a.fixtures.read_bytes()).hexdigest()
        if a.before == a.after:
            raise ValueError('Before and candidate controllers must differ')
        # Subprocesses keep monkeypatches/modules completely separate, using the
        # same interpreter/environment/model, serially and with owned cleanup.
        for controller in (a.before,a.after):
            command = [sys.executable, str(Path(__file__).resolve()), '--worker',
                       str(controller),str(a.executable),str(a.model),str(a.fixtures),str(a.port)]
            process = subprocess.Popen(command, stdout=subprocess.PIPE, env=os.environ.copy())
            try:
                raw, _ = process.communicate(timeout=900)
                if process.returncode:
                    raise subprocess.CalledProcessError(process.returncode, command)
            except BaseException:
                # Give the worker's finally block time to stop its owned backend;
                # subprocess.run(timeout=...) would kill only the worker first.
                if process.poll() is None:
                    process.send_signal(signal.SIGINT)
                    try:
                        process.communicate(timeout=60)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.communicate()
                raise
            report['runs'].append(json.loads(raw))
        before, after = report['runs']
        for old,new in zip(before['samples'],after['samples']):
            assert (old['label'],old['seed'],old['stream']) == (new['label'],new['seed'],new['stream'])
            assert old['answer'] == new['answer']
            assert old['usage'] == new['usage']
            assert old['finish'] == new['finish']
        assert len(before['samples']) == len(after['samples']) == 24
        rows = after['diagnostics']['internal_spans']['spans']
        for name in ['cache.fetch','cache.insert','cache.trim','batch.prefill','batch.decode',
                     'sequential.prefill_first','sequential.decode']:
            assert rows[name]['calls'] > 0 and rows[name]['failures'] == 0, name
            assert 0 <= rows[name]['last_ms'] <= rows[name]['max_ms'] <= rows[name]['total_ms']
        assert after['diagnostics']['retained_bytes'] <= after['diagnostics']['max_bytes'] == 1536 * 1024**2
        report['passed'] = True
    except BaseException as e:
        report['error'] = f'{type(e).__name__}: {e}'
        raise
    finally:
        a.output.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')


def worker():
    controller, executable, model, fixtures, port = sys.argv[2:]
    sys.path.insert(0,controller)
    from vllm_apple.backend import BackendConfig, BackendProcess
    os.environ.update(PYTORCH_ENABLE_MPS_FALLBACK='1',VLLM_APPLE_KV_CACHE_MIB='1536',VLLM_APPLE_ALLOCATOR_CACHE_MIB='512')
    backend=BackendProcess(BackendConfig(model,Path(executable),port=int(port),max_model_len=4096,backend_kind='mlx_lm'))
    report={'samples':[],'shutdownClean':False}
    try:
        backend.start()
        for fixture in json.loads(Path(fixtures).read_text()):
            for seed in (None,0):
                for stream in (False,True):
                    body=dict(model='default_model',messages=fixture['messages'],max_tokens=64,temperature=0,stream=stream)
                    if seed is not None:body['seed']=seed
                    if stream:body['stream_options']={'include_usage':True}
                    req=urllib.request.Request(backend.base_url+'/v1/chat/completions',json.dumps(body).encode(),{'Content-Type':'application/json'})
                    with urllib.request.urlopen(req,timeout=180) as response:raw=response.read(4*1024**2+1)
                    assert len(raw)<=4*1024**2
                    if stream:
                        answer='';usage=None;finish=None;done=False
                        for line in raw.decode().splitlines():
                            if not line.startswith('data:'):continue
                            text=line[5:].strip()
                            if text=='[DONE]':done=True;continue
                            event=json.loads(text);usage=event.get('usage') or usage
                            for choice in event.get('choices',[]):
                                answer+=choice.get('delta',{}).get('content','')
                                finish=choice.get('finish_reason') or finish
                        assert done
                    else:
                        event=json.loads(raw);answer=event['choices'][0]['message']['content'];usage=event['usage'];finish=event['choices'][0]['finish_reason']
                    assert answer.strip()=='STRATA-7391' and usage['prompt_tokens']+64<=4096
                    report['samples'].append(dict(label=fixture['label'],seed=seed,stream=stream,answer=answer,usage=usage,finish=finish))
        with urllib.request.urlopen(backend.base_url+'/vllm-apple/kv-cache',timeout=10) as response:report['diagnostics']=json.load(response)
    finally:
        backend.stop()
        report['shutdownClean']=not backend.running
    assert report['shutdownClean']
    print(json.dumps(report,ensure_ascii=False))


if __name__=='__main__':
    if sys.argv[1:2]==['--worker']:worker()
    else:main()
