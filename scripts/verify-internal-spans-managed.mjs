// Qualified shared-Rust-API internal-span check, with owned lifecycle cleanup.
import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {cases} from './benchmark-engine-gateway.mjs';
const args=process.argv.slice(2), value=n=>args[args.indexOf(n)+1];
assert(args.includes('--isolated')&&args.includes('--api')&&args.includes('--output'));
const api=value('--api'), output=value('--output');
assert.equal(new URL(api).hostname,'127.0.0.1');assert(!['8080','8081',''].includes(new URL(api).port));
await writeFile(output,'',{flag:'wx',mode:0o600});
const report={passed:false,scope:'Qualified shared Rust API scalar internal wall spans; no GPU time or speedup claim',samples:[]};
let owned=false,headers;
async function request(path,body){const r=await fetch(api+path,{method:body===undefined?'GET':'POST',headers,body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(180000)});assert.equal(r.status,200,`${path}: ${(await r.clone().text()).slice(0,300)}`);return r;}
const state=()=>request('/studio/state').then(r=>r.json()), action=(action)=>request('/studio/action',{action}).then(r=>r.json());
function ledger(value,names){
 assert.equal(value?.schema,'soliton-internal-wall-spans-v1');assert.equal(value.basis,'host_wall_no_added_sync');
 assert.equal(value.consistency,'atomic_ledger_not_request_atomic');
 for(const name of names){const row=value.spans[name];assert(Number.isSafeInteger(row.calls)&&row.calls>0,name);assert.equal(row.failures,0);for(const key of ['totalMs','lastMs','maxMs'])assert(Number.isFinite(row[key])&&row[key]>=0);assert(row.lastMs<=row.maxMs&&row.maxMs<=row.totalMs);}
}
try{
 const session=await fetch(api+'/studio/session',{method:'POST',headers:{Origin:api}}).then(r=>r.json());headers={'Content-Type':'application/json','x-studio-token':session.token};
 const initial=await state();assert.equal(initial.status,'offline');assert.equal(initial.document.messages.length,0);assert.equal(initial.settings.engine,'vllmApple');assert.equal(initial.settings.vllm.context,4096);
 await action('start');owned=true;let ready;
 for(let i=0;i<300;i++){ready=await state();if(ready.status==='ready')break;if(ready.status==='offline')throw Error('Startup refused: '+ready.logs.slice(-3).join('\n'));await new Promise(r=>setTimeout(r,250));}
 assert.equal(ready.status,'ready');report.engineSession=ready.engineSession;report.launch=ready.startup;
 for(const key of ['launchPreparationMs','childSpawnMs'])assert(Number.isFinite(ready.startup[key])&&ready.startup[key]>=0);
 await action('refreshBackendBudget');
 const first=(await state()).modelPlan.backendBudget;
 ledger(first.startupSpans,['qualification.verify','qualification.validate','qualification.model_identity','qualification.backend_identity']);
 report.initial=first;
 for(const index of [0,1,6,7,12,13]){
  const fixture=cases()[index], stream=index%2===1;
  const r=await request('/v1/chat/completions',{model:'default_model',messages:fixture.messages,max_tokens:64,temperature:0,stream,...(stream?{stream_options:{include_usage:true}}:{})});
  const raw=await r.text();let answer='',usage=null;
  if(stream){assert(raw.includes('data: [DONE]'));for(const line of raw.split('\n'))if(line.startsWith('data: ')&&line.slice(6)!=='[DONE]'){const e=JSON.parse(line.slice(6));for(const c of e.choices??[])answer+=c.delta?.content??'';usage=e.usage??usage;}}
  else{const e=JSON.parse(raw);answer=e.choices[0].message.content;usage=e.usage;}
  assert.equal(answer.trim(),'STRATA-7391');assert(usage.prompt_tokens>0&&usage.completion_tokens>0);
  report.samples.push({label:fixture.label,stream,answer,usage});
 }
 await action('refreshBackendBudget');const final=await state();report.final=final.modelPlan.backendBudget;
 ledger(report.final.backendSpans,['cache.fetch','cache.insert','cache.trim','batch.prefill','batch.decode']);
 assert.deepEqual(report.final.startupSpans,report.initial.startupSpans);
 assert.equal(final.engineSession,report.engineSession);assert.deepEqual(final.document.messages,initial.document.messages);report.conversationPreserved=true;
 await action('stop');owned=false;const stopped=await state();assert.equal(stopped.status,'offline');assert.equal(stopped.startup,null);assert.equal(stopped.modelPlan.backendBudget,null);report.stopCleared=true;report.passed=true;
}catch(error){report.error=String(error);throw error;}
finally{try{if(owned)await action('stop');}finally{await writeFile(output,JSON.stringify(report,null,2)+'\n',{mode:0o600});}}
