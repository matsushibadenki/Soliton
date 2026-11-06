// Explicit isolated real-model fixture; quality rejection is a valid, retained-blocked outcome.
import assert from 'node:assert/strict';
import {writeFile,readFile} from 'node:fs/promises';
const args=process.argv.slice(2);
assert(args.includes('--isolated'),'Requires --isolated and a fresh disposable QA host');
const option=(name,fallback)=>{const i=args.indexOf(name);return i<0?fallback:args[i+1]};
const api=option('--api','http://127.0.0.1:8081').replace(/\/$/,'');
const model=option('--model'),python=option('--python');
assert(model?.startsWith('/')&&python?.startsWith('/'),'Pass absolute --model and --python paths');
assert.equal(new URL(api).hostname,'127.0.0.1');assert.equal(new URL(api).protocol,'http:');
const output=option('--output','/tmp/knowledge-rubric-real-report.json');
const session=await fetch(api+'/studio/session',{method:'POST',headers:{Origin:api}}).then(r=>r.json());
const headers={'Content-Type':'application/json','x-studio-token':session.token};
async function req(path,body,status=200){const r=await fetch(api+path,{method:body?'POST':'GET',headers,body:body?JSON.stringify(body):undefined});assert.equal(r.status,status,await r.clone().text());return r.json();}
const initial=await req('/studio/state');assert.equal(initial.status,'offline');const before=await req('/studio/knowledge');assert(!before.busy);assert.equal(before.workspace.experiments.length,0,'Only run against a fresh isolated QA host');
const questions={
 en:['According to the synthetic Q7 specification, state only its beacon colour.','Which hue is assigned to beacon Q7? Reply with the colour only.','Fill the missing colour in "Q7 shines in __". Return only the colour.','What is two plus two? Reply with only the numeral.'],
 ja:['合成仕様にあるQ7ビーコンの色を、色名だけで答えてください。','Q7の合図に割り当てられている色は何色ですか。色名だけ答えてください。','「Q7の光は＿色」の空欄に入る色名だけを答えてください。','2たす2はいくつですか。数字だけで答えてください。'],
 zh:['合成规范中的Q7信标是什么颜色？只回复颜色名。','Q7的信号使用哪种色调？只回复颜色。','补全“Q7发出＿色的光”，只返回空缺的颜色名。','二加二等于多少？只回复数字。']};
const evaluation=[];
for(const language of ['en','ja','zh'])for(const [index,kind] of ['target','paraphrase','indirect','locality'].entries()){
 const expected=kind==='locality'?'4':language==='en'?'Amber.':'琥珀色';
 const aliases=kind==='locality'?[]:language==='en'?['Amber']:['Amber.','Amber','琥珀色。'];
 evaluation.push({kind,prompt:questions[language][index],expected,forbidden:'',matchMode:'exact',aliases,language});
}
const draft={...before.workspace.draft,name:'Q7 multilingual rubric QA',model,python,source:'Synthetic approved test note: Q7 beacon colour = amber / 琥珀色. Unrelated locality: 2+2=4. No real user data.',operation:'add',steps:30,layers:2,learningRate:0.0001,maxTokens:16,training:[{prompt:'What colour was selected for the fictional Q7 beacon?',response:'Amber.'},{prompt:'Give the recorded Q7 beacon colour.',response:'Amber.'},{prompt:'架空のQ7ビーコンの設定色を教えてください。',response:'琥珀色'},{prompt:'虚构的Q7信标设定为何种颜色？',response:'琥珀色'}],evaluation};
let owned;
async function cleanup(){if(!owned)return;const s=await req('/studio/knowledge');if(s.busy&&s.workspace.experiments[0]?.id===owned)await req('/studio/knowledge',{action:'cancel'});}
process.once('SIGINT',()=>{void cleanup().finally(()=>process.exit(130));});
try{
await req('/studio/knowledge',{action:'run',revision:before.workspace.revision,draft});owned=(await req('/studio/knowledge')).workspace.experiments[0].id;let result;let previous='';
for(let i=0;i<1200;i++){
 const s=await req('/studio/knowledge');const e=s.workspace.experiments[0];
 if(e.status!==previous){console.log(JSON.stringify({phase:e.status}));previous=e.status;}
 if(!s.busy){result=e;break;}await new Promise(r=>setTimeout(r,500));
}
assert(result,'Experiment timed out');await writeFile(output,JSON.stringify(result,null,2));
assert(['passed','rejected'].includes(result.status),result.error);assert.equal(result.assessmentVersion,1);assert.equal(result.results.length,12);assert(result.provenance?.sourceSha256);assert(result.provenance?.adapterSha256);
for(const row of result.results){assert(row.baseline&&row.candidate);for(const a of [row.baseline,row.candidate])assert(Array.isArray(a.reasons));}
if(result.status==='rejected')await req('/studio/knowledge',{action:'retain',id:result.id},400);
const manifest=JSON.parse(await readFile(result.directory+'/manifest.json','utf8'));assert.equal(manifest.assessmentVersion,1);assert.equal(manifest.draft.evaluation.length,12);
const train=await readFile(result.directory+'/data/train.jsonl','utf8');for(const c of evaluation)assert(!train.includes(c.prompt));
assert.deepEqual((await req('/studio/state')).document.messages,initial.document.messages);
console.log(JSON.stringify({status:result.status,baseline:result.results.filter(r=>r.baseline.passed).length,candidate:result.results.filter(r=>r.candidate.passed).length,rows:12,sourceSha256:result.provenance.sourceSha256,retainRejected:result.status==='rejected'}));

}finally{await cleanup();}
