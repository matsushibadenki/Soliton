"""Qualification-only active-memory/prefill comparison. Never issues/edits startup evidence.

Run with frontend Python, from the Soliton root: SCRIPT BUDGET NEW_REPORT.
Original candidate preflight, model/context/quality probes and a short soak run
first. A fresh supervised backend then runs identical exact-count workloads.
A selected profile still requires full 1800-second qualification + Soliton tests.
"""
import hashlib
import json
import os
from pathlib import Path
import select
import signal
import statistics
import subprocess
import sys
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
UP = ROOT / 'vLLM-apple'
sys.path.insert(0, str(UP))
from vllm_apple.architecture_evidence import capture_identity
from vllm_apple.backend import BackendConfig, BackendProcess
from vllm_apple.hardware import detect_hardware, detect_memory
from vllm_apple.model import inspect_model
from host_load_guard import HostLoadGuard

if not __debug__:
    raise RuntimeError('validation assertions required')
budget, output = int(sys.argv[1]), Path(sys.argv[2]).absolute()
micro = '--micro' in sys.argv[3:]
assert budget == 512 and not output.exists()
prefill_values=[arg.split('=',1)[1] for arg in sys.argv[3:] if arg.startswith('--prefill-step-size=')]
assert len(prefill_values)==1 and prefill_values[0] in {'512','1024','2048'}
prefill_step=int(prefill_values[0])
source = (ROOT / '.studio-state.json').read_bytes()
settings = json.loads(source)['settings']['vllm']
model = settings['model']
exe = Path(settings['executable'])
assert settings['context']==4096 and settings['kvCacheMiB']==1024
os.environ['VLLM_APPLE_KV_CACHE_MIB']='1024'
os.environ.pop('VLLM_APPLE_ALLOCATOR_CACHE_MIB',None)
if budget: os.environ['VLLM_APPLE_ALLOCATOR_CACHE_MIB']=str(budget)
os.environ['PYTORCH_ENABLE_MPS_FALLBACK']='1'
backend = counter = guard = None
workload_hash=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
fixtures_hash=hashlib.sha256((ROOT/'.soliton-runtime/allocator-fixtures-20261028.json').read_bytes()).hexdigest()
guard_hash=hashlib.sha256((ROOT/'scripts/host_load_guard.py').read_bytes()).hexdigest()
report = dict(hostGuardMode='liveBackgroundCPU' if micro else 'loadAverage',hostGuardSha256=guard_hash,workloadKind='micro' if micro else 'diverse-history',workloadSha256=workload_hash,fixturesSha256=fixtures_hash,passed=False, scope='experimental prefill arguments after original model/compatibility preflight; not startup evidence or approved profile',
              experimentKind='prefill-step-active-memory', prefillStepSize=prefill_step, certification=False, allocatorCacheMiB=budget, kvCacheMiB=1024, context=4096,
              startedAt=time.time(), loadAtStart=os.getloadavg(), cases=[], samples=[])

def processes():
    return [line.strip().split(None,2) for line in subprocess.check_output(['/bin/ps','-axo','pid=,ppid=,command='],text=True).splitlines() if line.strip()]

def exclusive():
    for pid,ppid,command in processes():
        first=Path(command.split()[0]).name
        assert first not in {'soliton','studio-server'}, 'close application owners for an isolated trial'
        assert not (command.startswith('/opt/') and ('/mlx_kv_budget.py' in command or 'mlx_lm.server --model ' in command)), 'another model backend exists'

def descendants(pid):
    rows=processes();owned={pid}
    for _ in range(30):
        add={int(p) for p,parent,_ in rows if int(parent) in owned}
        if add<=owned:break
        owned|=add
    return sorted(owned)

def request_json(path,body=None):
    payload=None if body is None else json.dumps(body).encode()
    req=urllib.request.Request(backend.base_url+path,data=payload,headers={'Content-Type':'application/json'})
    with urllib.request.urlopen(req,timeout=180) as response:
        raw=response.read(1024*1024+1);assert len(raw)<=1024*1024
        return json.loads(raw)

ident=0
counter_log=None

def count(messages):
    global ident
    assert counter.poll() is None,'counter failed'
    ident+=1
    frame=json.dumps({'id':ident,'request':{'messages':messages}}).encode()+b'\n'
    assert len(frame)<=2*1024*1024
    counter.stdin.write(frame);counter.stdin.flush()
    assert select.select([counter.stdout],[],[],30)[0], 'counter timed out'
    raw=counter.stdout.readline(4097);assert len(raw)<=4096 and raw.endswith(b'\n')
    result=json.loads(raw);assert result['id']==ident and type(result['prompt_tokens']) is int
    return result['prompt_tokens']

