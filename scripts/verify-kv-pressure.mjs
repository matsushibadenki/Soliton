// Real qualified shared-API pressure check; requires dedicated QA state.
import assert from 'node:assert/strict';import {writeFile} from 'node:fs/promises';
import {cacheHistory} from './cache-memory-workload.mjs';
const args=process.argv.slice(2),value=n=>args[args.indexOf(n)+1];assert(args.includes('--isolated')&&args.includes('--api')&&args.includes('--output'));
const api=value('--api');assert.equal(new URL(api).hostname,'127.0.0.1');assert(!['8080','8081',''].includes(new URL(api).port));
const output=value('--output');await writeFile(output,'',{flag:'wx',mode:0o600});
const {token}=await fetch(api+'/studio/session',{method:'POST',headers:{Origin:api}}).then(r=>r.json());const headers={'Content-Type':'application/json','x-studio-token':token};
async function request(path,body){const r=await fetch(api+path,{method:body===undefined?'GET':'POST',headers,body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(180000)});assert.equal(r.status,200,(await r.clone().text()).slice(0,300));return r.json();}
const state=()=>request('/studio/state'),action=(action,data={})=>request('/studio/action',{action,...data});
const initial=await state();assert.equal(initial.status,'offline');assert.equal(initial.settings.vllm.context,4096);assert.equal(initial.settings.vllm.kvCacheMiB,1536);
const report={result:'running',scope:'Qualified shared API, more histories than retained capacity; functional cap/recompute/reheat verification, no controlled performance claim',samples:[]};let owned=false,restoreSettings=null;
try{
 await action('start');owned=true;let ready;
 for(let i=0;i<300;i++){ready=await state();if(ready.status==='ready')break;if(ready.status==='offline')throw Error('Qualified startup refused');await new Promise(r=>setTimeout(r,250));}assert.equal(ready.status,'ready');report.engineSession=ready.engineSession;
 for(const [phase,index] of [['cold',0],['hot',0],['churn',1],['churn',2],['churn',3],['churn',4],['revisit',0],['reheat',0],['recent',4]]){
  const {messages}=cacheHistory(index);const result=await request('/v1/chat/completions',{model:'default_model',messages,max_tokens:64,temperature:0,stream:false});assert.equal(result.choices[0].message.content.trim(),'STRATA-7391');
  const usage=result.usage;assert(usage.prompt_tokens+64<=4096);const cached=usage.prompt_tokens_details.cached_tokens;assert(Number.isInteger(cached)&&cached>=0&&cached<=usage.prompt_tokens);
  await action('refreshBackendBudget');const snapshot=await state();const kv=snapshot.modelPlan.backendBudget?.retainedKv;assert(kv);assert.equal(kv.limitBytes,1536*1024**2);assert(kv.bytes<=kv.limitBytes);
  assert(snapshot.memoryUsage.current.availableBytes>=4*1024**3);
  report.samples.push({phase,index,promptTokens:usage.prompt_tokens,cachedTokens:cached,kv,engineMemory:snapshot.memoryUsage.engine.current});
  console.log(JSON.stringify({phase,index,cached,retainedBytes:kv.bytes}));
 }
 const by=phase=>report.samples.find(s=>s.phase===phase);
 assert(by('hot').cachedTokens>=by('hot').promptTokens-1);assert(by('revisit').cachedTokens<by('hot').cachedTokens/2);assert(by('reheat').cachedTokens>=by('reheat').promptTokens-1);
 assert.deepEqual((await state()).document.messages,initial.document.messages);report.historyPreserved=true;
 if(args.includes('--control-evidence')){
  await action('stop');owned=false;
  const correct=(await state()).settings;restoreSettings=correct;
  await action('settings',{settings:{...correct,vllm:{...correct.vllm,evidence:value('--control-evidence')}}});
  await action('start');owned=true;let refused;
  for(let i=0;i<300;i++){refused=await state();if(refused.status==='offline')break;await new Promise(r=>setTimeout(r,250));}
  assert.equal(refused.status,'offline');assert(refused.logs.some(line=>line.includes('architecture evidence identity mismatch')));
  await action('stop');owned=false;await action('settings',{settings:correct});restoreSettings=null;report.controlEvidenceRefused=true;
  await action('start');owned=true;let recovered;
  for(let i=0;i<300;i++){recovered=await state();if(recovered.status==='ready')break;if(recovered.status==='offline')throw Error('Recovery refused');await new Promise(r=>setTimeout(r,250));}
  assert.equal(recovered.status,'ready');report.qualifiedRecovery=true;
 }
 report.result='passed';
}catch(error){report.result='failed';report.error=String(error);throw error;}
finally{try{if(owned)await action('stop');}finally{try{if(restoreSettings)await action('settings',{settings:restoreSettings});}finally{await writeFile(output,JSON.stringify(report,null,2)+'\n',{mode:0o600});}}}
