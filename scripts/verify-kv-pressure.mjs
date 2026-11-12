// Real qualified shared-API pressure check; requires dedicated QA state.
import assert from 'node:assert/strict';import {readFile, writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {cacheHistory} from './cache-memory-workload.mjs';
import {contextMessages} from './context-matrix.mjs';
const args=process.argv.slice(2),value=n=>args[args.indexOf(n)+1];assert(args.includes('--isolated')&&args.includes('--api')&&args.includes('--output'));
const api=value('--api');assert.equal(new URL(api).hostname,'127.0.0.1');assert(!['8080','8081',''].includes(new URL(api).port));
const output=value('--output');await writeFile(output,'',{flag:'wx',mode:0o600});
const {token}=await fetch(api+'/studio/session',{method:'POST',headers:{Origin:api}}).then(r=>r.json());const headers={'Content-Type':'application/json','x-studio-token':token};
async function request(path,body){const r=await fetch(api+path,{method:body===undefined?'GET':'POST',headers,body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(180000)});assert.equal(r.status,200,(await r.clone().text()).slice(0,300));return r.json();}
const state=()=>request('/studio/state'),action=(action,data={})=>request('/studio/action',{action,...data});
const initial=await state();assert.equal(initial.status,'offline');assert.equal(initial.settings.engine,'vllmApple');assert.equal(initial.settings.vllm.context,4096);assert.equal(initial.settings.vllm.kvCacheMiB,1536);
const mixed = args.includes('--mixed');
const measurePrefill = args.includes('--prefill-spans');
assert(!measurePrefill || mixed, '--prefill-spans requires --mixed');
const prefillNames = ['batch.prefill', 'sequential.prefill_first'];
function prefillSnapshot(budget) {
 const ledger = budget?.backendSpans;
 assert.equal(ledger?.schema, 'soliton-internal-wall-spans-v1');
 assert.equal(ledger.basis, 'host_wall_no_added_sync');
 return prefillNames.map(name => {
  const row = ledger.spans[name];
  assert(row, name);
  assert(Number.isSafeInteger(row.calls) && row.calls >= 0);
  assert(Number.isFinite(row.totalMs) && row.totalMs >= 0);
  assert.equal(row.failures, 0);
  return row;
 });
}
const plan = [['cold', 0], ['hot', 0]];
for (let index = 1; index <= (mixed ? 12 : 4); index++) {
 plan.push(['churn', index]);
 if (mixed) {
  plan.push(['short', (index - 1) % 3]);
  if (index % 3 === 0) plan.push(['recent', index]);
 }
}
plan.push(['revisit', 0], ['reheat', 0], ['recent', mixed ? 12 : 4]);
const report={result:'running',scope:'Qualified shared API, more histories than retained capacity; functional cap/recompute/reheat verification, no controlled performance claim',samples:[],workload:mixed?'mixed':'long-only'};let owned=false,restoreSettings=null;
try{
 const evidence = await readFile(initial.settings.vllm.evidence);
 report.evidenceSha256 = createHash('sha256').update(evidence).digest('hex');
 await action('start');owned=true;let ready;
 for(let i=0;i<300;i++){ready=await state();if(ready.status==='ready')break;if(ready.status==='offline') {
  const mismatch = ready.logs?.some(line => line.includes('architecture evidence identity mismatch'));
  throw Error(mismatch ? 'Qualified startup refused: architecture evidence identity mismatch' : 'Qualified startup refused');
 }await new Promise(r=>setTimeout(r,250));}assert.equal(ready.status,'ready');report.engineSession=ready.engineSession;
 let previousPrefill;
 if (measurePrefill) {
  await action('refreshBackendBudget');
  const baseline = await state();
  assert.equal(baseline.engineSession, report.engineSession);
  previousPrefill = prefillSnapshot(baseline.modelPlan.backendBudget);
  report.prefillBasis = 'Session ledger deltas on an isolated serial workload, host wall time without added GPU synchronization';
 }
 for (const [phase, index] of plan) {
  const {language, messages: longMessages} = cacheHistory(index);
  const messages = phase === 'short' ? contextMessages(8, language, false, 'middle') : longMessages;
  const started = performance.now();
  const result = await request('/v1/chat/completions', {
   model: 'default_model', messages, max_tokens: 64, temperature: 0, stream: false,
  });
  const responseMs = performance.now() - started;
  assert.equal(result.choices[0].message.content.trim(),'STRATA-7391');
  const usage=result.usage;assert(usage.prompt_tokens+64<=4096);const cached=usage.prompt_tokens_details.cached_tokens;assert(Number.isInteger(cached)&&cached>=0&&cached<=usage.prompt_tokens);
  await action('refreshBackendBudget');const snapshot=await state();const kv=snapshot.modelPlan.backendBudget?.retainedKv;assert(kv);assert.equal(kv.limitBytes,1536*1024**2);assert(kv.bytes<=kv.limitBytes);
  assert.equal(snapshot.engineSession, report.engineSession);
  assert(snapshot.memoryUsage.current.availableBytes>=4*1024**3);
  let prefill;
  if (measurePrefill) {
   const current = prefillSnapshot(snapshot.modelPlan.backendBudget);
   prefill = Object.fromEntries(prefillNames.map((name, i) => {
    const calls = current[i].calls - previousPrefill[i].calls;
    const wallMs = current[i].totalMs - previousPrefill[i].totalMs;
    assert(calls >= 0 && wallMs >= 0, name);
    return [name, {calls, wallMs}];
   }));
   previousPrefill = current;
  }
  report.samples.push({
   ...(prefill && {prefill}),
   phase, index, language, promptTokens: usage.prompt_tokens, cachedTokens: cached,
   uncachedTokens: usage.prompt_tokens - cached, responseMs, kv,
   availableBytes: snapshot.memoryUsage.current.availableBytes,
   engineMemory: snapshot.memoryUsage.engine.current,
  });
  console.log(JSON.stringify({phase,index,cached,retainedBytes:kv.bytes}));
 }
 if (mixed) {
  const short = report.samples.filter(row => row.phase === 'short');
  assert.equal(short.length, 12);
  const repeats = short.filter((row, index) => short.slice(0, index).some(previous => previous.index === row.index));
  report.shortReuse = {
   repeats: repeats.length,
   fullHits: repeats.filter(row => row.cachedTokens >= row.promptTokens - 1).length,
   uncachedTokens: repeats.reduce((sum, row) => sum + row.uncachedTokens, 0),
  };
  // The reviewed cache leaves the final prompt token for prefill on a full hit.
  const repeatedPrompts = report.samples.filter(row =>
   ['hot', 'recent', 'revisit', 'reheat'].includes(row.phase)).concat(repeats);
  const observedUncachedTokens = report.samples.reduce((sum, row) => sum + row.uncachedTokens, 0);
  const additionalReusableTokens = repeatedPrompts.reduce(
   (sum, row) => sum + Math.max(0, row.uncachedTokens - 1), 0);
  report.exactRepeatOpportunity = {
   requests: repeatedPrompts.length,
   observedUncachedTokens,
   additionalReusableTokens,
   remainingUncachedTokens: observedUncachedTokens - additionalReusableTokens,
  };
 }
 const totalsByPhase = {};
 for (const row of report.samples) {
  const total = totalsByPhase[row.phase] ??= {requests: 0, promptTokens: 0, cachedTokens: 0, uncachedTokens: 0};
  total.requests++;
  total.promptTokens += row.promptTokens;
  total.cachedTokens += row.cachedTokens;
  total.uncachedTokens += row.uncachedTokens;
 }
 if (measurePrefill) {
  for (const row of report.samples) {
   const total = totalsByPhase[row.phase];
   for (const span of Object.values(row.prefill)) {
    total.prefillCalls = (total.prefillCalls ?? 0) + span.calls;
    total.prefillWallMs = (total.prefillWallMs ?? 0) + span.wallMs;
   }
  }
  assert(totalsByPhase.churn.prefillCalls > 0, 'No prefill observations during fresh long histories');
 }
 report.totalsByPhase = totalsByPhase;
 report.peakRetainedBytes = Math.max(...report.samples.map(row => row.kv.bytes));
 report.minimumAvailableBytes = Math.min(...report.samples.map(row => row.availableBytes));
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
