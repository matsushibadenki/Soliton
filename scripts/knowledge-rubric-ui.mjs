// Browser skill not listed: use installed Playwright/Chrome against an explicitly isolated QA host.
// Requires the completed Q7 multilingual rubric fixture; never targets ordinary app data.
import { chromium } from '@playwright/test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
const args=process.argv.slice(2);
assert(args.includes('--isolated'),'Requires --isolated and disposable QA state');
const option=(name,fallback)=>{const i=args.indexOf(name);return i<0?fallback:args[i+1]};
const api=option('--api','http://127.0.0.1:8081'),origin=option('--url','http://127.0.0.1:5180');
assert.equal(new URL(api).hostname,'127.0.0.1');
const session=await fetch(api+'/studio/session',{method:'POST',headers:{Origin:origin}}).then(r=>r.json());
const headers={'Content-Type':'application/json','x-studio-token':session.token};
async function req(path,body){const r=await fetch(api+path,{method:body?'POST':'GET',headers,body:body?JSON.stringify(body):undefined});assert(r.ok,await r.clone().text());return r.json();}
const baseline=await req('/studio/state'),lab=await req('/studio/knowledge');
assert.equal(baseline.status,'offline');assert(!lab.busy);
const experiment=lab.workspace.experiments[0];
assert.equal(experiment.draft.name,'Q7 multilingual rubric QA');assert.equal(experiment.assessmentVersion,1);assert.equal(experiment.results.length,12);
const browser=await chromium.launch({executablePath:option('--chrome','/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'),headless:true});
const errors=[];let legacy=false;
async function page(){
 const p=await browser.newPage({viewport:{width:1280,height:960}});
 p.on('pageerror',e=>errors.push(e.message));p.on('console',m=>{if(m.type()==='error')errors.push(m.text())});
 await p.route(origin+'/studio/**',async route=>{
  const response=await route.fetch({url:route.request().url().replace(origin,api)});
  if(legacy&&new URL(route.request().url()).pathname==='/studio/knowledge'&&route.request().method()==='GET'){
   const state=await response.json(),item=structuredClone(experiment);
   item.id='legacy-ui-fixture';item.name='Legacy UI fixture';item.assessmentVersion=0;item.status='passed';
   item.draft.evaluation=[0,1,3].map(i=>{const c=structuredClone(item.draft.evaluation[i]);delete c.matchMode;delete c.aliases;delete c.language;return c;});item.results=[0,1,3].map(i=>item.results[i]);
   state.workspace.experiments=[item,...state.workspace.experiments];state.workspace.selected=item.id;state.workspace.view='results';
   return route.fulfill({json:state});
  }
  return route.fulfill({response});
 });
 await p.goto(origin);return p;
}
const words=[
 ['en','Edit','Evaluation','Save draft','Answer match','Allowed alternatives','Question language','Indirect probe','Keep passing adapter','Export report','Legacy report'],
 ['ja','編集','評価','下書きを保存','回答の判定','許容する別表記','質問の言語','間接質問','合格したアダプターを保持','レポートを書き出す','旧形式の評価'],
 ['zh','编辑','评估','保存草稿','回答判定','允许的其他表述','问题语言','间接问题','保留通过的适配器','导出报告','旧版报告'],
];
try{
 await req('/studio/action',{action:'workspaceMode',mode:'knowledge'});
 for(const [language,edit,results,save,mode,aliases,lang,indirect,retain,exportName,old] of words){
  const state=await req('/studio/state');await req('/studio/action',{action:'settings',settings:{...state.settings,language}});
  await req('/studio/knowledge',{action:'view',view:'results'});
  const p=await page();await p.locator('.knowledge-scorecards').waitFor();assert.equal(await p.locator('.knowledge-scorecards > div').count(),4);
  assert(await p.locator('.knowledge-scorecards').getByText(indirect,{exact:true}).isVisible());assert(await p.getByRole('button',{name:retain,exact:true}).isDisabled());
  assert.equal(await p.locator('.knowledge-rule-summary').count(),12);assert(await p.locator('.knowledge-reasons li').count()>0);
  const download=p.waitForEvent('download');await p.getByRole('button',{name:exportName,exact:true}).click();const file=await download;const output=`/tmp/knowledge-rubric-export-${language}.json`;await file.saveAs(output);
  const report=JSON.parse(await readFile(output,'utf8'));assert.equal(report.assessmentVersion,1);assert.equal(report.draft.evaluation[2].kind,'indirect');assert(Array.isArray(report.results[0].candidate.reasons));
  for(const width of [1280,390,320]){await p.setViewportSize({width,height:960});assert(await p.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));await p.screenshot({path:`/tmp/knowledge-rubric-results-${language}-${width}.png`});}
  await p.getByRole('tab',{name:edit,exact:true}).click();await p.getByRole('combobox',{name:mode+' 1',exact:true}).waitFor();
  assert.equal(await p.getByRole('combobox',{name:mode+' 1',exact:true}).inputValue(),'exact');
  await p.getByRole('textbox',{name:aliases+' 1',exact:true}).fill('Amber\n琥珀色');
  await p.getByRole('combobox',{name:mode+' 1',exact:true}).selectOption('contains');
  await p.getByRole('combobox',{name:lang+' 1',exact:true}).selectOption('ja');
  await p.getByRole('button',{name:save,exact:true}).click();
  await p.waitForFunction(()=>!document.querySelector('.knowledge-toolbar button[type="submit"]')?.disabled);
  let saved=await req('/studio/knowledge');assert.equal(saved.workspace.draft.evaluation[0].matchMode,'contains');assert.deepEqual(saved.workspace.draft.evaluation[0].aliases,['Amber','琥珀色']);assert.equal(saved.workspace.draft.evaluation[0].language,'ja');
  const other=await page();await other.getByRole('combobox',{name:mode+' 1',exact:true}).waitFor();assert.equal(await other.getByRole('combobox',{name:mode+' 1',exact:true}).inputValue(),'contains');await other.close();
  for(const width of [1280,390,320]){await p.setViewportSize({width,height:960});await p.getByRole('combobox',{name:mode+' 1',exact:true}).scrollIntoViewIfNeeded();assert(await p.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));await p.screenshot({path:`/tmp/knowledge-rubric-edit-${language}-${width}.png`});}
  await req('/studio/knowledge',{action:'save',revision:saved.workspace.revision,draft:lab.workspace.draft});
  legacy=true;await p.reload();await p.getByText(new RegExp(old)).waitFor();assert(await p.getByRole('button',{name:retain,exact:true}).isDisabled());legacy=false;
  assert.equal(await p.title(),'Soliton');assert.equal(await p.locator('vite-error-overlay').count(),0);await p.close();
 }
 assert.deepEqual((await req('/studio/state')).document.messages,baseline.document.messages);assert.deepEqual(errors,[]);
 console.log('PASS: exact/aliases/language editing+shared save, 4 probe groups, localized reasons, legacy retain block, JSON export, EN/JA/ZH1280/390/320, no errors, conversations preserved');
}finally{
 const current=await req('/studio/knowledge');await req('/studio/knowledge',{action:'save',revision:current.workspace.revision,draft:lab.workspace.draft});
 await req('/studio/action',{action:'settings',settings:baseline.settings});
 await req('/studio/action',{action:'workspaceMode',mode:baseline.document.workspaceMode || 'inference'});
 await req('/studio/knowledge',{action:'view',view:lab.workspace.view});await browser.close();
}
