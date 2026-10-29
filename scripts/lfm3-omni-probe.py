"""Isolated official-runtime smoke, never Soliton startup qualification."""
import base64
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import urllib.request
import uuid
import wave
from host_load_guard import HostLoadGuard
R=Path(__file__).resolve().parents[1]
D=R/'.soliton-runtime/lfm3-omni-2b'
B=R/'.soliton-runtime/llama-b11960/llama-b11960'
OUT=R/'docs/evaluation/lfm3-omni-runtime-smoke-20261029.json'
assert not OUT.exists()
rows=subprocess.check_output(['/bin/ps','-axo','command='],text=True).splitlines()
assert not any(x.strip().split()[0].split('/')[-1] in {'studio-server','soliton','llama-server'} or '/mlx_kv_budget.py --model ' in x or 'run-active-retention-comparison.py' in x for x in rows if x.strip()),'another engine/experiment is active'
g=HostLoadGuard();stable=0;deadline=time.monotonic()+1800
try:
 while stable<6:
  cpu=g.snapshot()['busyPercent'];stable=stable+1 if cpu<=20 else 0
  print(f'quiet CPU={cpu:.1f}% stable={stable}/6',flush=True)
  assert time.monotonic()<deadline,'host busy; no model launched'
  time.sleep(5)
finally:g.stop()
# Synthetic fixtures remain local; no recordings or personal files are used.
subprocess.run(['/opt/homebrew/bin/ffmpeg','-v','error','-f','lavfi','-i','color=c=red:s=128x128','-frames:v','1','-threads','1','-y',str(D/'red.png')],check=True)
subprocess.run(['/usr/bin/say','-o',str(D/'spoken.aiff'),'The secret number is forty two.'],check=True)
subprocess.run(['/opt/homebrew/bin/ffmpeg','-v','error','-i',str(D/'spoken.aiff'),'-ar','16000','-ac','1','-y',str(D/'spoken.wav')],check=True)
subprocess.run(['/opt/homebrew/bin/ffmpeg','-v','error','-f','lavfi','-i','color=c=red:s=128x128:r=2:d=2','-i',str(D/'spoken.wav'),'-t','2','-c:v','libx264','-threads','1','-c:a','aac','-y',str(D/'red.mp4')],check=True)
key=uuid.uuid4().hex;keyfile=D/'probe.key';keyfile.write_text(key+'\n');keyfile.chmod(0o600)
report=dict(passed=False,certification=False,scope='official runtime smoke only; not Soliton integration or exact multimodal admission',checks={},responses={})
child=None

def stop():
 global child
 if child and child.poll() is None:
  os.killpg(child.pid,signal.SIGTERM)
  try:child.wait(timeout=20)
  except subprocess.TimeoutExpired:os.killpg(child.pid,signal.SIGKILL);child.wait()
 child=None

def interrupted(signum,frame):
 stop();raise KeyboardInterrupt('owned smoke interrupted')
signal.signal(signal.SIGTERM,interrupted);signal.signal(signal.SIGINT,interrupted)
def req(path,body=None):
 data=None if body is None else json.dumps(body).encode()
 request=urllib.request.Request('http://127.0.0.1:19341'+path,data=data,headers={'Content-Type':'application/json','Authorization':'Bearer '+key})
 with urllib.request.urlopen(request,timeout=180) as f:return json.loads(f.read(2*1024*1024))
def content(path,mime):return 'data:'+mime+';base64,'+base64.b64encode(path.read_bytes()).decode()
try:
 with (D/'server.log').open('xb') as log:
  child=subprocess.Popen([str(B/'llama'),'serve','-m',str(D/'LFM3-Omni-2B-Q4_K_M.gguf'),'-mm',str(D/'mmproj-LFM3-Omni-2B-Q8_0.gguf'),'--host','127.0.0.1','--port','19341','--parallel','1','-c','4096','--no-webui','--reasoning','off','--temp','0','--api-key-file',str(keyfile)],stdout=log,stderr=log,start_new_session=True)
  for _ in range(300):
   assert child.poll() is None,'runtime exited during startup'
   try:
    if req('/health').get('status')=='ok':break
   except Exception:pass
   time.sleep(1)
  else:raise RuntimeError('model startup timeout')
  fixtures=[('text','Reply only with the number 42.',None),('image','What color fills this image? Reply with one English color word.',{'type':'image_url','image_url':{'url':content(D/'red.png','image/png')}}),('audio','Transcribe the recording.',{'type':'input_audio','input_audio':{'data':base64.b64encode((D/'spoken.wav').read_bytes()).decode(),'format':'wav'}}),('video','What color fills the video? Reply with one English color word.',{'type':'input_video','input_video':{'data':base64.b64encode((D/'red.mp4').read_bytes()).decode()}})]
  for mode,text,media in fixtures:
   messages=[{'role':'system','content':'Respond in text.'},{'role':'user','content':text if media is None else [media,{'type':'text','text':text}]}]
   try:
    v=req('/v1/chat/completions',dict(messages=messages,max_tokens=64,temperature=0));answer=v['choices'][0]['message']['content'];usage=v['usage'];assert isinstance(usage['prompt_tokens'],int) and usage['prompt_tokens']>0
    exact=(answer.strip().rstrip('.').lower()=='42' if mode=='text' else 'red' in answer.lower() if mode in {'image','video'} else ('forty' in answer.lower() or '42' in answer))
    report['checks'][mode]=exact;report['responses'][mode]=dict(answer=answer,usage=usage)
    print(mode,exact,flush=True)
   except Exception as e:report['checks'][mode]=False;report['responses'][mode]={'error':str(e)}
 stop()
 with (D/'tts.log').open('xb') as log:
  child=subprocess.Popen([str(B/'llama-tts'),'-m',str(D/'LFM3-Omni-2B-Q4_K_M.gguf'),'-mm',str(D/'mmproj-LFM3-Omni-2B-Q8_0.gguf'),'-p','Hello world.','-o',str(D/'out.wav'),'-n','128'],stdout=log,stderr=log,start_new_session=True)
  status=child.wait(timeout=300);assert status==0,'TTS process failed'
  with wave.open(str(D/'out.wav')) as f:report['checks']['audioOutput']=f.getframerate()==24000 and f.getnframes()>0
 report['passed']=all(report['checks'].values())
except Exception as e:report['error']=str(e)
finally:
 stop();keyfile.unlink(missing_ok=True);report['ownedShutdownClean']=True
 OUT.write_text(json.dumps(report,indent=2)+'\n');OUT.chmod(0o600)
print(json.dumps(report),flush=True)
