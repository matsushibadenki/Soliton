"""Isolated KV retention pilot. No startup evidence or ordinary settings changes.
Run with the frontend Python; fixed private backend settings and synthetic fixtures required.
"""
import argparse
import hashlib
import http.client
import json
import math
import os
from pathlib import Path
import select
import signal
import statistics
import subprocess
import sys
import time
import urllib.request

if not __debug__:
    raise RuntimeError('Validation assertions must be enabled')

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'vLLM-apple'))
from vllm_apple.architecture_evidence import capture_identity
from vllm_apple.backend import BackendConfig,BackendProcess
from vllm_apple.hardware import detect_hardware,detect_memory
from vllm_apple.model import inspect_model
from host_load_guard import HostLoadGuard
ORDER=[1024,1536,2048,2048,1536,1024]

def summarize(trials):
    assert [t['budgetMiB'] for t in trials]==ORDER,'Incomplete/reordered comparison'
    assert all(t['passed'] and t['shutdownClean'] and t['counterStopped'] for t in trials),'Failed trial'
    assert len({t['fixturesSha256'] for t in trials})==1
    reference=trials[0]
    stable=['backend','backend_sha256','config_sha256','hardware_sha256','model_root_sha256','runtime_sha256']
    for trial in trials:
        assert trial['identityBefore']==trial['identityAfter'],'Trial identity changed'
        assert all(trial['identityBefore'][k]==reference['identityBefore'][k] for k in stable),'Non-budget identity changed'
        assert [(r['label'],r['answer'],r['promptTokens'],r['completionTokens']) for r in trial['cases']]==[(r['label'],r['answer'],r['promptTokens'],r['completionTokens']) for r in reference['cases']],'Workload/output mismatch'
        assert all(math.isfinite(r['firstContentMs']) and r['firstContentMs']>0 for r in trial['cases'])
    out={}
    for budget in sorted(set(ORDER)):
        group=[t for t in trials if t['budgetMiB']==budget]
        revisit=[r for t in group for r in t['cases'] if r['phase']=='revisit']
        out[budget]={'fullHits':sum(r['cachedTokens']>=r['promptTokens']-1 for r in revisit),
            'revisitRequests':len(revisit),'revisitMedianFirstMs':statistics.median(r['firstContentMs'] for r in revisit),
            'recomputedTokens':sum(r['promptTokens']-r['cachedTokens'] for r in revisit),
            'sampledPeakPhysicalBytes':max(s['physicalBytes'] for t in group for s in t['memory']),
            'minimumAvailableBytes':min(s['availableBytes'] for t in group for s in t['memory']),
            'latencyEligible':all(r['cpu']['averageBusyPercent']<=50 and r['cpu']['sustainedHighSeconds']<1 and r['cpuBeforePercent']<=50 for t in group for r in t['cases'])}
    return out

def process_tree(root):
    rows=[tuple(map(int,line.split())) for line in subprocess.check_output(['/bin/ps','-axo','pid=,ppid='],text=True).splitlines() if line.strip()]
    owned={root}
    for _ in range(32):
        updated=owned|{pid for pid,parent in rows if parent in owned}
        if updated==owned:break
        owned=updated
    assert len(owned)<=64,'Owned tree exceeds probe bound'
    return sorted(owned)


