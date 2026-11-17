"""Native Realtime protocol probe. Never enables the Soliton speech route."""
from pathlib import Path
import hashlib,json,os,signal,subprocess,sys,time,urllib.request,uuid
from host_load_guard import HostLoadGuard
R=Path(__file__).resolve().parents[1];B=R/'.soliton-runtime/llama-b11960/llama-b11960';D=R/'.soliton-runtime/lfm3-omni-2b';OUT=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else R/'.soliton-runtime/lfm3-realtime-probe-20261029'
assert not OUT.exists();OUT.mkdir(mode=0o700)
proof=json.loads((R/'.soliton-runtime/lfm3-omni-media-text-20261029-attempt2.json').read_bytes());assert proof['passed'] and proof['elapsedSeconds']>=1800 and proof['shutdownClean']
rows=subprocess.check_output(['/bin/ps','-axo','command='],text=True).splitlines()
assert not any(x.strip().split()[0].split('/')[-1] in {'studio-server','llama-server','llama-tts','llama'} or any(name in x for name in ('/mlx_kv_budget.py --model ', '/mlx_internal_spans.py --model ')) for x in rows if x.strip()),'another owned inference engine is present'
cpu_limit=100 if os.environ.get("OMNI_PROBE_FUNCTIONAL_ONLY") else 20

g=HostLoadGuard();stable=0;deadline=time.monotonic()+1800
try:
 while stable<6:
  cpu=g.snapshot()['busyPercent'];stable=stable+1 if cpu<=cpu_limit else 0;print(f'quiet CPU={cpu:.1f}% stable={stable}/6',flush=True);assert time.monotonic()<deadline,'host busy';time.sleep(5)
finally:g.stop()
key=uuid.uuid4().hex;env={k:v for k,v in os.environ.items() if not k.startswith(('LLAMA_','GGML_'))};env['LLAMA_API_KEY']=key;env['OMNI_PROBE_KEY']=key;env['OMNI_PROBE_OUTPUT']=str(OUT)
report=dict(transportPassed=False,qualified=False,admissionVerified=False,scope='native WebSocket speech transport probe only')
child=None
relay=None

def stop():
 global child
 if child and child.poll() is None:
  os.killpg(child.pid,signal.SIGTERM)
  try:child.wait(timeout=20)
  except subprocess.TimeoutExpired:os.killpg(child.pid,signal.SIGKILL);child.wait()
 if child:
  try:os.killpg(child.pid,0)
  except ProcessLookupError:report['ownedShutdownClean']=True
 child=None

