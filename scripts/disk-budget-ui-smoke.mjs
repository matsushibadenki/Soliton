// Browser skill not available: Playwright checks the real preparation dialog with synthetic capacity samples.
import { createServer } from 'vite';
import { chromium, expect } from '@playwright/test';
import { mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const evidence=await mkdtemp(join(tmpdir(),'soliton-disk-budget-'));
const html=`<!doctype html><html><head><title>Soliton disk budget QA</title><link rel="icon" href="data:,"><meta name="viewport" content="width=device-width, initial-scale=1"></head><body><div id="root"></div><script type="module">
import React,{useState}from'react';import{createRoot}from'react-dom/client';import{SetupDialog}from'/src/SetupDialog.tsx';import'/src/style.css';
const lang=new URLSearchParams(location.search).get('lang')||'ja';
function App(){const[mode,setMode]=useState('short'),[runs,setRuns]=useState(0);const free=mode==='short'?111e9:mode==='unknown'?null:112e9;
const plan={target:'/Volumes/DownloadSSD/Strata',requiredBytes:112e9,freeBytes:free,shortfallBytes:free===null?null:Math.max(0,112e9-free),status:free===null?'unknown':free<112e9?'insufficient':'sufficient'};
const inspection={rootExists:true,setupAvailable:true,supportedHost:true,python:{supported:true,dependencies:true,version:'3.12'},configs:[],selected:null,models:[{id:'Q2_0',downloadGb:66.4,workingGb:112}],freeBytes:999e9,recommended:'Q2_0',diskPlans:[{id:'Q2_0',download:plan,local:{...plan,target:'/Volumes/RuntimeSSD/Strata-data',requiredBytes:45.6e9}}]};
return React.createElement(SetupDialog,{language:lang,inspection,job:{status:'idle',model:'',error:null},engineOffline:true,config:'',onInspect:async()=>setMode(mode==='short'?'ready':'unknown'),onModels:()=>{},onConfig:async()=>{},onPrepare:async()=>{setRuns(runs+1);document.title='Prepared '+(runs+1);},onCancel:async()=>{},onClose:()=>{}});}
createRoot(document.getElementById('root')).render(React.createElement(App));</script></body></html>`;
const server=await createServer({plugins:[{name:'disk-qa',configureServer(s){s.middlewares.use(async(req,res,next)=>{if(req.url?.split('?')[0]!=='/disk-qa.html')return next();try{res.setHeader('Content-Type','text/html');res.end(await s.transformIndexHtml('/disk-qa.html',html));}catch(e){next(e);}});}}],server:{host:'127.0.0.1',port:5193,strictPort:true}});
let browser;await server.listen();
try{
 browser=await chromium.launch({headless:true,executablePath:process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
 const page=await browser.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));page.on('console',m=>{if(m.type()==='error')errors.push(m.text());});
 for(const width of [1440,390,320])for(const lang of ['en','ja','zh']){
  await page.setViewportSize({width,height:960});await page.goto('http://127.0.0.1:5193/disk-qa.html?lang='+lang);await expect(page).toHaveTitle('Soliton disk budget QA');
  const dialog=page.getByRole('dialog');await expect(dialog).toBeVisible();await expect(dialog.locator('.disk-budget')).toContainText('111.0 GB');await expect(dialog.locator('.disk-budget')).not.toContainText('999.0 GB');await expect(dialog.locator('.disk-budget')).toContainText('1.0 GB');
  await dialog.getByRole('checkbox').check();const prepare=dialog.locator('button.primary');await expect(prepare).toBeDisabled();
  await dialog.locator('button').filter({has:page.locator('svg.lucide-refresh-cw')}).click();await expect(dialog.locator('.disk-budget')).toContainText('112.0 GB');await expect(prepare).toBeEnabled();await prepare.click();await expect(page).toHaveTitle('Prepared 1');
  await dialog.locator('button').filter({has:page.locator('svg.lucide-refresh-cw')}).click();await expect(dialog.locator('.disk-budget')).toContainText('—');
  await expect(page.locator('vite-error-overlay')).toHaveCount(0);
  if(await dialog.evaluate(el=>el.scrollWidth>el.clientWidth+1))throw new Error('Dialog overflow');
  await dialog.screenshot({path:join(evidence,lang+'-'+width+'.png')});
 }
 if(errors.length)throw new Error(errors.join('\n'));console.log(JSON.stringify({passed:true,languages:3,widths:[1440,390,320],flow:'insufficient -> refresh -> exact capacity -> prepare -> unknown',consoleErrors:errors,evidence}));
}finally{await browser?.close();await server.close();}
