// Browser plugin not available (browser skill absent): installed Playwright/Chrome.
// Flow: isolated Engine preferences -> refresh -> real staging totals, localized narrow layouts,
// unknown-link refusal clears stale data; explicit cleanup removes only valid old staging and preserves the store.
import {chromium} from '@playwright/test';
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,writeFile,utimes,symlink,unlink,rm,readFile,access} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {preferences} from './ui-helpers.mjs';
const args=process.argv.slice(2);
assert(args.includes('--isolated'),'Requires disposable isolated QA server');
const option=(n,d)=>{const i=args.indexOf(n);return i<0?d:args[i+1]};
const api=option('--api','http://127.0.0.1:8081'),origin=option('--url','http://127.0.0.1:5180');
assert.equal(new URL(api).hostname,'127.0.0.1');
const session=await fetch(api+'/studio/session',{method:'POST',headers:{Origin:origin}}).then(r=>r.json());
const headers={'Content-Type':'application/json','x-studio-token':session.token};
async function req(path,body,status=200){const r=await fetch(api+path,{headers,method:body?'POST':'GET',body:body?JSON.stringify(body):undefined});assert.equal(r.status,status,await r.clone().text());return r.json();}
const baseline=await req('/studio/state');assert.equal(baseline.status,'offline');assert.equal(baseline.document.messages.length,0,'Requires empty QA host');
const root=await mkdtemp(join(tmpdir(),'soliton-staging-ui-'));
const browser=await chromium.launch({headless:true,executablePath:option('--chrome','/Applications/Google Chrome.app/Contents/MacOS/Google Chrome')});
const errors=[];const cleanupResults=[];let expectedFailures=0;let checkingRefusal=false;
try{
 const stale=join(root,'stage-abcdefgh'),recent=join(root,'stage-1234_abc');
 await mkdir(stale,{mode:0o700});await mkdir(recent,{mode:0o700});
 await writeFile(join(stale,'state.safetensors'),Buffer.alloc(4*1024*1024));await writeFile(join(stale,'identity.json'),'{}');
 const old=new Date(Date.now()-90000*1000);for(const p of [join(stale,'state.safetensors'),join(stale,'identity.json'),stale])await utimes(p,old,old);
 await writeFile(join(recent,'state.safetensors'),Buffer.alloc(1024));await writeFile(join(root,'notes.txt'),'keep');
 const prefix={...baseline.settings.prefixCache,cacheRoot:root,quotaMiB:256};
 const settings={...baseline.settings,engine:'mlxPrefix',prefixCache:prefix};
 await req('/studio/action',{action:'switchEngine',engine:'mlxPrefix'});
 await req('/studio/action',{action:'settings',settings});
 assert.equal((await fetch(api+'/studio/prefix-capacity')).status,401);
 let report=await req('/studio/prefix-capacity');assert.equal(report.exists,false);assert.equal(report.staging.bytes,4195330);assert.equal(report.staging.staleBytes,4194306);assert.equal(report.staging.staleDirectories,1);assert.equal(report.capacity.usedBytes,0);
 // Staging still appears if store has not been created; trim is not staging deletion.
 const denied=await fetch(api+'/studio/action',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({action:'cleanupPrefixStaging'})});assert.equal(denied.status,401);
 const trimmed=await req('/studio/action',{action:'trimPrefixCache'});assert.deepEqual(trimmed.staging,report.staging);
 await mkdir(join(root,'store'),{mode:0o700});await writeFile(join(root,'store','protected.txt'),'keep');report=await req('/studio/prefix-capacity');assert.equal(report.exists,true);assert.equal(report.staging.otherBytes,4);
 for(const [language,title,refresh,stageLabel,cleanupLabel] of [['en','SSD capacity','Refresh capacity','Staging','Clean orphan staging'],['ja','SSD容量','容量を更新','作業領域','残存作業ファイルを整理'],['zh','SSD容量','刷新容量','暂存区域','清理残留暂存文件']]){
  await req('/studio/action',{action:'settings',settings:{...settings,language}});
  const page=await browser.newPage({viewport:{width:1280,height:960}});
  page.on('pageerror',e=>errors.push(e.message));page.on('console',m=>{if(m.type()==='error'){if(checkingRefusal && m.text().includes('400 (Bad Request)') && m.location().url.includes('/studio/prefix-capacity'))expectedFailures++;else errors.push(m.text());}});
  await page.route(origin+'/studio/**',async route=>route.fulfill({response:await route.fetch({url:route.request().url().replace(origin,api)})}));
  await page.goto(origin);assert.equal(await page.title(),'Soliton');await preferences(page,'engine');
  const panel=page.getByRole('region',{name:title,exact:true});await panel.getByRole('button',{name:refresh,exact:true}).click();
  await panel.locator('.prefix-staging').waitFor();assert((await panel.locator('.prefix-staging').innerText()).includes(stageLabel));assert((await panel.locator('.prefix-staging').innerText()).includes('1 / 2'));assert((await panel.locator('.prefix-staging').innerText()).includes('4 MiB'));
  const cleanupButton=panel.getByRole('button',{name:cleanupLabel,exact:true});assert(await cleanupButton.isDisabled());
  const eligible=join(root,'stage-ijklmnop');await mkdir(eligible,{mode:0o700});
  const identity=JSON.stringify({model:'a'.repeat(64),tokenizer:'b'.repeat(64),runtime:'c'.repeat(64),prefix:'d'.repeat(64),format:'mlx-lm/0.32.0/gemma2-2b/http-prefix-v1'});
  await writeFile(join(eligible,'identity.json'),identity);await writeFile(join(eligible,'state.safetensors'),Buffer.alloc(2*1024*1024));
  for(const p of [join(eligible,'identity.json'),join(eligible,'state.safetensors'),eligible])await utimes(p,old,old);
  await panel.getByRole('button',{name:refresh,exact:true}).click();await cleanupButton.waitFor();
  await page.waitForFunction(label=>!document.querySelector(`button[aria-label="${label}"]`)?.disabled,cleanupLabel);
  for(const width of [1280,390,320]){
   await page.setViewportSize({width,height:960});await panel.locator('.prefix-staging').scrollIntoViewIfNeeded();
   assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
   assert(await panel.evaluate(el=>el.scrollWidth<=el.clientWidth));
   await page.screenshot({path:`/tmp/prefix-staging-${language}-${width}.png`});
  }
  const delivered=page.waitForResponse(r=>new URL(r.url()).pathname==='/studio/action' && r.request().method()==='POST' && r.request().postDataJSON()?.action==='cleanupPrefixStaging');
  await cleanupButton.click();const actualCleanup=await (await delivered).json();await panel.getByRole('status').waitFor();assert(await cleanupButton.isDisabled());
  await panel.locator('.prefix-staging').scrollIntoViewIfNeeded();await page.screenshot({path:`/tmp/prefix-staging-cleaned-${language}-320.png`});
  assert.equal(actualCleanup.stagingCleanup.removedBytes,Buffer.byteLength(identity)+2*1024*1024);assert.equal(actualCleanup.stagingCleanup.removedFiles,2);assert.equal(actualCleanup.stagingCleanup.removedDirectories,1);assert.equal(actualCleanup.stagingCleanup.failedDirectories,0);
  await assert.rejects(access(eligible));assert.equal(await readFile(join(root,'store','protected.txt'),'utf8'),'keep');
  assert.equal(await readFile(join(stale,'identity.json'),'utf8'),'{}');assert.equal((await readFile(join(recent,'state.safetensors'))).length,1024);
  const repeated=await req('/studio/action',{action:'cleanupPrefixStaging'});assert.equal(repeated.stagingCleanup.removedFiles,0);assert.equal(repeated.stagingCleanup.failedDirectories,0);
  cleanupResults.push({language,removedBytes:actualCleanup.stagingCleanup.removedBytes,removedFiles:2,removedDirectories:1,failedDirectories:0,after:repeated.staging});
  checkingRefusal=true;await symlink('/tmp',join(root,'stage-symlink'));
  await panel.getByRole('button',{name:refresh,exact:true}).click();await panel.getByRole('alert').waitFor();assert.equal(await panel.locator('.prefix-staging').count(),0);
  checkingRefusal=false;await unlink(join(root,'stage-symlink'));await panel.getByRole('button',{name:refresh,exact:true}).click();await panel.locator('.prefix-staging').waitFor();
  assert.equal(await page.locator('vite-error-overlay').count(),0);await page.close();
 }
 assert.deepEqual((await req('/studio/state')).document.messages,baseline.document.messages);assert.deepEqual(errors,[]);assert.equal(expectedFailures,3);
 console.log(JSON.stringify({passed:true,staging:report.staging,cleanupResults,languages:['en','ja','zh'],widths:[1280,390,320],missingStore:true,trimPreservesStaging:true,unauthenticatedRefused:true,linkRefusedAndCleared:true,historyPreserved:true}));
}finally{
 try { await req('/studio/action',{action:'switchEngine',engine:baseline.settings.engine});await req('/studio/action',{action:'settings',settings:baseline.settings}); } finally { await browser.close();await rm(root,{recursive:true,force:true}); }
}