def chat(messages,expected,phase,index,seed=None):
    host_wait=guard.wait_quiet() if micro and phase!='prime' else 0
    load_before=os.getloadavg()[0]
    cpu_before=guard.mark() if micro else None
    if micro and phase!='prime':assert cpu_before['busyPercent']<=50, 'background CPU busy before request'
    elif not micro:assert load_before<=10, 'host load before request invalidates trial'
    started=time.monotonic();prompt=count(messages);assert prompt+64<=4096
    body=dict(model='default_model',messages=messages,max_tokens=64,temperature=0,stream=False)
    if seed is not None:body['seed']=seed
    value=request_json('/v1/chat/completions',body)
    elapsed=(time.monotonic()-started)*1000
    load_after=os.getloadavg()[0]
    cpu_after=guard.mark() if micro else None
    window=guard.interval(cpu_before,cpu_after) if micro else None
    cpu_max=window['peakBusyPercent'] if micro else None
    if micro and phase!='prime':assert window['averageBusyPercent']<=50 and window['sustainedHighSeconds']<1, 'background CPU busy during request'
    elif not micro:assert load_after<=10, 'host load during request invalidates trial'
    assert value['choices'][0]['message']['content'].strip()==expected,'incorrect model response'
    assert value['usage']['prompt_tokens']==prompt,'local/backend exact count mismatch'
    cached=value['usage']['prompt_tokens_details']['cached_tokens']
    assert type(cached) is int and 0<=cached<=prompt
    row=dict(hostWaitMs=host_wait,cpuAveragePercent=window['averageBusyPercent'] if micro else None,cpuSustainedHighSeconds=window['sustainedHighSeconds'] if micro else None,cpuMaxPercent=cpu_max,loadBefore=load_before,loadAfter=load_after,phase=phase,index=index,exact=True,promptTokens=prompt,cachedTokens=cached,elapsedMs=elapsed)
    report['cases'].append(row)
    return row

def sample(phase):
    cpu_wait=guard.wait_quiet(limit=50) if micro else 0
    for attempt in range(5):
        ids=descendants(backend.pid)
        try:
            value=json.loads(subprocess.check_output([str(ROOT/'.soliton-runtime/physical-footprint-20261028'),*map(str,ids)],stderr=subprocess.DEVNULL))
        except subprocess.CalledProcessError:
            value=None
        if value and ids==descendants(backend.pid):break
        time.sleep(.05)
    else:raise RuntimeError('owned memory sample did not stabilize')
    assert value['physicalBytes']>0 and value['rssBytes']>0 and value['processCount']==len(ids)
    kv=request_json('/vllm-apple/kv-cache')
    assert kv['max_bytes']==1024**3 and 0<=kv['retained_bytes']<=1024**3
    assert kv['allocator']['configured_limit_bytes']==(budget*1024**2 if budget else None)
    if budget:assert 0<=kv['allocator']['effective_limit_bytes']<=budget*1024**2
    memory=detect_memory();assert memory.available_bytes>1024**3 and memory.pressure.value!='critical'
    if micro:assert guard.snapshot()['busyPercent']<=50, 'host CPU busy at memory sample'
    else:assert os.getloadavg()[0]<=10, 'host load episode: stop rather than attribute latency to budget'
    report['samples'].append(dict(hostWaitMs=cpu_wait,cpuBusyPercent=guard.snapshot()['busyPercent'] if micro else None,phase=phase,**value,kvCache=kv,availableBytes=memory.available_bytes,load=os.getloadavg(),sampleRetries=attempt))

