"""Original-runtime1800-second media/text qualification, no bypass switches."""
from pathlib import Path
import hashlib,json,os,signal,subprocess,sys,time,urllib.request,uuid,shutil
from host_load_guard import HostLoadGuard
R=Path(__file__).resolve().parents[1];D=R/'.soliton-runtime/lfm3-omni-2b';B=R/'.soliton-runtime/llama-b11960/llama-b11960'
proof=R/'.soliton-runtime/lfm3-omni-media-text-20261029-attempt2.json';assert not proof.exists()
parity=R/'docs/evaluation/lfm3-omni-count-parity-20261029.json';deadline=time.monotonic()+1800
while not parity.exists():
 assert time.monotonic()<deadline,'processor parity did not finish; no second model'
 time.sleep(10)
p=json.loads(parity.read_bytes());assert p['passed'] and p['ownedShutdownClean'] and all(p['responses'][m].get('exactCountParity') is True for m in ['text','image','audio','video']),'media count parity required'
rows=subprocess.check_output(['/bin/ps','-axo','command='],text=True).splitlines()
assert not any(x.strip().split()[0].split('/')[-1] in {'studio-server','soliton','llama-server','llama-tts'} or '/mlx_kv_budget.py --model ' in x for x in rows if x.strip()),'another inference engine is active'
g=HostLoadGuard();stable=0;deadline=time.monotonic()+1800
try:
 while stable<6:
  cpu=g.snapshot()['busyPercent'];stable=stable+1 if cpu<=20 else 0;print(f'quiet CPU={cpu:.1f} stable={stable}/6',flush=True);assert time.monotonic()<deadline,'host busy';time.sleep(5)
finally:g.stop()
def digest(path):
 with path.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def identity():
 h=hashlib.sha256()
 for path in sorted(x for x in B.rglob('*') if x.is_file()):
  assert path.resolve().is_relative_to(B.resolve());h.update(str(path.relative_to(B)).encode()+b'\0'+digest(path).encode()+b'\n')
 return dict(runtimeSha256=h.hexdigest(),modelSha256=digest(D/'LFM3-Omni-2B-Q4_K_M.gguf'),projectorSha256=digest(D/'mmproj-LFM3-Omni-2B-Q8_0.gguf'),platform='macos-aarch64',hardware=subprocess.check_output(['/usr/sbin/sysctl','-n','hw.model','hw.memsize','machdep.cpu.brand_string'],text=True).strip(),ffmpegSha256=digest(Path('/opt/homebrew/bin/ffmpeg')),ffprobeSha256=digest(Path('/opt/homebrew/bin/ffprobe')),osVersion=subprocess.check_output(['/usr/bin/sw_vers','-productVersion'],text=True).strip(),context=4096,parallel=1,reasoning=False,admissionVersion=1)
key=uuid.uuid4().hex;env={k:v for k,v in os.environ.items() if not k.startswith(('LLAMA_','GGML_'))};env['LLAMA_API_KEY']=key
media=R/'.soliton-runtime/omni-media';media.mkdir(exist_ok=True)
report=dict(schema=1,scope='omni_media_text_30min',passed=False,identity=identity(),createdAt=int(time.time()),elapsedSeconds=0,failures=0,successes=0,shutdownClean=False,exactCountParity={m:False for m in ['text','image','audio','video']},limitations=['Text-output scope only; speech/realtime output remains unqualified.','This report is not Soliton integration/boundary approval.'])
child=None

def stop():
 global child
 if child and child.poll() is None:
  os.killpg(child.pid,signal.SIGTERM)
  try:child.wait(timeout=20)
  except subprocess.TimeoutExpired:os.killpg(child.pid,signal.SIGKILL);child.wait()
 if child:
  try:os.killpg(child.pid,0)
  except ProcessLookupError:report['shutdownClean']=True
 child=None

def interrupted(signum,frame):stop();raise KeyboardInterrupt('owned qualification interrupted')
signal.signal(signal.SIGTERM,interrupted);signal.signal(signal.SIGINT,interrupted)
def request(path,body=None):
 req=urllib.request.Request('http://127.0.0.1:19341'+path,data=None if body is None else json.dumps(body).encode(),headers={'Content-Type':'application/json','Authorization':'Bearer '+key})
 with urllib.request.urlopen(req,timeout=180) as f:return json.loads(f.read(2*1024*1024))
