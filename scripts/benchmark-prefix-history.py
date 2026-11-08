"""Matched-history ABBA first-request pilot, with conservative live CPU guards.
Fresh means no application prefix state, not cold OS caches. No managed settings change.
"""
import argparse
import hashlib
import http.client
import json
import math
import os
from pathlib import Path
import secrets
import signal
import statistics
import subprocess
import time
import urllib.request
from host_load_guard import HostLoadGuard


def digest(path):
    result=hashlib.sha256()
    with Path(path).open('rb') as source:
        for chunk in iter(lambda:source.read(1024*1024),b''): result.update(chunk)
    return result.hexdigest()


def block_modes(round_index, order):
    if order not in {'ABBA', 'counterbalanced'}: raise ValueError('Unknown comparison order')
    return ['restore','fresh','fresh','restore'] if order=='counterbalanced' and round_index%2 else ['fresh','restore','restore','fresh']


def summarize(samples, fixtures, rounds, order='ABBA'):
    """Reject incomplete, reordered, contaminated or nonmatching comparison groups."""
    assert rounds>=2 and (order!='counterbalanced' or rounds%2==0),'Balanced order requires an even number of rounds'
    expected=[(label,round_index,slot,mode) for label in fixtures for round_index in range(rounds)
              for slot,mode in enumerate(block_modes(round_index,order))]
    assert [(s['label'],s['round'],s['slot'],s['mode']) for s in samples]==expected,'Incomplete or reordered ABBA data'
    for s in samples:
        assert all(math.isfinite(s[key]) and s[key]>0 for key in ['firstContentMs','completionMs']),'Invalid latency'
        assert s['exact'] is True and s['cacheVerified'] is True,'Answer/count/cache gate failed'
        assert all(math.isfinite(s['cpu'][k]) and s['cpu'][k]>=0 for k in ['averageBusyPercent','sustainedHighSeconds']),'Invalid CPU sample'
        assert math.isfinite(s['quietBeforePercent']) and s['quietBeforePercent']>=0,'Invalid pre-request sample'
        assert s['firstContentMs']<=s['completionMs'],'First content follows completion'
        assert s['cpu']['averageBusyPercent']<=50 and s['cpu']['sustainedHighSeconds']<1,'Contaminated CPU window'
        assert s['quietBeforePercent']<=20,'Host was not quiet before request'
    out={}
    for label in fixtures:
        rows=[s for s in samples if s['label']==label]
        out[label]={}
        for mode in ['fresh','restore']:
            selected=[s for s in rows if s['mode']==mode]
            out[label][mode]={'requests':len(selected),'firstContentMedianMs':statistics.median(s['firstContentMs'] for s in selected),
                'completionMedianMs':statistics.median(s['completionMs'] for s in selected),
                'firstContentRangeMs':[min(s['firstContentMs'] for s in selected),max(s['firstContentMs'] for s in selected)]}
        out[label]['restoreFirstContentChangePercent']=100*(out[label]['restore']['firstContentMedianMs']/out[label]['fresh']['firstContentMedianMs']-1)
        paired=[]
        for r in range(rounds):
            block=[s for s in rows if s['round']==r]
            medians={mode:statistics.median(s['firstContentMs'] for s in block if s['mode']==mode) for mode in ['fresh','restore']}
            paired.append({'round':r,'order':''.join('A' if mode=='fresh' else 'B' for mode in block_modes(r,order)),
                'freshMedianMs':medians['fresh'],'restoreMedianMs':medians['restore'],
                'restoreChangePercent':100*(medians['restore']/medians['fresh']-1)})
        out[label]['pairedRounds']=paired
        changes=[r['restoreChangePercent'] for r in paired]
        out[label]['pairedChangeMedianPercent']=statistics.median(changes)
        out[label]['pairedChangeRangePercent']=[min(changes),max(changes)]
    return out


class QuietHostError(RuntimeError):
    def __init__(self, samples, timeout):
        values=[s['busyPercent'] for s in samples]
        self.details={'timeoutSeconds':timeout,'readings':len(values),'limitPercent':20,
            'requiredConsecutiveReadings':3,'minimumPercent':min(values) if values else None,
            'maximumPercent':max(values) if values else None}
        super().__init__('Quiet host gate timed out; no comparison claim')