# SIGINT/SIGTERM cannot leave an experimental model resident.
def interrupted(signum,frame):raise KeyboardInterrupt('trial interrupted')
signal.signal(signal.SIGTERM,interrupted)
signal.signal(signal.SIGINT,interrupted)
try:
    exclusive()
    if micro:
        guard=HostLoadGuard()
        # The driver observes a quiet host before launch; frontend import/model
        # load is priming, not a latency observation.
    else:assert os.getloadavg()[0]<=10, 'host load too high to start a latency trial'
    preflight=output.with_name(output.stem+'-preflight.json')
    assert not preflight.exists()
    command=[settings['python'],'-m','vllm_apple','qualify-model',model,'--backend-kind','mlx_lm','--backend-executable',str(exe),'--candidate-mlx-lm-version','0.32.0','--max-model-len','4096','--concurrency','1','--duration','2','--warmup','1','--phase-samples','3','--allow-short-run','--backend-port','19231','--output',str(preflight)]
    with output.with_suffix('.preflight.log').open('x') as log:
        # A private session owns the preflight and all its backend descendants.
        # Interrupt/timeout cleanup must not kill only the qualifier parent.
        preflight_process=subprocess.Popen(command,cwd=UP,env=os.environ,stdout=log,stderr=log,start_new_session=True)
        try:
            assert preflight_process.wait(timeout=240)==0, 'qualification preflight failed'
        finally:
            try:os.killpg(preflight_process.pid,signal.SIGTERM)
            except ProcessLookupError:pass
            try:preflight_process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                try:os.killpg(preflight_process.pid,signal.SIGKILL)
                except ProcessLookupError:pass
                preflight_process.wait()

    proof=json.loads(preflight.read_bytes())
    assert proof['passed'] and proof['shutdown_clean'] and not proof['soak']['certification_required']
    assert 'architecture_evidence' not in proof
    report['preflightSha256']=hashlib.sha256(preflight.read_bytes()).hexdigest()
    exclusive()
    hardware=detect_hardware();inspected=inspect_model(Path(model),backend='mlx_lm')
    before=capture_identity(inspected,exe,'mlx_lm',hardware)
    backend=BackendProcess(BackendConfig(model,exe,port=19231,max_model_len=4096,backend_kind='mlx_lm',extra_arguments=('--prefill-step-size',str(prefill_step))))
    backend.start()
    fixtures=json.loads((ROOT/'.soliton-runtime/allocator-fixtures-20261028.json').read_bytes())
    counter_log=output.with_suffix('.counter.log').open('xb')
    counter=subprocess.Popen([str(exe.parent/'python'),'-I','-u',str(ROOT/'crates/studio-core/src/token_budget.py'),model,'--soliton-token-counter'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=counter_log)
    short=fixtures['short'];histories=fixtures['long']
    if micro:
        guard.set_owned([os.getpid(),backend.pid,counter.pid])
        for _ in range(80):
            if guard.snapshot()['busyPercent']<=40:break
            time.sleep(.25)
        else:raise RuntimeError('host CPU did not settle after model load')
    # First round primes model/counter. Remaining identical requests are warm.
    for round in range(4):
        for index,case in enumerate(short):chat(case['messages'],case['expected'],'prime' if round==0 else 'warm-before',index)
    sample('before-churn')
    first=chat(histories[0]['messages'],'STRATA-7391','anchor-cold',0);sample('anchor-cold')
    hot=chat(histories[0]['messages'],'STRATA-7391','anchor-hot',0);sample('anchor-hot')
    assert hot['cachedTokens']>=hot['promptTokens']-64
    for index in range(1,3 if micro else 16):
        chat(histories[index]['messages'],'STRATA-7391','churn',index);sample(f'churn-{index}')
        print(f'pilot {budget}: history {index}/{2 if micro else 15}',flush=True)
    miss=chat(histories[0]['messages'],'STRATA-7391','revisit',0);sample('revisit')
    assert miss['cachedTokens']<hot['cachedTokens']/2
    hit=chat(histories[0]['messages'],'STRATA-7391','reheat',0);sample('reheat')
    assert hit['cachedTokens']>=hit['promptTokens']-64
    recent_index=2 if micro else 15
    chat(histories[recent_index]['messages'],'STRATA-7391','recent',recent_index);sample('recent')
    for index in [1,2,3]:chat(histories[index]['messages'],'STRATA-7391','seeded',index,42);sample(f'seeded-{index}')
    for round in range(3):
        for index,case in enumerate(short):chat(case['messages'],case['expected'],'warm-after',index)
    sample('after-churn');time.sleep(10);sample('idle10')
    # Local exact refusal is recorded only as a pilot admission guard, not a
    # claim about Soliton HTTP: actual 4096/4097 HTTP checks run after certification.
    prompt=count(histories[0]['messages']);assert prompt<4096
    report['exactLocalReservation']=dict(accepted=prompt+(4096-prompt),rejected=prompt+(4097-prompt),inferenceOnOverflow=False)
    if guard is not None:guard.stop()
    backend.stop();assert not backend.running
    report['identityBefore']=before;report['identityAfter']=capture_identity(inspect_model(Path(model),backend='mlx_lm'),exe,'mlx_lm',detect_hardware())
    assert report['identityAfter']==before
    assert hashlib.sha256((ROOT/'scripts/host_load_guard.py').read_bytes()).hexdigest()==guard_hash
    assert hashlib.sha256(Path(__file__).read_bytes()).hexdigest()==workload_hash
    assert hashlib.sha256((ROOT/'.soliton-runtime/allocator-fixtures-20261028.json').read_bytes()).hexdigest()==fixtures_hash
    report['passed']=True
finally:
    if guard is not None:guard.stop()
    if backend is not None:backend.stop();report['shutdownClean']=not backend.running
    if counter is not None:
        counter.stdin.close()
        try:counter.wait(timeout=5)
        except subprocess.TimeoutExpired:counter.kill();counter.wait()
        counter.stdout.close();report['counterStopped']=counter.poll() is not None
    if counter_log is not None:counter_log.close()
    if guard is not None:
        guard.stop();report['cpuTrace']=guard.samples
    report['finishedAt']=time.time();report['loadAtEnd']=os.getloadavg()
    assert (ROOT/'.studio-state.json').read_bytes()==source
    report['userStatePreserved']=True
    with output.open('x') as file:
        os.chmod(output,0o600);json.dump(report,file,indent=2);file.write('\n')
exclusive()
print(json.dumps(dict(passed=report['passed'],budget=budget,cases=len(report['cases']),samples=len(report['samples']))),flush=True)
