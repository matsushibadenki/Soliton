// Browser plugin/skill not available: validate the rendered component with Playwright.
// Synthetic response data only; this does not run Kapitsa or measure GPU performance.
import { createServer } from 'vite';
import { chromium, expect } from '@playwright/test';
import { mkdtemp, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const evidence = await mkdtemp(join(tmpdir(), 'soliton-model-plan-'));
const realPlan = process.env.SOLITON_MODEL_PLAN_FIXTURE ? JSON.parse(await readFile(process.env.SOLITON_MODEL_PLAN_FIXTURE,"utf8")) : null;
const html = `<!doctype html><html><head><link rel="icon" href="data:,"><title>Soliton model plan QA</title><meta name="viewport" content="width=device-width, initial-scale=1"></head><body><div id="root"></div><script type="module">
import React,{useState}from'react';import{createRoot}from'react-dom/client';import{ModelPlan}from'/src/ModelPlan.tsx';import'/src/style.css';
const base=${realPlan ? JSON.stringify(realPlan).replace(/</g,'\\u003c') : 'null'} ?? {backend:'strata-gemma3-metal',context:2048,weightsBytes:2006574112,residentEstimateBytes:null,kvBytes:54525952,workspaceBytes:1048576,kvBasis:"dense_payload_envelope",workspaceBasis:"output_vector_floor",retainedKvLimitBytes:null,allocatorCacheLimitBytes:null,osReserveBytes:7000000000,memoryBudgetBytes:27359738368,diskFreeBytes:12000000000,warnings:[],assessment:'incomplete'};
function App(){const[lang,setLang]=useState('ja'),[mode,setMode]=useState('valid');const value=mode==='valid'?base:mode==='stopped'?{...base,runtimeBuffers:null}:mode==='limits'?{...base,retainedKvLimitBytes:1073741824,allocatorCacheLimitBytes:536870912}:mode==='missing'?{...base,context:null,weightsBytes:null,memoryBudgetBytes:null,diskFreeBytes:null,kvBytes:null,workspaceBytes:null}:{...base,assessment:'attention',warnings:['reserve_exhausted','output_reservation','disk_full','state_workspace_budget']};
return React.createElement('main',{style:{maxWidth:720,margin:'16px auto',padding:'0 16px',boxSizing:'border-box'}},React.createElement('nav',{},...['ja','en','zh'].map(l=>React.createElement('button',{key:l,onClick:()=>setLang(l)},l)),...['valid','missing','warning','limits','stopped'].map(m=>React.createElement('button',{key:m,onClick:()=>setMode(m)},m))),React.createElement(ModelPlan,{value,language:lang}));}
createRoot(document.getElementById('root')).render(React.createElement(App));</script></body></html>`;
const server = await createServer({plugins:[{name:'model-plan-fixture',configureServer(s){s.middlewares.use(async(req,res,next)=>{if(req.url?.split('?')[0]!=='/model-plan-qa.html')return next();try{res.setHeader('Content-Type','text/html');res.end(await s.transformIndexHtml('/model-plan-qa.html',html));}catch(e){next(e);}});}}],server:{host:'127.0.0.1',port:5193,strictPort:true}});
let browser;
await server.listen();
try {
 browser=await chromium.launch({headless:true,executablePath:process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
 const page=await browser.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));page.on('console',m=>{if(m.type()==='error')errors.push(m.text()+' '+m.location().url);});
 await page.goto('http://127.0.0.1:5193/model-plan-qa.html');await expect(page).toHaveTitle('Soliton model plan QA');
 for(const width of [1440,390,320]){
  await page.setViewportSize({width,height:960});
  for(const lang of ['ja','en','zh']){
   await page.getByRole('button',{name:lang,exact:true}).click();await page.getByRole('button',{name:'valid',exact:true}).click();
   const details=page.locator('details.model-plan');if(!await details.getAttribute('open').then(v=>v!==null))await details.locator('summary').click();
   await expect(details).toContainText('1.87 GiB');await expect(details).toContainText('≈ 52.00 MiB');await expect(details).toContainText('≥ 1.00 MiB');await expect(details).toContainText('strata-gemma3-metal');await expect(details.locator('dd').filter({hasText:'—'})).toHaveCount(3);
   await expect(page.locator('vite-error-overlay')).toHaveCount(0);
   if(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth+1))throw new Error('Horizontal overflow');
   if(realPlan?.runtimeBuffers){
    const labels={en:'Reported compute graph',ja:'報告された計算グラフ',zh:'报告的计算图'};
    await expect(details.getByText(labels[lang],{exact:true})).toBeVisible();
    const n=realPlan.runtimeBuffers.computeBytes;const formatted=n>=2**30?(n/2**30).toFixed(2)+' GiB':(n/2**20).toFixed(2)+' MiB';await expect(details).toContainText('≈ '+formatted);
    await page.getByRole('button',{name:'stopped',exact:true}).click();await expect(details.getByText(labels[lang],{exact:true})).toHaveCount(0);
   }
   await page.getByRole('button',{name:'limits',exact:true}).click();await expect(details).toContainText('1.00 GiB');await expect(details).toContainText('512.00 MiB');
   await page.getByRole('button',{name:'missing',exact:true}).click();await expect(details.locator('dd').filter({hasText:'—'})).toHaveCount(9);
   await page.getByRole('button',{name:'warning',exact:true}).click();await expect(details.locator('li')).toHaveCount(4);
   await page.locator('main').screenshot({path:join(evidence,lang+'-'+width+'.png')});
   if(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth+1))throw new Error('Warning horizontal overflow');
  }
 }
 if(errors.length)throw new Error(errors.join('\n'));console.log(JSON.stringify({passed:true,languages:3,widths:[1440,390,320],states:['valid','missing','warning','limits','stopped'],rustMetadataFixture:!!realPlan,consoleErrors:errors,evidence}));
}finally{await browser?.close();await server.close();}
