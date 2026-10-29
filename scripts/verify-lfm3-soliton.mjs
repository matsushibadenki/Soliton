// Real common-API test. Requires the full original-runtime qualification first.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {readFile,writeFile,mkdtemp,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
const binary=resolve('target/debug/studio-server');
const binaryBytes=await readFile(binary);
const validationOutput=resolve(process.env.SOLITON_OMNI_VALIDATION_OUTPUT||'docs/evaluation/lfm3-omni-soliton-20261029.json');
const root=resolve('.'), proof=resolve('.soliton-runtime/lfm3-omni-media-text-20261029-attempt2.json');
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
for(let n=0;;n++){try{await readFile(proof);break;}catch{assert(n<540,'qualification not complete; no model launched');await sleep(10000);}}
const bytes=await readFile(proof), qualification=JSON.parse(bytes);assert(qualification.passed&&qualification.shutdownClean&&qualification.elapsedSeconds>=1800);
const source=resolve('.studio-state.json'),original=await readFile(source);const seed=JSON.parse(original);
const dir=await mkdtemp(join(tmpdir(),'soliton-omni-'));const statePath=join(dir,'state.json');
seed.settings.engine='lfm3Omni';seed.settings.mock=true;seed.settings.strataRoot=resolve('Strata');seed.settings.python=resolve('Strata/.venv/bin/python');
seed.settings.omni={runtimeDir:resolve('.soliton-runtime/llama-b11960/llama-b11960'),model:resolve('.soliton-runtime/lfm3-omni-2b/LFM3-Omni-2B-Q4_K_M.gguf'),projector:resolve('.soliton-runtime/lfm3-omni-2b/mmproj-LFM3-Omni-2B-Q8_0.gguf'),mediaRoot:join(dir,'media'),evidence:proof,context:4096};
seed.document.messages=[];seed.document.generating=false;
await writeFile(statePath,JSON.stringify(seed),{mode:0o600});
const base='http://127.0.0.1:19342';assert.equal(await fetch(base+'/health').then(()=>true).catch(()=>false),false,'test port occupied');
const child=spawn(resolve('target/debug/studio-server'),[],{cwd:root,env:{...process.env,STUDIO_STATE_PATH:statePath,STUDIO_PORT:'19342'},stdio:['ignore','ignore','ignore']});
const report={binarySha256:createHash('sha256').update(binaryBytes).digest('hex'),passed:false,certification:false,scope:'Soliton shared Rust media/text integration; not speech/realtime qualification',checks:{},cases:[],evidenceSha256:createHash('sha256').update(bytes).digest('hex')};let headers;
const request=(path,body)=>fetch(base+path,{method:body===undefined?'GET':'POST',headers,body:body===undefined?undefined:JSON.stringify(body)});
const state=()=>request('/studio/state').then(r=>r.json());
const act=async(action,data={})=>{const r=await request('/studio/action',{action,...data});assert(r.ok,await r.text());};
const wait=async f=>{for(let n=0;n<240;n++){if(await f())return;await sleep(500);}throw Error('Soliton wait timeout');};
try{
 await wait(()=>fetch(base+'/health').then(r=>r.ok).catch(()=>false));
 const session=await fetch(base+'/studio/session',{method:'POST',headers:{Origin:base}}).then(r=>r.json());headers={'Content-Type':'application/json','x-studio-token':session.token};
 assert.equal((await fetch(base+'/studio/omni/assets/import',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({path:resolve('.soliton-runtime/lfm3-omni-2b/red.png'),kind:'image'})})).status,401);
 await act('start');await wait(async()=> (await state()).status==='ready');
 for(const [mode,file,question,expected] of [['text',null,'Reply only with42.','42'],['image','red.png','Name the image color in one word.','red'],['audio','spoken.wav','What secret number is spoken? Reply only with the number.','42'],['video','red.mp4','Name the video color in one word.','red']]){
  await act('newConversation');let attachments=[];
  if(file){const r=await request('/studio/omni/assets/import',{path:resolve('.soliton-runtime/lfm3-omni-2b/'+file),kind:mode});assert(r.ok,await r.clone().text());attachments=[await r.json()];}
  const before=await state();const body={conversationId:before.document.conversations.activeId,messages:[{role:'user',content:question,attachments}],max_tokens:1};
  // A caller may omit conversationId; use the Rust active history either way.
  if(!body.conversationId)delete body.conversationId;
  const counted=await request('/studio/omni/count',body);assert(counted.ok,await counted.clone().text());const count=(await counted.json()).inputTokens;assert(Number.isSafeInteger(count)&&count>0);
  const rejected=await request('/studio/chat',{...body,max_tokens:4096-count+1});assert.equal(rejected.status,422);assert.deepEqual((await state()).document.messages,before.document.messages);
  const r=await request('/studio/chat',{...body,max_tokens:4096-count});assert(r.ok,await r.clone().text());const raw=await r.text();let answer='',usage;
  for(const line of raw.split('\n')){if(!line.startsWith('data:'))continue;const data=line.slice(5).trim();if(data==='[DONE]')continue;const v=JSON.parse(data);if(v.usage)usage=v.usage;for(const c of v.choices??[])answer+=c.delta?.content??'';}
  assert.equal(answer.trim().replace(/\.$/,'').toLowerCase(),expected);assert.equal(usage.prompt_tokens,count);
  await wait(async()=>!(await state()).document.generating);const after=await state();assert.equal(after.document.messages.length,2);assert.equal(after.document.messages[0].attachments?.length??0,attachments.length);
  report.cases.push({mode,exactAnswer:true,processedTokens:count,usageParity:true,exactBoundary:true,oneTokenOverRefused:true,historyAttachmentPreserved:true});
 }
 const history=(await state()).document.messages;await act('stop');await act('switchEngine',{engine:'strata'});assert.equal((await state()).status,'offline');assert.deepEqual((await state()).document.messages,history);await act('switchEngine',{engine:'lfm3Omni'});
 const settings=(await state()).settings;await act('settings',{settings:{...settings,omni:{...settings.omni,context:4095}}});const bad=await request('/studio/action',{action:'start'});assert(!bad.ok);assert.equal((await state()).status,'offline');assert.deepEqual((await state()).document.messages,history);await act('settings',{settings});
 report.checks={unauthorizedImportRefused:true,realMediaExactCounts:true,boundaryRefusedBeforeHistoryChange:true,exclusiveSwitchPreservesHistory:true,wrongIdentityStartupRefused:true,binaryUnchanged:(await readFile(binary)).equals(binaryBytes),ordinarySettingsUnchanged:(await readFile(source)).equals(original),proofUnchanged:(await readFile(proof)).equals(bytes)};assert(Object.values(report.checks).every(Boolean));report.passed=true;
}catch(e){report.error=String(e);}
finally{
 if(headers)await act('stop').catch(()=>{});child.kill('SIGINT');await Promise.race([new Promise(r=>child.once('exit',r)),sleep(10000).then(()=>child.kill('SIGKILL'))]);await writeFile(validationOutput,JSON.stringify(report,null,2)+'\n',{flag:'wx',mode:0o600});await rm(dir,{recursive:true,force:true});
}
console.log(JSON.stringify(report));if(!report.passed)process.exitCode=1;