try:
 with (D/'qualification-server-attempt2.log').open('xb') as log:
  child=subprocess.Popen([str(B/'llama'),'serve','--alias','LiquidAI/LFM3-Omni-2B','-m',str(D/'LFM3-Omni-2B-Q4_K_M.gguf'),'-mm',str(D/'mmproj-LFM3-Omni-2B-Q8_0.gguf'),'--host','127.0.0.1','--port','19341','--parallel','1','-c','4096','--no-webui','--reasoning','off','--temp','0','--media-path',str(media),'--video-ffmpeg-dir','/opt/homebrew/bin'],env=env,stdout=log,stderr=log,start_new_session=True)
  for _ in range(300):
   assert child.poll() is None,'model died'
   try:
    if request('/health').get('status')=='ok':break
   except Exception:pass
   time.sleep(1)
  else:raise RuntimeError('startup timeout')
  report['mediaChecks']={}
  for mode,filename,typ,payload,question in [
    ('text',None,None,None,'Reply only with42.'),
    ('image','red.png','image_url','image_url','Name the image color in one word.'),
    ('audio','spoken.wav','input_audio','input_audio','What secret number is spoken? Reply only with the number.'),
    ('video','red.mp4','input_video','input_video','Name the video color in one word.')]:
   content=question
   if filename:
    asset=media/str(uuid.uuid4());shutil.copyfile(D/filename,asset);asset.chmod(0o400)
    content=[{'type':typ,payload:{'url':'file://'+asset.name}},{'type':'text','text':question}]
   b=dict(messages=[dict(role='system',content='Respond in text.'),dict(role='user',content=content)],max_tokens=64,temperature=0)
   count=request('/v1/chat/completions/input_tokens',b)['input_tokens'];v=request('/v1/chat/completions',b);assert v['usage']['prompt_tokens']==count and type(count) is int and count>0
   answer=v['choices'][0]['message']['content'].strip().rstrip('.').lower();assert (answer=='42' if mode in {'text','audio'} else answer=='red'), 'media quality probe failed: '+mode
   b.update(stream=True,stream_options={'include_usage':True})
   req=urllib.request.Request('http://127.0.0.1:19341/v1/chat/completions',data=json.dumps(b).encode(),headers={'Content-Type':'application/json','Authorization':'Bearer '+key})
   streamed='';usage=None;done=False
   with urllib.request.urlopen(req,timeout=180) as response:
    for raw in response:
     if not raw.startswith(b'data:'):continue
     line=raw[5:].strip()
     if line==b'[DONE]':done=True;break
     event=json.loads(line)
     if event.get('usage'):usage=event['usage']
     for choice in event.get('choices',[]):streamed+=choice.get('delta',{}).get('content','') or ''
   assert done and usage and usage['prompt_tokens']==count and streamed.strip().rstrip('.').lower()==answer,'SSE count/answer mismatch'
   report['exactCountParity'][mode]=True;report['mediaChecks'][mode]={'json':True,'sse':True,'inputTokens':count}
  # Native SSE and JSON share the same exact serialized prompt count above.

  for language in ['Return only the number42.','数字42だけを返してください。','只回复数字42。']:
   body=dict(messages=[dict(role='system',content='Respond in text.'),dict(role='user',content=language)],max_tokens=16,temperature=0)
   count=request('/v1/chat/completions/input_tokens',body)['input_tokens'];v=request('/v1/chat/completions',body);assert v['usage']['prompt_tokens']==count and v['choices'][0]['message']['content'].strip()=='42'
  body=dict(messages=[dict(role='system',content='Respond in text.'),dict(role='user',content='Reply only with42.')],max_tokens=16,temperature=0)
  started=time.monotonic()
  while time.monotonic()-started<1800:
   count=request('/v1/chat/completions/input_tokens',body)['input_tokens'];v=request('/v1/chat/completions',body);assert v['usage']['prompt_tokens']==count and v['choices'][0]['message']['content'].strip()=='42';report['successes']+=1
  report['elapsedSeconds']=time.monotonic()-started
  assert report['identity']==identity(),'runtime/model identity changed'
 stop();report['passed']=report['shutdownClean'] and report['elapsedSeconds']>=1800 and report['failures']==0
except Exception as e:report['error']=str(e);report['failures']+=1
finally:
 stop();proof.write_text(json.dumps(report,indent=2)+'\n');proof.chmod(0o600)
print(json.dumps(report),flush=True)
