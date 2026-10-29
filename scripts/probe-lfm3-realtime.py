"""Native Realtime protocol probe. Never enables the Soliton speech route."""
from pathlib import Path
import json,os,signal,subprocess,sys,time,urllib.request,uuid
from host_load_guard import HostLoadGuard
R=Path(__file__).resolve().parents[1];B=R/'.soliton-runtime/llama-b11960/llama-b11960';D=R/'.soliton-runtime/lfm3-omni-2b';OUT=R/'.soliton-runtime/lfm3-realtime-probe-20261029'
assert not OUT.exists();OUT.mkdir(mode=0o700)
proof=json.loads((R/'.soliton-runtime/lfm3-omni-media-text-20261029-attempt2.json').read_bytes());assert proof['passed'] and proof['elapsedSeconds']>=1800 and proof['shutdownClean']
rows=subprocess.check_output(['/bin/ps','-axo','command='],text=True).splitlines()
assert not any(x.strip().split()[0].split('/')[-1] in {'studio-server','soliton','llama-server','llama-tts','llama'} or '/mlx_kv_budget.py --model ' in x for x in rows if x.strip()),'another owned inference engine is present'
g=HostLoadGuard();stable=0;deadline=time.monotonic()+1800
try:
 while stable<6:
  cpu=g.snapshot()['busyPercent'];stable=stable+1 if cpu<=20 else 0;print(f'quiet CPU={cpu:.1f}% stable={stable}/6',flush=True);assert time.monotonic()<deadline,'host busy';time.sleep(5)
finally:g.stop()
key=uuid.uuid4().hex;env={k:v for k,v in os.environ.items() if not k.startswith(('LLAMA_','GGML_'))};env['LLAMA_API_KEY']=key;env['OMNI_PROBE_KEY']=key;env['OMNI_PROBE_OUTPUT']=str(OUT)
report=dict(transportPassed=False,qualified=False,admissionVerified=False,scope='native WebSocket speech transport probe only')
child=None

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
 with (OUT/'server.log').open('xb') as log:
  child=subprocess.Popen([str(B/'llama'),'serve','--alias','LiquidAI/LFM3-Omni-2B','-m',str(D/'LFM3-Omni-2B-Q4_K_M.gguf'),'-mm',str(D/'mmproj-LFM3-Omni-2B-Q8_0.gguf'),'--host','127.0.0.1','--port','19341','--parallel','1','-c','4096','--no-webui','--reasoning','off','--temp','0','--video-ffmpeg-dir','/opt/homebrew/bin'],env=env,stdout=log,stderr=log,start_new_session=True)
  for _ in range(300):
   assert child.poll() is None,'runtime exited'
   try:
    req=urllib.request.Request('http://127.0.0.1:19341/health',headers={'Authorization':'Bearer '+key})
    with urllib.request.urlopen(req,timeout=3) as f:
     if json.load(f).get('status')=='ok':break
   except Exception:pass
   time.sleep(1)
  else:raise RuntimeError('startup timeout')
  with (OUT/'client.log').open('xb') as clientlog:
   result=subprocess.run([str(R/'target/debug/examples/omni_realtime_probe')],env=env,stdout=clientlog,stderr=clientlog,timeout=600)
  report['transportPassed']=result.returncode==0
  if (OUT/'results.json').exists():report['results']=json.loads((OUT/'results.json').read_bytes())
except Exception as e:report['error']=str(e)
finally:
 stop();p=R/'docs/evaluation/lfm3-omni-realtime-transport-20261029.json';assert not p.exists();p.write_text(json.dumps(report,indent=2)+'\n');p.chmod(0o600)
print(json.dumps(report),flush=True)