def quiet(guard,timeout=120,record=None):
    if record is None:record=[]
    start=time.monotonic();seen=None;stable=0
    while time.monotonic()-start<timeout:
        row=guard.snapshot()
        if row['at']!=seen:
            if record is not None:record.append({k:row[k] for k in ['busyPercent','globalBusyPercent','ownedBusyPercent']})
            stable=stable+1 if row['busyPercent']<=20 else 0
            seen=row['at']
            if stable>=3:return row['busyPercent']
        time.sleep(.5)
    raise QuietHostError(record,timeout)


def verify_qualification(python, runtime, model, store, root, evidence, env):
    """Full original startup verifier; no GPU allocation or evidence rewriting."""
    command=[str(python),str(runtime/'prefix-cache-http.py'),'--model',str(model),
        '--cache-root',str(root),'--quota-mib','256','--store-executable',str(store),
        '--evidence',str(evidence),'--verify-only']
    result=subprocess.run(command,env=env,capture_output=True,timeout=90)
    if result.returncode:raise RuntimeError('Existing qualification refused; no comparison claim')
    if len(result.stdout)>65536:raise RuntimeError('Oversized qualification verification response')
    value=json.loads(result.stdout)
    if value.get('verified') is not True or value.get('binding_schema')!=2:
        raise RuntimeError('Expected verified volume-binding v2 qualification')
    identity=value.get('identity')
    if not isinstance(identity,str) or len(identity)!=64 or any(c not in '0123456789abcdef' for c in identity):
        raise RuntimeError('Invalid verified qualification identity')
    return {'verified':True,'identity':identity,'binding_schema':2}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['python','model','store','fixtures','output-dir']:parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--labels',nargs='+',default=['en-5','ja-9'])
    parser.add_argument('--rounds',type=int,default=2)
    parser.add_argument('--order',choices=['ABBA','counterbalanced'],default='counterbalanced')
    parser.add_argument('--evidence',type=Path)
    parser.add_argument('--qualified-cache-root',type=Path)
    parser.add_argument('--port',type=int,default=18091)
    args=parser.parse_args()
    if bool(args.evidence)!=bool(args.qualified_cache_root):parser.error('Supply both --evidence and --qualified-cache-root')
    if not 2<=args.rounds<=5:parser.error('Use2–5 complete ABBA rounds per history')
    if args.order=='counterbalanced' and args.rounds%2:parser.error('Counterbalanced order requires2 or4 rounds')
    if len(set(args.labels))!=len(args.labels):parser.error('Fixture labels must be unique')
    source=json.loads(args.fixtures.read_text())
    fixtures={label:next(c for c in source['canonical'] if c['label']==label) for label in args.labels}
    assert source['result']=='passed' and source['fresh_text_parity']>=len(fixtures)
    args.output_dir=args.output_dir.resolve();args.output_dir.mkdir(mode=0o700,exist_ok=False)
    runtime=Path(__file__).resolve().parent
    artifacts=[Path(__file__).resolve(),runtime/'prefix-cache-http.py',runtime/'prefix_cache_contract.py',runtime/'prefix_cache_writes.py',runtime/'prefix_cache_roundtrip_support.py',runtime/'host_load_guard.py',args.store]
    hashes={p.name:digest(p) for p in artifacts}
    fixture_hash=digest(args.fixtures)
    cache=args.output_dir/'cache';cache.mkdir(mode=0o700)
    token=secrets.token_hex(24);headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'}
    env=dict(os.environ,SOLITON_PREFIX_HTTP_TOKEN=token)
    for key in list(env):
        if key.startswith(('MLX_','METAL_','VECLIB_','OMP_','TOKENIZERS_')) or key in {'PYTORCH_ENABLE_MPS_FALLBACK','PYTHONPATH','PYTHONHOME','PYTHONEXECUTABLE','__PYVENV_LAUNCHER__'}:env.pop(key,None)
    worker=None;log=None;guard=None;identity=None
    report={'result':'running','scope':'Matched-history first-request comparison; fresh+persist versus SSD restore; no physical-cold-cache or steady-state claim',
        'quietWindows':[],'order':args.order,'rounds':args.rounds,'labels':args.labels,'fixtureSha256':fixture_hash,'workerHashes':hashes,'samples':[],'seeds':{}}
    def save(): (args.output_dir/'comparison.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    def get(path='/studio/ssd-cache',body=None):
        request=urllib.request.Request(f'http://127.0.0.1:{args.port}'+path,headers=headers,data=None if body is None else json.dumps(body).encode())
        with urllib.request.urlopen(request,timeout=90) as reply:return json.load(reply)
    def stop():
        nonlocal worker,log
        if worker is not None:
            try:
                if guard is not None:guard.set_owned((os.getpid(),))
            finally:
                # CPU accounting failure must never prevent owned worker cleanup.
                try:
                    if worker.poll() is None:os.killpg(worker.pid,signal.SIGINT)
                    worker.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    os.killpg(worker.pid,signal.SIGKILL);worker.wait();raise RuntimeError('Unclean worker shutdown')
                finally:worker=None;log.close();log=None
    def start(label,measured=True):
        nonlocal worker,log,identity
        if measured:
            window={'phase':'launch','label':label,'samples':[]};report['quietWindows'].append(window)
            quiet(guard,record=window['samples'])
        log=(args.output_dir/(label+'.log')).open('x')
        worker=subprocess.Popen([str(args.python),str(runtime/'prefix-cache-http.py'),'--model',str(args.model),'--cache-root',str(cache),
            '--quota-mib','256','--store-executable',str(args.store),'--port',str(args.port),'--qualification-mode'],
            env=env,stdout=log,stderr=log,start_new_session=True)
        for _ in range(300):
            if worker.poll() is not None:raise RuntimeError('Worker exited: '+label)
            try:
                info=get()
                if identity is None:identity=info['identity']
                assert info['identity']==identity,'Runtime identity changed'
                guard.set_owned((os.getpid(),worker.pid));return
            except OSError:time.sleep(.2)
        raise RuntimeError('Worker startup timeout')
    def prepare(seed=None):
        store=cache/'store'
        if store.exists():
            assert not store.is_symlink()
            existing=list(store.iterdir())
            assert all(p.name=='.lock' or (p.name in {v['entry'] for v in report['seeds'].values()} and p.is_file() and not p.is_symlink()) for p in existing),'Unknown benchmark file; refusing cleanup'
            for path in existing:
                if path.name!='.lock':path.unlink()
        if seed is not None:
            store.mkdir(mode=0o700,exist_ok=True)
            path=store/seed['entry']
            with path.open('xb') as output,Path(seed['archive']).open('rb') as input_file:
                os.chmod(path,0o600)
                for chunk in iter(lambda:input_file.read(1024*1024),b''):output.write(chunk)
                output.flush();os.fsync(output.fileno())
    def infer(query):
        connection=http.client.HTTPConnection('127.0.0.1',args.port,timeout=90)
        begin=time.perf_counter();first=None;text='';usage=None;finish=None
        try:
            connection.request('POST','/v1/chat/completions',json.dumps(dict(query,stream=True,stream_options={'include_usage':True})),headers)
            response=connection.getresponse()
            assert response.status==200,'HTTP '+str(response.status)
            for raw in response:
                if not raw.startswith(b'data:'):continue
                data=raw[5:].strip()
                if data==b'[DONE]':break
                frame=json.loads(data);choices=frame.get('choices') or []
                if choices:
                    content=choices[0].get('delta',{}).get('content','')
                    if content:
                        if first is None:first=time.perf_counter()-begin
                        text+=content
                    finish=choices[0].get('finish_reason') or finish
                usage=frame.get('usage') or usage
            elapsed=time.perf_counter()-begin
        finally:connection.close()
        assert first is not None and usage and finish=='stop','Missing/truncated output'
        return text,usage,first*1000,elapsed*1000
    try:
        report['phase']='qualification_before'
        if args.evidence:
            report['qualificationBefore']=verify_qualification(args.python,runtime,args.model,args.store,args.qualified_cache_root,args.evidence,env)
        report['phase']='host_preflight'
        guard=HostLoadGuard()
        window={'phase':'host_preflight','samples':[]};report['quietWindows'].append(window)
        quiet(guard,record=window['samples'])
        save();print(json.dumps({'phase':'host-preflight-passed','modelLoaded':False}),flush=True)
        report['phase']='seed_preparation'
        # Seed each selected state once; these are excluded from measurement.
        for label,fixture in fixtures.items():
            prepare();start('seed-'+label,measured=False)
            counted=get('/tokenize',fixture['query'])['count']
            text,usage,_,_=infer(fixture['query']);assert text==fixture['answer']
            assert usage['prompt_tokens']==counted and get()['writes']==1
            stop()
            entries=list((cache/'store').glob('*.cache'));assert len(entries)==1
            archive=args.output_dir/(label+'.state')
            with archive.open('xb') as dest,entries[0].open('rb') as src:
                os.chmod(archive,0o600)
                for chunk in iter(lambda:src.read(1024*1024),b''):dest.write(chunk)
            report['seeds'][label]={'entry':entries[0].name,'archive':str(archive),'sha256':digest(archive),'bytes':archive.stat().st_size,'inputTokens':counted}
            # Unmeasured functional preparation: independent cold process and exact SSD restore.
            start('seed-restore-'+label,measured=False)
            restored,restored_usage,_,_=infer(fixture['query'])
            assert restored==fixture['answer'] and restored_usage['prompt_tokens']==counted
            assert restored_usage['prompt_tokens_details']['cached_tokens']==counted-1
            assert get()['hits']==1 and get()['writes']==0
            stop()
            assert digest(entries[0])==report['seeds'][label]['sha256']
            report['seeds'][label]['restoreVerified']=True
            save();print(json.dumps({'phase':'seeded-and-restored','label':label,'tokens':counted,'measured':False}),flush=True)
        for label,fixture in fixtures.items():
            seed=report['seeds'][label]
            report['phase']='measurement'
            for round_index in range(args.rounds):
                for slot,mode in enumerate(block_modes(round_index,args.order)):
                    prepare(seed if mode=='restore' else None)
                    start(f'{label}-{round_index}-{slot}-{mode}')
                    # Explicitly warm the same serialized bytes before both modes. Only B has a committed entry.
                    warm=cache/'store'/seed['entry'] if mode=='restore' else Path(seed['archive'])
                    assert digest(warm)==seed['sha256'],'Seed bytes changed'
                    window={'phase':'request','label':label,'round':round_index,'slot':slot,'samples':[]};report['quietWindows'].append(window)
                    before_cpu=quiet(guard,30,window['samples'])
                    before=get();mark=guard.mark()
                    text,usage,first,elapsed=infer(fixture['query'])
                    aftermark=guard.mark();cpu=guard.interval(mark,aftermark);after=get()
                    expected_cached=0 if mode=='fresh' else seed['inputTokens']-1
                    assert usage['prompt_tokens']==seed['inputTokens'] and usage['prompt_tokens_details']['cached_tokens']==expected_cached
                    delta={key:after[key]-before[key] for key in ['hits','misses','writes','errors','skippedWrites','serializedBytes','committedBytes']}
                    assert text==fixture['answer'] and delta['errors']==0
                    assert delta['writes']==(1 if mode=='fresh' else 0) and delta['hits']==(1 if mode=='restore' else 0)
                    assert cpu['averageBusyPercent']<=50 and cpu['sustainedHighSeconds']<1,'Background CPU contamination; entire pilot rejected'
                    row={'label':label,'round':round_index,'slot':slot,'mode':mode,'firstContentMs':first,'completionMs':elapsed,
                        'usage':usage,'counters':delta,'cpu':cpu,'quietBeforePercent':before_cpu,'exact':True,'cacheVerified':True}
                    report['samples'].append(row);save();stop()
                    print(json.dumps({'label':label,'round':round_index,'slot':slot,'mode':mode,'firstMs':round(first,1)}),flush=True)
        assert {p.name:digest(p) for p in artifacts}==hashes and digest(args.fixtures)==fixture_hash,'Inputs changed'
        report['phase']='qualification_after'
        if args.evidence:
            report['qualificationAfter']=verify_qualification(args.python,runtime,args.model,args.store,args.qualified_cache_root,args.evidence,env)
            assert report['qualificationAfter']==report['qualificationBefore'],'Qualification changed during comparison'
        report.update(result='passed',summary=summarize(report['samples'],args.labels,args.rounds,args.order),identity=identity,
            limits=[f'one Mac/{len(fixtures)} histories/{args.rounds*2} samples per mode per history; paired rounds are descriptive, no confidence interval or population inference','warm-read OS page cache, no purge or physical I/O measurement','includes fresh serialization/commit versus verified restoration, and first-forward/JIT costs','CPU guard excludes controller/worker; transient Rust helpers remain conservatively counted as background','no GPU load/thermal/page-residency guard; not a general speedup certification','no new qualification or ordinary settings change'])
        save();print(json.dumps(report['summary']),flush=True)
    except BaseException as error:
        report['failureStage']=report.get('phase')
        if isinstance(error,QuietHostError):report['quietFailure']=error.details
        report.update(result='rejected',error=type(error).__name__+': '+str(error));save();raise
    finally:
        try:stop()
        finally:
            if guard is not None:guard.stop()


if __name__=='__main__':main()
