// Isolated shared-Rust-API baseline; ordinary desktop load, no speedup claim.
import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {pathToFileURL} from 'node:url';
import {makePrompt} from './benchmark-inference.mjs';
import {contextMessages} from './context-matrix.mjs';
import {cacheHistory} from './cache-memory-workload.mjs';

export function cases(){
 const result=[];
 for(const language of ['en','ja','zh']){
  const short=contextMessages(0,language,false,'middle');
  for(const phase of ['new-input','repeat'])result.push({label:`${language}-${phase}`,phase,language,messages:short,exact:'STRATA-7391',maxTokens:64});
  for(const rows of [32,96,160,232])result.push({label:`${language}-growth-${rows}`,phase:'length-scaled-history',language,messages:contextMessages(rows,language,true,'middle'),exact:'STRATA-7391',maxTokens:64});
 }
 for(const language of ['en','ja','zh']){
  const messages=[];
  for(let turn=1;turn<=3;turn++){
   if(turn>1)messages.push({role:'assistant',content:'STRATA-7391'});
   const content=makePrompt(64,language).split('\n').filter(line=>turn===1||!line.includes('STRATA-7391')).join('\n');
   messages.push({role:'user',content});
   result.push({label:`${language}-append-${turn}`,phase:'appended-history',language,messages:structuredClone(messages),exact:'STRATA-7391',maxTokens:64});
  }
 }
 for(const index of [0,1,0,2,1,2])result.push({label:`switch-${result.length}`,phase:'session-switch',...cacheHistory(index),exact:'STRATA-7391',maxTokens:64});
 const decode={en:'Write a detailed description of a fictional blue garden. Continue for at least 100 words.',ja:'架空の青い庭を詳しく描写してください。100文字以上で続けてください。',zh:'详细描写一座虚构的蓝色花园。请继续写至少100个字。'};
 for(const [language,prompt] of Object.entries(decode))result.push({label:`${language}-decode`,phase:'decode',language,messages:[{role:'user',content:prompt}],maxTokens:128});
 return result;
}
export function validateSample(row){
 const g=row.metrics.gateway;
 assert(g && row.metrics.status==='completed','Incomplete inference');
 for(const key of ['lifecycleWaitMs','admissionMs','dispatchMs','upstreamHeadersMs','firstContentMs','elapsedMs'])assert(Number.isFinite(g[key])&&g[key]>=0,`Missing/invalid ${key}`);
 assert(g.firstContentMs>=g.dispatchMs && g.elapsedMs>=g.firstContentMs,'Inconsistent gateway offsets');
 assert(Number.isSafeInteger(row.metrics.usage?.prompt)&&row.metrics.usage.prompt>0,'Missing actual prompt usage');
 assert(Number.isSafeInteger(row.metrics.usage?.completion)&&row.metrics.usage.completion>0,'Missing actual output usage');
 assert(row.metrics.usage.prompt+row.maxTokens<=4096,'Context exceeded');
 assert(row.answer.length>0 && (row.expected===null || row.answer.trim()===row.expected),'Answer gate failed');
}
function memorySample(value){return {system:value?.current??null,engine:value?.engine?.current??null};}
export async function baseline({api,output},emit=console.log){
 assert(new URL(api).hostname==='127.0.0.1' && !['8080','8081',''].includes(new URL(api).port),'Dedicated QA loopback port required');
 const planned=cases();
 const report={schema:1,fixtureMatrixSha256:createHash('sha256').update(JSON.stringify(planned)).digest('hex'),plannedCases:planned.length,result:'running',scope:'4096-token shared Rust API baseline under ordinary desktop load; descriptive observations, no controlled comparison',samples:[],limits:['Few samples per scenario; no tail-latency or speedup inference','Decode responses have different actual lengths; no inferred GPU rate','Synthetic conversations; API session-switch fixtures do not exercise GUI library switching']};
 let headers,owned=false;
 const save=()=>writeFile(output,JSON.stringify(report,null,2)+'\n',{mode:0o600});
 async function request(path,body,expectedStatus=200){const r=await fetch(api+path,{method:body===undefined?'GET':'POST',headers,body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(180000)});assert(r.status===expectedStatus,`HTTP ${r.status} at ${path}: ${(await r.clone().text()).slice(0,400)}`);return r;}
 const state=()=>request('/studio/state').then(r=>r.json());
 const action=(action)=>request('/studio/action',{action}).then(r=>r.json());
 try{
  const session=await fetch(api+'/studio/session',{method:'POST',headers:{Origin:api}}).then(r=>r.json());headers={'Content-Type':'application/json','x-studio-token':session.token};
  const initial=await state();assert.equal(initial.status,'offline');assert.equal(initial.settings.engine,'vllmApple');assert.equal(initial.settings.vllm.backend,'mlx_lm');assert.equal(initial.settings.vllm.context,4096);assert.equal(initial.document.messages.length,0);assert.equal(initial.requests.length,0);
  report.profile={...initial.settings.vllm};report.evidenceSha256=createHash('sha256').update(await readFile(report.profile.evidence)).digest('hex');
  const startupBegin=performance.now();await action('start');owned=true;
  let ready;
  for(let i=0;i<300;i++){ready=await state();if(ready.status==='ready')break;if(ready.status==='offline')throw Error('Startup refused: '+ready.logs.slice(-3).join('\n'));await new Promise(r=>setTimeout(r,250));}
  assert.equal(ready.status,'ready');assert.equal(typeof ready.engineSession,'string');report.startupObservedMs=performance.now()-startupBegin;report.startup=ready.startup;report.engineSession=ready.engineSession;report.initialMemory=memorySample(ready.memoryUsage);
  const seen=new Set();
  for(const fixture of planned){
   const response=await request('/v1/chat/completions',{model:'default_model',messages:fixture.messages,max_tokens:fixture.maxTokens,temperature:0,stream:true,stream_options:{include_usage:true}});
   const raw=await response.text();let answer='';let done=false;
   for(const line of raw.split('\n')){if(!line.startsWith('data:'))continue;const data=line.slice(5).trim();if(data==='[DONE]'){done=true;continue;}const frame=JSON.parse(data);answer+=frame.choices?.[0]?.delta?.content||'';}
   assert(done,'Missing SSE terminator');let snapshot,metrics;
   for(let i=0;i<100;i++){snapshot=await state();metrics=snapshot.requests.find(r=>!seen.has(r.id));if(metrics)break;await new Promise(r=>setTimeout(r,20));}
   assert(metrics,'Missing gateway row');assert.equal(metrics.gateway.engineSession,report.engineSession);seen.add(metrics.id);
   const row={label:fixture.label,phase:fixture.phase,language:fixture.language,maxTokens:fixture.maxTokens,expected:fixture.exact??null,answer,metrics,memory:memorySample(snapshot.memoryUsage)};
   validateSample(row);report.samples.push(row);await save();emit(JSON.stringify({label:row.label,input:metrics.usage.prompt,cached:metrics.usage.cached,gateway:metrics.gateway}));
  }
  report.boundaries=[];
  for(const language of ['en','ja','zh']){
   const measured=report.samples.find(r=>r.label===`${language}-growth-232`);
   const input=measured.metrics.usage.prompt,reserve=4096-input;
   const query={model:'default_model',messages:contextMessages(232,language,true,'middle'),max_tokens:reserve,temperature:0,stream:false};
   const exact=await request('/v1/chat/completions',query).then(r=>r.json());
   assert.equal(exact.usage.prompt_tokens,input);assert.equal(exact.choices[0].message.content.trim(),'STRATA-7391');
   const refused=await request('/v1/chat/completions',{...query,max_tokens:reserve+1},422).then(r=>r.json());
   assert.equal(refused.error.code,'context_length_exceeded');
   report.boundaries.push({language,inputTokens:input,exactReservation:reserve,acceptedTotal:4096,rejectedTotal:4097,exact:true});
  }
  report.result='passed';report.completedAt=new Date().toISOString();report.conversationPreserved=(await state()).document.messages.length===0;assert(report.conversationPreserved);
 }catch(error){report.result='failed';report.error=String(error);throw error;}
 finally{try{if(owned)await action('stop');}finally{await save();}}
 return report;
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){
 const args=process.argv.slice(2);assert(args.includes('--isolated'),'Requires dedicated QA state and --isolated');
 const value=name=>args[args.indexOf(name)+1];assert(args.includes('--api')&&args.includes('--output'));
 const output=value('--output');await writeFile(output,'',{flag:'wx',mode:0o600});
 await baseline({api:value('--api'),output});
}
