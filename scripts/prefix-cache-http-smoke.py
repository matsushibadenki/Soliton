import argparse, http.client, json, os, pathlib, secrets, signal, subprocess, tempfile, time, urllib.request

parser=argparse.ArgumentParser(description="Opt-in real Gemma HTTP SSD-cache regression (isolated process and temporary store)")
parser.add_argument("--python",required=True)
parser.add_argument("--model",required=True,type=pathlib.Path)
parser.add_argument("--port",type=int,default=18082)
args=parser.parse_args()
ROOT=pathlib.Path(__file__).resolve().parents[1]
PYTHON=args.python
MODEL=args.model.resolve(strict=True)
TOKEN=secrets.token_hex(24)
PORT=args.port
PROMPT='The fictional code word is soliton. '*20+'What is the code word? Answer briefly.'
BODY=dict(model='default_model',messages=[dict(role='user',content=PROMPT)],temperature=0,seed=0,max_tokens=16)
def request(path='/studio/ssd-cache',body=None,auth=True):
 headers={'Content-Type':'application/json'}
 if auth:headers['Authorization']='Bearer '+TOKEN
 req=urllib.request.Request(f'http://127.0.0.1:{PORT}'+path,headers=headers,data=None if body is None else json.dumps(body).encode())
 with urllib.request.urlopen(req,timeout=90) as r:return json.load(r)
def start(folder,log):
 env=dict(os.environ,SOLITON_PREFIX_HTTP_TOKEN=TOKEN)
 p=subprocess.Popen([PYTHON,str(ROOT/'scripts/prefix-cache-http.py'),'--model',str(MODEL),'--cache-root',str(folder/'cache'),'--qualification-mode','--quota-mib','256','--port',str(PORT)],env=env,stdout=log,stderr=log)
 for _ in range(100):
  if p.poll() is not None:raise RuntimeError('Server exited; inspect /tmp/soliton-prefix-http.log')
  try:request();return p
  except Exception:time.sleep(.3)
 p.kill();p.wait();raise RuntimeError('Startup timeout')
def stop(p):
 p.send_signal(signal.SIGINT)
 try:p.wait(timeout=15)
 except subprocess.TimeoutExpired:p.kill();p.wait();raise
with tempfile.TemporaryDirectory(prefix='soliton-prefix-http-') as temp,open('/tmp/soliton-prefix-http.log','w') as log:
 folder=pathlib.Path(temp)
 p=start(folder,log)
 try:
  try:request(auth=False);raise AssertionError('Unauthorized allowed')
  except urllib.error.HTTPError as e:assert e.code==401
  for invalid in [dict(BODY,stream='yes'),dict(BODY,model='other'),dict(BODY,tools=[])]:
   try:request('/v1/chat/completions',invalid);raise AssertionError('Unsupported request accepted')
   except urllib.error.HTTPError as error:assert error.code==400
  connection=http.client.HTTPConnection('127.0.0.1',PORT,timeout=10)
  connection.request('GET','/studio/ssd-cache',headers={'Authorization':'Bearer '+chr(233)})
  assert connection.getresponse().status==401;connection.close()
  a=request('/v1/chat/completions',BODY);print('FIRST_STATS',request(),flush=True);assert request()['writes']==1
  print('COLD',a['choices'][0]['message']['content'],request(),flush=True)
 finally:stop(p)
 p=start(folder,log)
 try:
  b=request('/v1/chat/completions',BODY);assert request()['hits']==1
  assert a['choices'][0]['message']['content']==b['choices'][0]['message']['content']
  assert b['usage']['prompt_tokens_details']['cached_tokens']>0
  print('RESTORED',b['usage'],request(),flush=True)
 finally:stop(p)
 cache=next((folder/'cache/store').glob('*.cache'))
 with cache.open('r+b') as f:f.seek(4104);byte=f.read(1);f.seek(4104);f.write(bytes([byte[0]^1]))
 p=start(folder,log)
 try:
  c=request('/v1/chat/completions',BODY);assert request()['hits']==0;assert request()['misses']==1
  assert a['choices'][0]['message']['content']==c['choices'][0]['message']['content']
  print('CORRUPTION_FALLBACK',request(),flush=True)
  # Same-worker warm requests must avoid serialization/put, but repair changed entries.
  before=request()
  for _ in range(5):
   repeated=request('/v1/chat/completions',BODY)
   assert repeated['choices'][0]['message']['content']==a['choices'][0]['message']['content']
  after=request();assert after['writes']==before['writes'];assert after['skippedWrites']>=before['skippedWrites']+5
  assert after['serializedBytes']==before['serializedBytes']
  for damage in ['corrupt','delete']:
   if damage=='delete':cache.unlink()
   else:
    with cache.open('r+b') as f:f.seek(4104);byte=f.read(1);f.seek(4104);f.write(bytes([byte[0]^1]))
   before=request();fixed=request('/v1/chat/completions',BODY)
   assert fixed['choices'][0]['message']['content']==a['choices'][0]['message']['content']
   assert request()['writes']==before['writes']+1
  print('DEDUP_AND_LIVE_REPAIR',request(),flush=True)

  # Stream a request, disconnect, then verify the next ordinary request recovers.
  conn=http.client.HTTPConnection('127.0.0.1',PORT,timeout=60)
  conn.request('POST','/v1/chat/completions',json.dumps(dict(BODY,stream=True,max_tokens=128)),{'Content-Type':'application/json','Authorization':'Bearer '+TOKEN})
  response=conn.getresponse();assert response.status==200;response.read(32);conn.close();time.sleep(1)
  d=request('/v1/chat/completions',BODY);assert d['choices'][0]['message']['content']==a['choices'][0]['message']['content']
  print('PASS: authenticated HTTP, process-restart SSD hit, answer parity, corrupt-cache recompute, disconnected SSE recovery',flush=True)
 finally:stop(p)

 # A sparse fixture occupies logical quota without consuming a model-sized allocation.
 with (folder/'cache/store/quota-fixture.cache').open('wb') as filler:filler.truncate(256*1024**2)
 p=start(folder,log)
 try:
  other=dict(BODY,messages=[dict(role='user',content=PROMPT+' Use a short sentence.')])
  answer=request('/v1/chat/completions',other)
  assert 'soliton' in answer['choices'][0]['message']['content'].lower()
  stats=request();assert stats['misses']==1 and stats['writes']==0 and stats['errors']>=1
  print('QUOTA_FALLBACK',stats,flush=True)
  (folder/'cache/store/quota-fixture.cache').unlink()
  # A failed put must not poison the memo: retry succeeds once quota is available.
  request('/v1/chat/completions',other);assert request()['writes']==1
  request('/v1/chat/completions',other);assert request()['writes']==1
  assert request()['skippedWrites']>=1
  print('QUOTA_RETRY',request(),flush=True)
 finally:stop(p)
