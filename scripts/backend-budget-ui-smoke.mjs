// Browser skill unavailable: Playwright fixture, no engine launch or qualification claim.
import {createServer} from 'vite';
import {chromium,expect} from '@playwright/test';
import {mkdtemp} from 'node:fs/promises';import {tmpdir} from 'node:os';import {join} from 'node:path';
const evidence=await mkdtemp(join(tmpdir(),'soliton-backend-budget-'));
const html=`<!doctype html><html><head><title>Soliton runtime budget QA</title><link rel="icon" href="data:,"><meta name="viewport" content="width=device-width, initial-scale=1"></head><body><div id="root"></div><script type="module">
import React,{useState}from'react';import{createRoot}from'react-dom/client';import{ModelPlan}from'/src/ModelPlan.tsx';import'/src/style.css';
const base={backend:'vLLM-Apple',context:4096,weightsBytes:null,residentEstimateBytes:null,kvBytes:null,workspaceBytes:null,osReserveBytes:7000000000,memoryBudgetBytes:27359738368,diskFreeBytes:12000000000,warnings:[],assessment:'incomplete'};
function App(){const[lang,setLang]=useState('ja'),[mode,setMode]=useState('none');const reading={allocatorBytes:1073741824,allocatorPeakBytes:null,kvBytes:12582912,sampledAt:Date.now(),components:[{name:'weights',bytes:2147483648,basis:'artifact'},{name:'mtp',bytes:0,basis:'model_spec'},{name:'scratch',bytes:20971520,basis:'ledger'}]};
return React.createElement('main',{style:{maxWidth:720,margin:'16px auto',padding:'0 16px'}},React.createElement('nav',{},...['ja','en','zh'].map(l=>React.createElement('button',{key:l,onClick:()=>setLang(l)},l)),...['expire','stop','resume','wrapper'].map(m=>React.createElement('button',{key:m,onClick:()=>setMode(m==='stop'?'stopped':m==='wrapper'?'wrapper':'none')},m))),React.createElement(ModelPlan,{value:{...base,backendBudget:mode==='live'?reading:mode==='wrapper'?{...reading,allocatorBytes:null,allocatorPeakBytes:null,kvBytes:null,mlxAllocator:{activeBytes:1073741824,freeCacheBytes:134217728,peakActiveBytes:1610612736},retainedKv:{bytes:41943040,limitBytes:1073741824,entries:6}}:null},language:lang,onRefresh:async()=>setMode('live'),refreshDisabled:mode==='stopped'}));}
createRoot(document.getElementById('root')).render(React.createElement(App));</script></body></html>`;
const server=await createServer({plugins:[{name:'budget-fixture',configureServer(s){s.middlewares.use(async(req,res,next)=>{if(req.url!=='/backend-budget-qa.html')return next();try{res.setHeader('Content-Type','text/html');res.end(await s.transformIndexHtml('/backend-budget-qa.html',html));}catch(e){next(e);}});}}],server:{host:'127.0.0.1',port:5193,strictPort:true}});
let browser;await server.listen();
try{
 browser=await chromium.launch({headless:true,executablePath:process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});const page=await browser.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));page.on('console',m=>{if(m.type()==='error')errors.push(m.text());});
 await page.goto('http://127.0.0.1:5193/backend-budget-qa.html');await expect(page).toHaveTitle('Soliton runtime budget QA');
 const labels={ja:['実行中のメモリ診断を更新','報告されたアロケータ使用量','報告されたアロケータピーク','報告された論理KV'],en:['Refresh runtime memory','Reported allocator current','Reported allocator peak','Reported logical KV'],zh:['刷新运行内存诊断','报告的分配器当前量','报告的分配器峰值','报告的逻辑KV']};
 for(const width of [1440,390,320])for(const lang of ['en','ja','zh']){
  await page.setViewportSize({width,height:960});await page.getByRole('button',{name:lang,exact:true}).click();await page.getByRole('button',{name:'resume',exact:true}).click();
  const details=page.locator('details.model-plan');if(!await details.getAttribute('open').then(v=>v!==null))await details.locator('summary').click();
  const refresh=page.getByRole('button',{name:labels[lang][0],exact:true});await refresh.click();
  const cell=name=>details.locator('dl > div').filter({has:page.locator('dt').getByText(name,{exact:true})}).locator('dd');
  await expect(cell(labels[lang][1])).toHaveText('1.00 GiB');await expect(cell(labels[lang][2])).toHaveText('—');await expect(cell(labels[lang][3])).toHaveText('12.00 MiB');
  await expect(details).toContainText('MTP');await expect(details).toContainText('0.00 MiB');await expect(details).toContainText('20.00 MiB');
  if(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth+1))throw new Error('Horizontal overflow');await expect(page.locator('vite-error-overlay')).toHaveCount(0);
  await page.locator('main').screenshot({path:join(evidence,lang+'-'+width+'.png')});
  await page.getByRole('button',{name:'expire',exact:true}).click();await expect(details.getByText(labels[lang][1],{exact:true})).toHaveCount(0);await refresh.click();await expect(cell(labels[lang][1])).toHaveText('1.00 GiB');
  await page.getByRole('button',{name:'wrapper',exact:true}).click();
  const wrapperLabels={en:['MLX active allocations','MLX free allocator cache','MLX active allocation peak','Retained KV accounting'],ja:['MLX使用中の割当','MLXアロケータ空きキャッシュ','MLX使用中の割当ピーク','保持KVの論理量'],zh:['MLX活动分配','MLX分配器空闲缓存','MLX活动分配峰值','保留KV逻辑量']};
  await expect(cell(wrapperLabels[lang][0])).toHaveText('1.00 GiB');await expect(cell(wrapperLabels[lang][1])).toHaveText('128.00 MiB');await expect(cell(wrapperLabels[lang][2])).toHaveText('1.50 GiB');await expect(cell(wrapperLabels[lang][3])).toHaveText('40.00 MiB');
  await page.locator('main').screenshot({path:join(evidence,lang+'-wrapper-'+width+'.png')});
  await page.getByRole('button',{name:'stop',exact:true}).click();await expect(refresh).toBeDisabled();await expect(details.getByText(labels[lang][1],{exact:true})).toHaveCount(0);
 }
 if(errors.length)throw new Error(errors.join('\n'));console.log(JSON.stringify({passed:true,languages:3,widths:[1440,390,320],states:['refresh','unknown peak','zero MTP','expiry','refresh again','stop'],consoleErrors:errors,evidence}));
}finally{await browser?.close();await server.close();}
