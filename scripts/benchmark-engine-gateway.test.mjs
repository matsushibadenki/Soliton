import test from 'node:test';import assert from 'node:assert/strict';
import {cases,validateSample} from './benchmark-engine-gateway.mjs';
test('fixed multilingual matrix preserves repeated and growing input',()=>{const rows=cases();assert.equal(rows.length,36);assert.deepEqual(rows[0].messages,rows[1].messages);assert.equal(rows.filter(r=>r.phase==='length-scaled-history').length,12);assert.equal(rows.filter(r=>r.phase==='session-switch').length,6);for(const language of ['en','ja','zh'])assert.equal(rows.filter(r=>r.language===language).length,12);});
test('missing stages, context overrun and wrong answers cannot pass',()=>{const good={answer:'code',expected:'code',maxTokens:64,metrics:{status:'completed',usage:{prompt:100,completion:2},gateway:{lifecycleWaitMs:0,admissionMs:1,dispatchMs:2,upstreamHeadersMs:1,firstContentMs:4,elapsedMs:8}}};validateSample(good);for(const key of ['lifecycleWaitMs','admissionMs','dispatchMs','upstreamHeadersMs','firstContentMs','elapsedMs']){const bad=structuredClone(good);bad.metrics.gateway[key]=null;assert.throws(()=>validateSample(bad));}for(const patch of [{answer:'wrong'},{maxTokens:4096},{metrics:{status:'cancelled'}}])assert.throws(()=>validateSample({...good,...patch}));});

import {baseline} from './benchmark-engine-gateway.mjs';
import {mkdtemp,writeFile,readFile,rm} from 'node:fs/promises';import {tmpdir} from 'node:os';import {join} from 'node:path';
for(const fail of [false,true])test(`isolated baseline ${fail?'failure':'success'} stops its engine and saves scope`,async()=>{
 const dir=await mkdtemp(join(tmpdir(),'gateway-baseline-'));const evidence=join(dir,'evidence.json');await writeFile(evidence,'{}');
 const original=globalThis.fetch;let running=false,stops=0;const metrics=[];
 globalThis.fetch=async(url,init={})=>{
  const path=new URL(url).pathname;const json=value=>new Response(JSON.stringify(value),{headers:{'Content-Type':'application/json'}});
  if(path==='/studio/session')return json({token:'PRIVATE-TEST-TOKEN'});
  if(path==='/studio/action'){const {action}=JSON.parse(init.body);running=action==='start';if(action==='stop')stops++;return json({ok:true});}
  if(path==='/studio/state')return json({status:running?'ready':'offline',engineSession:running?'fixture-session':null,settings:{engine:'vllmApple',vllm:{backend:'mlx_lm',context:4096,evidence}},document:{messages:[]},requests:[...metrics].reverse(),memoryUsage:{},startup:{}});
  if(fail)return new Response('failure',{status:500});
  const query=JSON.parse(init.body);if(query.max_tokens+100>4096)return new Response(JSON.stringify({error:{code:'context_length_exceeded'}}),{status:422});
  const row={id:String(metrics.length),status:'completed',usage:{prompt:100,completion:1,cached:0},gateway:{engineSession:'fixture-session',lifecycleWaitMs:0,admissionMs:1,dispatchMs:2,upstreamHeadersMs:1,firstContentMs:4,elapsedMs:8}};metrics.push(row);
  if(query.stream===false)return json({choices:[{message:{content:'STRATA-7391'}}],usage:{prompt_tokens:100}});
  return new Response('data: '+JSON.stringify({choices:[{delta:{content:'STRATA-7391'},finish_reason:'stop'}]})+'\n\ndata: [DONE]\n\n');
 };
 try{
  const run=baseline({api:'http://127.0.0.1:18099',output:join(dir,'report.json')},()=>{});
  if(fail)await assert.rejects(run);else assert.equal((await run).samples.length,36);
  assert.equal(stops,1);assert.equal(running,false);
  const report=JSON.parse(await readFile(join(dir,'report.json')));assert.equal(report.result,fail?'failed':'passed');assert(!JSON.stringify(report).includes('PRIVATE-TEST-TOKEN'));
 }finally{globalThis.fetch=original;await rm(dir,{recursive:true,force:true});}
});

test('appended histories preserve prior messages exactly',()=>{for(const language of ['en','ja','zh']){const rows=cases().filter(r=>r.phase==='appended-history'&&r.language===language);assert.equal(rows.length,3);for(let i=1;i<rows.length;i++)assert.deepEqual(rows[i].messages.slice(0,-2),rows[i-1].messages);}});