def interrupt(signum,frame):stop();raise KeyboardInterrupt('owned realtime probe interrupted')
signal.signal(signal.SIGTERM,interrupt);signal.signal(signal.SIGINT,interrupt)
try:
 if os.environ.get('OMNI_PROBE_BOUNDARY'):
  server=Path(os.environ['OMNI_PROBE_SERVER']).resolve()
  manifest=json.loads(Path(os.environ['OMNI_PROBE_IDENTITY']).read_text())
  assert manifest['scope']=='realtime_preflight' and manifest['qualified'] is False
  for name,digest in manifest['files'].items():
   assert hashlib.sha256((server.parent/name).read_bytes()).hexdigest()==digest
  assert {'llama-server','libllama-server-impl.dylib'} <= manifest['files'].keys()
  report['runtimeIdentity']=manifest
 with (OUT/'server.log').open('xb') as log:
  child=subprocess.Popen([*([os.environ['OMNI_PROBE_SERVER']] if 'OMNI_PROBE_SERVER' in os.environ else [str(B/'llama'),'serve']),'--alias','LiquidAI/LFM3-Omni-2B','-m',str(D/'LFM3-Omni-2B-Q4_K_M.gguf'),'-mm',str(D/'mmproj-LFM3-Omni-2B-Q8_0.gguf'),'--host','127.0.0.1','--port','19341','--parallel','1','-c','4096','--no-webui','--reasoning','off','--temp','0','--video-ffmpeg-dir','/opt/homebrew/bin'],env=env,stdout=log,stderr=log,start_new_session=True)
  for _ in range(300):
   assert child.poll() is None,'runtime exited'
   try:
    req=urllib.request.Request('http://127.0.0.1:19341/health',headers={'Authorization':'Bearer '+key})
    with urllib.request.urlopen(req,timeout=3) as f:
     if json.load(f).get('status')=='ok':break
   except Exception:pass
   time.sleep(1)
  else:raise RuntimeError('startup timeout')
  if os.environ.get('OMNI_PROBE_RELAY'):
   relay=subprocess.Popen([str(R/'target/debug/examples/omni_realtime_preflight')],env=env,
                          stdout=log,stderr=log)
   for _ in range(50):
    assert relay.poll() is None
    try:
     import socket
     with socket.create_connection(('127.0.0.1',19342),timeout=.2):break
    except OSError:time.sleep(.1)
   else:raise RuntimeError('Relay startup timeout')
   env['OMNI_PROBE_WS']='ws://127.0.0.1:19342/v1/realtime'
   report['sharedRelayExercised']=True
  if os.environ.get('OMNI_PROBE_BOUNDARY'):
   boundaries={}
   for mode in ('text','audio','interleaved'):
    caps=[1]; boundary=[]
    for label in ('discover','exact','overflow'):
     case=OUT/f'{mode}-{label}';case.mkdir()
     cap=caps.pop(0)
     with (case/'client.log').open('xb') as clientlog:
      subprocess.run([str(R/'target/debug/examples/omni_realtime_probe')],
                     env=dict(env,OMNI_PROBE_OUTPUT=str(case),OMNI_PROBE_MAX_OUTPUT=str(cap),OMNI_PROBE_MODE=mode),
                     stdout=clientlog,stderr=clientlog,timeout=180,check=True)
     events=[json.loads(line) for line in (case/f'{mode}.jsonl').read_text().splitlines()]
     [admission]=[event for event in events if event['type']=='soliton.realtime.admission']
     complete=[event['response']['usage'] for event in events if event['type']=='response.done']
     errors=[event['error'] for event in events if event['type']=='error']
     deltas=sum(event['type'] in {'response.output_text.delta','response.output_audio.delta','soliton.audio.buffered'} for event in events)
     assert admission['context_positions']+admission['available_positions']+1==4096
     assert admission['input_tokens']==admission['context_positions']>0
     assert admission['output_reservation']==cap
     if label=='discover':
      available=admission['available_positions']
      caps.extend([available,available+1])
     else:
      assert admission['context_positions']==boundary[0]['admission']['context_positions']
     if label=='overflow':
      assert not admission['accepted'] and not complete and not deltas
      assert any('reservation exceeds' in error['message'] for error in errors)
     else:
      assert admission['accepted'] and not errors and len(complete)==1
      assert complete[0]['input_tokens']==admission['input_tokens']
      assert 0<complete[0]['output_tokens']<=cap
     boundary.append(dict(case=label,admission=admission,usage=complete,errors=errors,outputDeltas=deltas))
    boundaries[mode]=boundary
   cancelled=OUT/'cancel';cancelled.mkdir()
   with (cancelled/'client.log').open('xb') as log:
    subprocess.run([str(R/'target/debug/examples/omni_realtime_probe')],
                   env=dict(env,OMNI_PROBE_OUTPUT=str(cancelled),OMNI_PROBE_MODE='audio',
                            OMNI_PROBE_MAX_OUTPUT='512',OMNI_PROBE_CANCEL='1'),
                   stdout=log,stderr=log,timeout=180,check=True)
   data=json.loads((cancelled/'results.json').read_text())['cases'][0]
   assert data['cancelled'] and data['audioBytes']==0 and not (cancelled/'cancelled.wav').exists()
   report['cancellation']=dict(nativeStatus='cancelled',discardedAudio=True,wavCreated=False)
   followup=OUT/'multiturn';followup.mkdir()
   followup_env=dict(env,OMNI_PROBE_OUTPUT=str(followup),OMNI_PROBE_MAX_OUTPUT='128')
   followup_env.pop('OMNI_PROBE_BOUNDARY')
   with (followup/'client.log').open('xb') as log:
    subprocess.run([str(R/'target/debug/examples/omni_realtime_probe')],
                   env=followup_env,stdout=log,stderr=log,timeout=180,check=True)
   accounting={}
   for mode in ('text','audio','interleaved'):
    events=[json.loads(line) for line in (followup/f'{mode}.jsonl').read_text().splitlines()]
    admitted=[e for e in events if e['type']=='soliton.realtime.admission']
    terminal=[e['response']['usage'] for e in events if e['type']=='response.done']
    assert len(admitted)==len(terminal)==2
    for admission,usage in zip(admitted,terminal):
     assert admission['accepted'] and admission['input_tokens']==usage['input_tokens']>0
    accounting[mode]=dict(admission=admitted,usage=terminal)
   report['multiturnAccounting']=accounting
   report['boundaries']=boundaries
   report['transportPassed']=True
  else:
   with (OUT/'client.log').open('xb') as clientlog:
    result=subprocess.run([str(R/'target/debug/examples/omni_realtime_probe')],env=env,stdout=clientlog,stderr=clientlog,timeout=600)
   report['transportPassed']=result.returncode==0
   if (OUT/'results.json').exists():report['results']=json.loads((OUT/'results.json').read_bytes())
except Exception as e:report['error']=str(e)
finally:
 if relay is not None:
  relay.terminate();relay.wait(timeout=10)
 stop();p=OUT/'transport.json';assert not p.exists();p.write_text(json.dumps(report,indent=2)+'\n');p.chmod(0o600)
print(json.dumps(report),flush=True)
