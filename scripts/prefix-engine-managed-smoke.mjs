// Explicit isolated-host test: can target the native QA app or an isolated studio-server.
import assert from 'node:assert/strict';
import { readFile, writeFile, mkdtemp, chmod, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const args=process.argv.slice(2);
if(!args.includes('--isolated'))throw Error('Requires --isolated and a dedicated QA host; never run on ordinary app state');
const option=(name,fallback)=>{const i=args.indexOf(name);return i<0?fallback:args[i+1]};
const api=option('--api','http://127.0.0.1:8081');
assert.equal(new URL(api).hostname,'127.0.0.1');
const output=option('--output','/tmp/soliton-prefix-managed-report.json');
const session=await fetch(api+'/studio/session',{method:'POST',headers:{Origin:api}}).then(r=>r.json());
const headers={'Content-Type':'application/json','x-studio-token':session.token,Authorization:'Bearer '+session.token};
async function request(path,body,status=200){const r=await fetch(api+path,{method:body===undefined?'GET':'POST',headers,body:body===undefined?undefined:JSON.stringify(body)});assert.equal(r.status,status,await r.clone().text());return r;}
const state=()=>request('/studio/state').then(r=>r.json());
const action=(action,data={},status=200)=>request('/studio/action',{action,...data},status).then(r=>r.json());
async function waitFor(fn,label){for(let i=0;i<360;i++){if(await fn())return;await new Promise(r=>setTimeout(r,250));}throw Error('Timeout: '+label);}
const baseline=await state();assert.equal(baseline.settings.engine,'mlxPrefix','Select the dedicated QA MLX SSD configuration first');
const prefix=baseline.settings.prefixCache;
const evidence=JSON.parse(await readFile(prefix.evidence,'utf8'));
const originalMessages=baseline.document.messages;
const folder=await mkdtemp(join(tmpdir(),'prefix-managed-'));
const checks={};
let leftRunning=false;
async function start(){await action('start');leftRunning=true;await waitFor(async()=> (await state()).status==='ready','qualified engine readiness');const st=await state();assert.equal(st.backend,'MLX SSD');return st;}
async function stop(){await action('stop');leftRunning=false;assert.equal((await state()).status,'offline');}
const prompt='The fictional code word is soliton. '.repeat(20)+'What is the code word? Answer briefly.';
const body={model:'default_model',messages:[{role:'user',content:prompt}],temperature:0,seed:0,max_tokens:16};
const infer=async(b=body)=>request('/v1/chat/completions',b).then(r=>r.json());
try{
 if(baseline.status!=='offline')await stop();
 let running=await start();checks.qualifiedStartup=true;
 const capacity=await request('/studio/prefix-capacity').then(r=>r.json());assert(capacity.capacity.entryCount>0);checks.capacityInspection=true;
 await action('trimPrefixCache',{},409);checks.liveTrimRefused=true;
 const first=await infer();const answer=first.choices[0].message.content;assert(answer.toLowerCase().includes('soliton'));
 assert.equal(first.usage.prompt_tokens_details.cached_tokens,177);
 const metrics=await request('/studio/prefix-cache').then(r=>r.json());assert.equal(metrics.identity,evidence.identity);assert(metrics.hits>=1);checks.diskReuse=true;
 assert.equal(metrics.experimental,false);assert.equal(metrics.productionIntegration,true);

 if (args.includes('--require-persistence-timings')) {
  const phases = metrics.persistenceTimings;
  for (const name of ['restoreStage', 'storeGet', 'deserialize']) {
   const row = phases?.[name];
   assert(Number.isSafeInteger(row?.calls) && row.calls >= 1, name);
   assert.equal(row.exceptions, 0, name);
   assert(Number.isSafeInteger(row.maxNs) && row.maxNs > 0 && row.totalNs >= row.maxNs, name);
  }
  for (const name of ['saveStage', 'storePut', 'serialize']) {
   assert.deepEqual(phases[name], {calls: 0, exceptions: 0, totalNs: 0, maxNs: null}, name);
  }
  checks.persistenceTimings = true;
 }

 await action('start',{},400);checks.exclusiveStart=true;
 await action('settings',{settings:{...running.settings,prefixCache:{...prefix,quotaMiB:512}}},409);checks.settingsLocked=true;
 await request('/v1/chat/completions',{...body,max_tokens:129},422);
 await request('/v1/chat/completions',{...body,messages:[{role:'user',content:'a '.repeat(600)}]},422);
 await request('/v1/chat/completions',{...body,tools:[]},422);checks.admissionRefusals=true;
 assert.deepEqual((await state()).document.messages,originalMessages);
 // SSE traverses the same Rust gateway as GUI streaming.
 const stream=await request('/v1/chat/completions',{...body,stream:true,stream_options:{include_usage:true}});
 const raw=await stream.text();let text='';let usage=null;
 for(const line of raw.split('\n')){if(!line.startsWith('data:')||line.slice(5).trim()==='[DONE]')continue;const frame=JSON.parse(line.slice(5));text+=frame.choices?.[0]?.delta?.content||'';if(frame.usage)usage=frame.usage;}
 assert.equal(text,answer);assert(usage?.prompt_tokens_details?.cached_tokens>0);checks.sse=true;
 const repeatedMetrics=await request('/studio/prefix-cache').then(r=>r.json());
 assert.equal(repeatedMetrics.writes,metrics.writes);assert.equal(repeatedMetrics.serializedBytes,metrics.serializedBytes);
 assert(repeatedMetrics.skippedWrites>metrics.skippedWrites);checks.duplicateWriteSuppression=true;
 const pid=running.memoryUsage.engine?.current?.pid;assert(pid>0,'Owned engine PID unavailable');
 await stop();assert.throws(()=>process.kill(pid,0));checks.ownedProcessCleanup=true;
 await start();const again=await infer();assert.equal(again.choices[0].message.content,answer);assert.equal(again.usage.prompt_tokens_details.cached_tokens,177);checks.restartReuse=true;
 await stop();
 // A genuine report with changed quota or expiry must not authorize startup.
 let current=(await state()).settings;
 await action('settings',{settings:{...current,prefixCache:{...prefix,quotaMiB:512}}});await action('start',{},400);assert.equal((await state()).status,'offline');checks.wrongQuotaRefused=true;
 const expired=join(folder,'expired.json');await writeFile(expired,JSON.stringify({...evidence,created_at:Date.now()/1000-8*86400}));await chmod(expired,0o600);
 await action('settings',{settings:{...current,prefixCache:{...prefix,evidence:expired}}});await action('start',{},400);assert.equal((await state()).status,'offline');checks.expiredEvidenceRefused=true;
 await action('settings',{settings:current});
 // Switch to a second engine and back without disturbing conversation state.
 await start();await action('switchEngine',{engine:'strata'});leftRunning=false;assert.equal((await state()).status,'offline');
 await action('switchEngine',{engine:'mlxPrefix'});assert.deepEqual((await state()).document.messages,originalMessages);checks.engineSwitch=true;
 await start();checks.recovery=true;
 const final=await state();assert.deepEqual(final.document.messages,originalMessages);checks.conversationsPreserved=true;
 await stop();
 const maintained=await action('trimPrefixCache');assert(maintained.capacity.usedBytes<=maintained.capacity.quotaBytes);checks.stoppedMaintenance=true;
 const report={result:'passed',qualificationIdentity:evidence.identity,checks,initialCapacity:capacity.capacity,engine:'MLX SSD',api};
 await writeFile(output,JSON.stringify(report,null,2));console.log(JSON.stringify(report));
}finally{
 if(leftRunning)await stop().catch(()=>{});
 const current=await state();if(current.status==='offline')await action('settings',{settings:{...baseline.settings,engine:current.settings.engine}}).catch(()=>{});
 await rm(folder,{recursive:true,force:true});
}