def exclusive():
    for line in subprocess.check_output(['/bin/ps','-axo','command='],text=True).splitlines():
        words=line.strip().split()
        if not words:continue
        first=Path(words[0]).name
        assert first not in {'soliton','studio-server'},'A Soliton owner is running; no concurrent pilot'
        if first.lower().startswith('python'):
            assert not any(w.endswith('/mlx_kv_budget.py') for w in words),'Another owned model exists'
        assert first!='mlx_lm.server','Another model exists'


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--settings',type=Path,required=True)
    parser.add_argument('--fixtures',type=Path,required=True)
    parser.add_argument('--output-dir',type=Path,required=True)
    parser.add_argument('--memory-probe',type=Path,required=True)
    parser.add_argument('--port',type=int,default=19233)
    args=parser.parse_args();args.output_dir=args.output_dir.resolve();args.output_dir.mkdir(mode=0o700,exist_ok=False)
    original=args.settings.read_bytes();settings=json.loads(original)['settings']['vllm']
    assert settings['context']==4096 and settings['backend']=='mlx_lm' and settings['allocatorCacheMiB']==512
    model=settings['model'];exe=Path(settings['executable']);fixtures=json.loads(args.fixtures.read_text())
    assert len(fixtures)==18 and not any(t.get('secret') for t in fixtures)
    fixture_hash=hashlib.sha256(args.fixtures.read_bytes()).hexdigest()
    source_hash=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    manifest={'passed':False,'certification':False,'scope':'Direct supervised backend under guarded ordinary desktop load; not Rust gateway latency or startup approval','order':ORDER,'trials':[],'fixturesSha256':fixture_hash}
    def save(path,value):path.write_text(json.dumps(value,ensure_ascii=False,indent=2)+'\n');path.chmod(0o600)
    def interrupted(*_):raise KeyboardInterrupt('Owned experiment interrupted')
    signal.signal(signal.SIGINT,interrupted);signal.signal(signal.SIGTERM,interrupted)
    for trial_index,budget in enumerate(ORDER):
        backend=counter=guard=None;counter_log=None;pre=None
        report={'passed':False,'budgetMiB':budget,'fixturesSha256':fixture_hash,'cases':[],'memory':[]}
        os.environ.update(PYTORCH_ENABLE_MPS_FALLBACK='1',VLLM_APPLE_KV_CACHE_MIB=str(budget),VLLM_APPLE_ALLOCATOR_CACHE_MIB='512')
        stem=args.output_dir/f'{trial_index}-{budget}'
        try:
            exclusive()
            preflight=stem.with_suffix('.preflight.json')
            command=[settings['python'],'-m','vllm_apple','qualify-model',model,'--backend-kind','mlx_lm','--backend-executable',str(exe),'--candidate-mlx-lm-version','0.32.0','--max-model-len','4096','--concurrency','1','--duration','2','--warmup','1','--phase-samples','3','--allow-short-run','--backend-port',str(args.port),'--output',str(preflight)]
            with stem.with_suffix('.preflight.log').open('x') as log:
                pre=subprocess.Popen(command,cwd=ROOT/'vLLM-apple',stdout=log,stderr=log,start_new_session=True)
                assert pre.wait(timeout=240)==0,'Original candidate preflight failed'
            proof=json.loads(preflight.read_text());assert proof['passed'] and proof['shutdown_clean'] and not proof['soak']['certification_required'] and 'architecture_evidence' not in proof
            report['preflightSha256']=hashlib.sha256(preflight.read_bytes()).hexdigest()
            identity=capture_identity(inspect_model(Path(model),backend='mlx_lm'),exe,'mlx_lm',detect_hardware());report['identityBefore']=identity
            exclusive()
            backend=BackendProcess(BackendConfig(model,exe,port=args.port,max_model_len=4096,backend_kind='mlx_lm'));backend.start()
            counter_log=stem.with_suffix('.counter.log').open('xb')
            counter=subprocess.Popen([str(exe.parent/'python'),'-I','-u',str(ROOT/'crates/studio-core/src/token_budget.py'),model,'--soliton-token-counter'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=counter_log)
            guard=HostLoadGuard();guard.set_owned([os.getpid(),backend.pid,counter.pid])
            for index,fixture in enumerate(fixtures):
                counter.stdin.write((json.dumps({'id':index,'request':{'messages':fixture['messages']}})+'\n').encode());counter.stdin.flush()
                assert select.select([counter.stdout],[],[],30)[0],'Counter timeout'
                counted=json.loads(counter.stdout.readline(4097));assert counted['id']==index
                prompt=counted['prompt_tokens'];assert prompt+fixture['maxTokens']<=4096
                body=dict(model='default_model',messages=fixture['messages'],temperature=0,max_tokens=fixture['maxTokens'],stream=True,stream_options={'include_usage':True})
                mark=guard.mark();begin=time.monotonic();first=None;answer='';usage=None;finish=None;done=False
                conn=http.client.HTTPConnection('127.0.0.1',args.port,timeout=180)
                try:
                    conn.request('POST','/v1/chat/completions',json.dumps(body),{'Content-Type':'application/json'});response=conn.getresponse();assert response.status==200
                    for raw in response:
                        assert len(raw)<=65536
                        if not raw.startswith(b'data:'):continue
                        data=raw[5:].strip()
                        if data==b'[DONE]':done=True;break
                        event=json.loads(data);choices=event.get('choices') or []
                        if choices:
                            content=choices[0].get('delta',{}).get('content','')
                            if content:
                                if first is None:first=1000*(time.monotonic()-begin)
                                answer+=content
                            finish=choices[0].get('finish_reason') or finish
                        usage=event.get('usage') or usage
                    elapsed=1000*(time.monotonic()-begin)
                finally:conn.close()
                cpu=guard.interval(mark,guard.mark());assert done and first is not None and finish in {'stop','length'} and usage
                assert usage['prompt_tokens']==prompt and answer and (fixture['expected'] is None or answer.strip()==fixture['expected'])
                cached=usage['prompt_tokens_details']['cached_tokens'];assert type(cached) is int and 0<=cached<=prompt
                row=dict(label=fixture['label'],phase=fixture['phase'],answer=answer,promptTokens=prompt,cachedTokens=cached,completionTokens=usage['completion_tokens'],firstContentMs=first,elapsedMs=elapsed,cpu=cpu,cpuBeforePercent=mark['busyPercent'])
                report['cases'].append(row)
                for _ in range(3):
                    owned=process_tree(backend.pid)
                    probe=json.loads(subprocess.check_output([str(args.memory_probe),*map(str,owned)]))
                    if owned==process_tree(backend.pid):break
                else:raise RuntimeError('Unstable owned memory tree')
                assert probe['processCount']==len(owned) and probe['physicalBytes']>0
                with urllib.request.urlopen(backend.base_url+'/vllm-apple/kv-cache',timeout=10) as reply:kv=json.load(reply)
                assert kv['max_bytes']==budget*1024**2 and kv['retained_bytes']<=kv['max_bytes'] and kv['allocator']['configured_limit_bytes']==512*1024**2
                memory=detect_memory();assert memory.available_bytes>=4*1024**3 and memory.pressure.value!='critical','Memory headroom guard'
                report['memory'].append(dict(**probe,kv=kv,availableBytes=memory.available_bytes))
                print(json.dumps(dict(trial=trial_index,budgetMiB=budget,label=fixture['label'],cached=cached,input=prompt,firstMs=round(first,2))),flush=True)
            guard.set_owned([os.getpid()]);backend.stop();assert not backend.running
            report['identityAfter']=capture_identity(inspect_model(Path(model),backend='mlx_lm'),exe,'mlx_lm',detect_hardware());assert identity==report['identityAfter']
            assert hashlib.sha256(args.fixtures.read_bytes()).hexdigest()==fixture_hash and hashlib.sha256(Path(__file__).read_bytes()).hexdigest()==source_hash
            report['passed']=True
        except BaseException as error:
            report['error']=str(error);raise
        finally:
            if guard is not None:guard.stop()
            if backend is not None:backend.stop();report['shutdownClean']=not backend.running
            if counter is not None:
                counter.stdin.close()
                try:counter.wait(timeout=5)
                except subprocess.TimeoutExpired:counter.kill();counter.wait()
                counter.stdout.close();report['counterStopped']=counter.poll() is not None
            if counter_log is not None:counter_log.close()
            if pre is not None:
                try:os.killpg(pre.pid,signal.SIGTERM)
                except ProcessLookupError:pass
                try:pre.wait(timeout=5)
                except subprocess.TimeoutExpired:os.killpg(pre.pid,signal.SIGKILL);pre.wait()
            assert args.settings.read_bytes()==original
            save(stem.with_suffix('.json'),report);manifest['trials'].append(report);save(args.output_dir/'comparison.json',manifest)
    manifest['summary']=summarize(manifest['trials']);manifest['passed']=True;save(args.output_dir/'comparison.json',manifest)
    print(json.dumps(manifest['summary']),flush=True)
if __name__=='__main__':main()
