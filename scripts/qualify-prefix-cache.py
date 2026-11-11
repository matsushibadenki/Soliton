"""Generate private, identity-bound 30-minute MLX SSD qualification evidence.
No evidence is issued by --preflight. Existing production app state is never used.
"""
import argparse
import http.client
import json
import os
from pathlib import Path
import secrets
import signal
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
from prefix_cache_contract import PROFILE, REQUIRED_GATES, verify_evidence


def annotate_http_error(error):
    # HTTPError.reason is read-only; msg preserves the status code and is supported on3.10+.
    detail=error.read(1024).decode('utf-8',errors='replace')
    error.msg=f'{error.reason}: {detail}'
    return error


def publish_evidence(output, report, identity, quota):
    """Validate a complete private report before atomically publishing it."""
    with tempfile.TemporaryDirectory(prefix='.ssd-evidence-', dir=output.parent) as staging:
        candidate = Path(staging) / 'evidence.json'
        fd = os.open(candidate, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
        with os.fdopen(fd, 'w') as file:
            json.dump(report, file, indent=2)
            file.flush()
            os.fsync(file.fileno())
        verify_evidence(candidate, identity, quota)
        os.link(candidate, output)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--python',required=True,type=Path)
    parser.add_argument('--model',required=True,type=Path)
    parser.add_argument('--cache-root',required=True,type=Path)
    parser.add_argument('--store-executable',required=True,type=Path)
    parser.add_argument('--output',required=True,type=Path)
    parser.add_argument('--quota-mib',type=int,default=256)
    parser.add_argument('--port',type=int,default=18082)
    parser.add_argument('--preflight',action='store_true')
    parser.add_argument('--require-eviction',action='store_true',help='Require real pressure eviction, retained restart hit and evicted-prefix recomputation')
    args=parser.parse_args()
    if args.require_eviction and not 64 <= args.quota_mib <= 256: parser.error('Eviction pressure fixture requires 64–256 MiB quota')
    if args.output.exists(): parser.error('Evidence output already exists; choose a new file')
    try:
        from transformers import AutoTokenizer
    except ImportError as error:
        parser.error(f'Run the controller with the selected MLX Python and its Transformers dependency: {error}')
    args.cache_root.mkdir(mode=0o700,exist_ok=True)
    root=Path(__file__).resolve().parent
    token=secrets.token_hex(24)
    headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'}
    env=dict(os.environ,SOLITON_PREFIX_HTTP_TOKEN=token)
    for key in list(env):
        if key.startswith(('MLX_', 'METAL_', 'VECLIB_', 'OMP_', 'TOKENIZERS_')) or key in {'PYTORCH_ENABLE_MPS_FALLBACK','PYTHONPATH','PYTHONHOME','PYTHONEXECUTABLE','__PYVENV_LAUNCHER__'}:env.pop(key,None)
    log_path=args.output.with_suffix('.log')
    log_fd=os.open(log_path,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
    logfile=os.fdopen(log_fd,'w')
    worker=None
    successes=0
    def request(path='/studio/ssd-cache',body=None,authenticate=True):
        request=urllib.request.Request(f'http://127.0.0.1:{args.port}'+path,
            headers=headers if authenticate else {},data=None if body is None else json.dumps(body).encode())
        try:
            with urllib.request.urlopen(request,timeout=60) as reply:return json.load(reply)
        except urllib.error.HTTPError as error:
            # Preserve bounded server diagnostics without retrying or forgiving any failure.
            annotate_http_error(error)
            raise
    def start():
        nonlocal worker
        worker=subprocess.Popen([str(args.python),str(root/'prefix-cache-http.py'),'--model',str(args.model),
            '--cache-root',str(args.cache_root),'--quota-mib',str(args.quota_mib),'--store-executable',str(args.store_executable),
            '--port',str(args.port),'--qualification-mode'],env=env,stdout=logfile,stderr=logfile,start_new_session=True)
        for _ in range(200):
            if worker.poll() is not None:raise RuntimeError('Qualification worker failed; inspect private log')
            try:
                info=request('/health')
                if info.get('inference_ready') is True:return request()
            except (OSError,ValueError):pass
            time.sleep(.3)
        raise RuntimeError('Qualification worker readiness timeout')
    def stop():
        nonlocal worker
        if worker is None:return
        try:
            if worker.poll() is None:os.killpg(worker.pid,signal.SIGINT)
            worker.wait(timeout=15)
        except subprocess.TimeoutExpired:
            os.killpg(worker.pid,signal.SIGKILL);worker.wait();raise RuntimeError('Worker did not shut down cleanly')
        finally:worker=None
    prompt='The fictional code word is soliton. '*20+'What is the code word? Answer briefly.'
    body=dict(model='default_model',messages=[dict(role='user',content=prompt)],temperature=0,seed=0,max_tokens=16)
    arithmetic=dict(body,messages=[dict(role='user',content='What is 2 + 2? Answer with only the number.')])
    def answer(query,expected=None):
        nonlocal successes
        result=request('/v1/chat/completions',query)
        text=result['choices'][0]['message']['content']
        if expected is not None and text!=expected:raise RuntimeError('Qualification answer mismatch')
        successes+=1
        return text,result
    def reject(query,status=400):
        try:request('/v1/chat/completions',query)
        except urllib.error.HTTPError as error:
            if error.code==status:return
            raise
        raise RuntimeError('Qualification request rejection failed')
    gates={}
    eviction_pressure=None
    try:
        original=start();identity=original['identity']
        try:request(authenticate=False);raise RuntimeError('Unauthenticated request was accepted')
        except urllib.error.HTTPError as error:assert error.code==401
        gates['authentication']=True
        for invalid in [dict(body,stream='yes'),dict(body,model='other'),dict(body,temperature=.7),dict(body,tools=[])]:reject(invalid)
        gates['request_rejection']=True
        baseline,_=answer(body)
        if 'soliton' not in baseline.lower():raise RuntimeError('Cold answer is incorrect')
        if request()['writes']<1:raise RuntimeError('Cold cache was not saved')
        gates['cold']=True
        stop();warm=start()
        if warm['identity']!=identity:raise RuntimeError('Runtime identity changed across restart')
        _,restored=answer(body,baseline)
        if request()['hits']!=1 or restored['usage']['prompt_tokens_details']['cached_tokens']!=177:raise RuntimeError('Restart did not restore the expected prefix')
        gates['restart_hit']=True
        stop()
        # Corrupt only an owned cache whose filename was captured from this run.
        caches=list((args.cache_root/'store').glob('*.cache'))
        if len(caches)!=1:raise RuntimeError('Use a fresh qualification cache root')
        with caches[0].open('r+b') as file:
            file.seek(4104);byte=file.read(1);file.seek(4104);file.write(bytes([byte[0]^1]))
        start();answer(body,baseline)
        if request()['hits']!=0 or request()['misses']!=1:raise RuntimeError('Corruption fallback failed')
        gates['corruption_fallback']=True
        prior=request()
        for _ in range(5):answer(body,baseline)
        repeated=request()
        if repeated['writes']!=prior['writes'] or repeated['serializedBytes']!=prior['serializedBytes'] or repeated['skippedWrites']<prior['skippedWrites']+5:
            raise RuntimeError('Duplicate write suppression failed')
        for damage in ['corrupt','delete']:
            if damage=='delete':caches[0].unlink()
            else:
                with caches[0].open('r+b') as file:
                    file.seek(4104);byte=file.read(1);file.seek(4104);file.write(bytes([byte[0]^1]))
            prior=request();answer(body,baseline)
            if request()['writes']!=prior['writes']+1:raise RuntimeError('Changed committed entry was not repaired')
        gates['duplicate_write_suppression']=True
        gates['live_entry_repair']=True
        connection=http.client.HTTPConnection('127.0.0.1',args.port,timeout=30)
        connection.request('POST','/v1/chat/completions',json.dumps(dict(body,stream=True,max_tokens=128)),headers)
        response=connection.getresponse();assert response.status==200;response.read(32);connection.close();time.sleep(1)
        answer(body,baseline);gates['disconnect_recovery']=True
        stop()
        quota_entries_before={p.name for p in (args.cache_root/'store').glob('*.cache')}
        filler=args.cache_root/'store'/'quota-fixture.cache'
        with filler.open('xb') as file:file.truncate(args.quota_mib*1024**2)
        try:
            start();altered=dict(body,messages=[dict(role='user',content=prompt+' Use a short sentence.')])
            text,_=answer(altered)
            stats=request()
            if 'soliton' not in text.lower() or stats['writes']!=0 or stats['errors']<1:raise RuntimeError('Quota-full fallback failed')
            if {p.name for p in (args.cache_root/'store').glob('*.cache')} != quota_entries_before|{filler.name}:raise RuntimeError('Protected quota failure evicted existing states')
            gates['quota_fallback']=True
            gates['protected_quota_preservation']=True
        finally:stop();filler.unlink()
        # Failed writes are not memoized; releasing quota allows a later save.
        start();answer(altered)
        if request()['writes']!=1:raise RuntimeError('Write retry after quota recovery failed')
        answer(altered)
        if request()['writes']!=1 or request()['skippedWrites']<1:raise RuntimeError('Retry result was not deduplicated')
        gates['quota_retry']=True
        stop()
        start()
        # Use the same installed tokenizer/template without loading a second model.
        tokenizer=AutoTokenizer.from_pretrained(str(args.model),local_files_only=True,trust_remote_code=False)
        boundary=None
        for n in range(512):
            for suffix in ['', '.', ' x', '!']:
                messages=[dict(role='user',content='Say soliton. '+'a '*n+suffix)]
                count=len(tokenizer.apply_chat_template(messages,tokenize=True,add_generation_prompt=True,return_dict=False))
                if count==496:boundary=dict(body,messages=messages);break
            if boundary is not None:break
        if boundary is None:raise RuntimeError('Could not construct exact context boundary')
        counted=request('/tokenize',boundary)
        if counted['count']!=496:raise RuntimeError('Tokenizer boundary disagreement')
        answer(boundary)
        reject(dict(boundary,max_tokens=17),422)
        gates['context_boundary']=True
        arithmetic_answer,_=answer(arithmetic)
        if arithmetic_answer.strip()!='4':raise RuntimeError('Arithmetic fixture failed')
        if args.require_eviction:
            protected=args.cache_root/'store'/'eviction-protected.txt'
            protected.write_text('Keep unknown qualification data')
            pressure=[];pressure_samples=[];removed=set();first_entry=None;previous={p.name for p in (args.cache_root/'store').glob('*.cache')}
            before_pressure=request()
            # 22 distinct ~19MiB states exceed both 128MiB and 256MiB qualification stores.
            for index in range(22):
                query=dict(body,messages=[dict(role='user',content=f'Pressure document {index}. '+prompt)])
                text,result=answer(query)
                if 'soliton' not in text.lower():raise RuntimeError('Eviction pressure answer failed')
                current={p.name for p in (args.cache_root/'store').glob('*.cache')}
                if index==0:
                    created=current-previous
                    if len(created)!=1:raise RuntimeError('First pressure prefix was not saved')
                    first_entry=next(iter(created))
                removed.update(previous-current)
                removed_now=len(previous-current)
                used=sum(p.stat().st_size for p in (args.cache_root/'store').iterdir() if p.name!='.lock')
                if used>args.quota_mib*1024**2:raise RuntimeError('Eviction exceeded logical store quota')
                if protected.read_text()!='Keep unknown qualification data':raise RuntimeError('Eviction removed protected data')
                pressure_samples.append(dict(index=index,used_bytes=used,entries=len(current),removed_entries=removed_now))
                pressure.append((query,text));previous=current
            after_pressure=request()
            if not removed or first_entry in previous or after_pressure['writes']-before_pressure['writes']!=22 or after_pressure['errors']!=before_pressure['errors']:
                raise RuntimeError('Automatic eviction pressure gates failed')
            gates['automatic_eviction']=True
            stop();restarted=start()
            if restarted['identity']!=identity:raise RuntimeError('Eviction restart identity changed')
            _,retained=answer(*pressure[-1])
            if request()['hits']!=1 or retained['usage']['prompt_tokens_details']['cached_tokens']<=0:raise RuntimeError('Retained eviction state did not restore')
            # A second restart removes partial RAM prefix reuse from the cold-miss assertion.
            stop();restarted=start()
            if restarted['identity']!=identity:raise RuntimeError('Cold recompute identity changed')
            answer(*pressure[0])
            if request()['misses']!=1 or request()['writes']!=1:raise RuntimeError('Evicted prefix was not recomputed and saved')
            answer(*pressure[0])
            if request()['skippedWrites']<1:raise RuntimeError('Recomputed prefix was not deduplicated')
            if protected.read_text()!='Keep unknown qualification data':raise RuntimeError('Protected eviction data changed')
            protected.unlink()
            gates['eviction_restart_recompute']=True
            eviction_pressure=dict(requests=22,saved_writes=after_pressure['writes']-before_pressure['writes'],storage_errors=after_pressure['errors']-before_pressure['errors'],removed_entries=len(removed),samples=pressure_samples,restart_counters=request())
        for i in range(10):answer(arithmetic if i%2 else body,arithmetic_answer if i%2 else baseline)
        def rss():
            if worker.poll() is not None:raise RuntimeError('Worker exited during qualification')
            return int(subprocess.check_output(['/bin/ps','-o','rss=','-p',str(worker.pid)],text=True).strip())*1024
        base=rss();peak=base
        duration=10 if args.preflight else 1800
        started=time.monotonic();last_progress=started;last_sample=started
        print(json.dumps(dict(status='soaking',identity=identity,gates=gates,duration=duration)),flush=True)
        while time.monotonic()-started<duration:
            index=successes%10
            answer(arithmetic if index==0 else body,arithmetic_answer if index==0 else baseline)
            now=time.monotonic()
            if now-last_sample>=5:peak=max(peak,rss());last_sample=now
            if now-last_progress>=30:
                progress=dict(status='soaking',elapsed_seconds=round(now-started,1),successes=successes,rss_growth_bytes=max(0,peak-base))
                args.output.with_suffix('.progress.json').write_text(json.dumps(progress));print(json.dumps(progress),flush=True);last_progress=now
            if peak-base>128*1024**2:raise RuntimeError('Qualification RSS growth exceeded 128 MiB')
        elapsed=time.monotonic()-started
        final=request()
        if final['identity']!=identity:raise RuntimeError('Runtime identity changed')
        stop()
        # Recompute all content identities after the soak, without model allocation.
        checked=subprocess.run([str(args.python),str(root/'prefix-cache-http.py'),'--model',str(args.model),
            '--cache-root',str(args.cache_root),'--quota-mib',str(args.quota_mib),'--store-executable',str(args.store_executable),
            '--qualification-mode','--verify-only'],env=env,capture_output=True,check=True,timeout=60)
        final_binding=json.loads(checked.stdout)
        if final_binding['identity']!=identity or final_binding['cache_binding']!=original['cache_binding']:raise RuntimeError('Model/runtime/volume changed during qualification')
        report=dict(result='preflight' if args.preflight else 'passed',identity=identity,profile=PROFILE,binding_schema=original['binding_schema'],cache_binding=original['cache_binding'],cache_device_observed=original['cache_device_observed'],cache_device_observed_end=final_binding['cache_device_observed'],
            quota_bytes=args.quota_mib*1024**2,created_at=time.time(),duration_seconds=elapsed,successes=successes,
            failures=0,requires_eviction=args.require_eviction,eviction_pressure=eviction_pressure,rss_growth_bytes=max(0,peak-base),gates=gates,final_counters=final)
        if not args.preflight:
            publish_evidence(args.output, report, identity, args.quota_mib * 1024**2)
        print(json.dumps(report),flush=True)
    finally:
        stop();logfile.close()


if __name__=='__main__':main()
