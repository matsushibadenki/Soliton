// Browser plugin/skill not available: validate the rendered component with Playwright.
// Synthetic response data only; this does not run Kapitsa or measure GPU performance.
import { createServer } from 'vite';
import { chromium, expect } from '@playwright/test';
import { mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const evidence = await mkdtemp(join(tmpdir(), 'soliton-prompt-metrics-'));
const html = `<!doctype html><html><head><link rel="icon" href="data:,"><title>Soliton prompt metrics QA</title><meta name="viewport" content="width=device-width, initial-scale=1"></head><body><div id="root"></div><script type="module">
import React,{useState}from'react';import{createRoot}from'react-dom/client';import{RequestMetrics}from'/src/InferenceView.tsx';import'/src/style.css';
const base={id:'fixture',source:'chat',model:'synthetic-response',engine:'fixture',startedAt:0,status:'completed',elapsedMs:900,firstOutputMs:150,firstAnswerMs:200,finishReason:'stop',usage:{prompt:1000,completion:5,total:1005,cached:800,cacheCreation:0,reasoning:0},timings:{promptMs:100,decodeMs:100,tokensPerSecond:50},promptProcessing:{uncachedTokens:200,cacheHitPercent:80,tokensPerSecond:2000}};
function App(){const[lang,setLang]=useState('ja'),[mode,setMode]=useState('valid');const value=mode==='valid'?base:mode==='missing'?{...base,usage:null,timings:null,promptProcessing:null}:{...base,status:'cancelled',promptProcessing:{...base.promptProcessing,tokensPerSecond:null}};
return React.createElement('main',{style:{maxWidth:720,margin:'16px auto',padding:'0 16px',boxSizing:'border-box'}},React.createElement('nav',{},...['ja','en','zh'].map(l=>React.createElement('button',{key:l,onClick:()=>setLang(l)},l)),...['valid','missing','cancelled'].map(m=>React.createElement('button',{key:m,onClick:()=>setMode(m)},m))),React.createElement(RequestMetrics,{value,language:lang}));}
createRoot(document.getElementById('root')).render(React.createElement(App));</script></body></html>`;
const server = await createServer({plugins:[{name:'prompt-metrics-fixture',configureServer(s){s.middlewares.use(async(req,res,next)=>{if(req.url?.split('?')[0]!=='/prompt-metrics-qa.html')return next();try{res.setHeader('Content-Type','text/html');res.end(await s.transformIndexHtml('/prompt-metrics-qa.html',html));}catch(e){next(e);}});}}],server:{host:'127.0.0.1',port:5193,strictPort:true}});
let browser;
await server.listen();
try {
 browser=await chromium.launch({headless:true,executablePath:process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
 const page=await browser.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));page.on('console',m=>{if(m.type()==='error')errors.push(m.text()+' '+m.location().url);});
 await page.goto('http://127.0.0.1:5193/prompt-metrics-qa.html');await expect(page).toHaveTitle('Soliton prompt metrics QA');
 const labels={ja:['入力キャッシュ再利用率','入力処理速度','未キャッシュ入力トークン'],en:['Prompt cache reuse','Input processing speed','Uncached input tokens'],zh:['输入缓存复用率','输入处理速度','未缓存输入token']};
 for(const width of [1440,390]){
  await page.setViewportSize({width,height:960});
  for(const lang of ['ja','en','zh']){
   await page.getByRole('button',{name:lang,exact:true}).click();await page.getByRole('button',{name:'valid',exact:true}).click();
   const details=page.locator('details.request-metrics');if(!await details.getAttribute('open').then(v=>v!==null))await details.locator('summary').click();
   const cell=name=>page.locator('dl > div').filter({has:page.locator('dt').getByText(name,{exact:true})}).locator('dd');
   await expect(cell(labels[lang][0])).toHaveText('80.0%');await expect(cell(labels[lang][1])).toHaveText('2000.0 tok/s');await expect(cell(labels[lang][2])).toHaveText('200');
   await expect(page.locator('vite-error-overlay')).toHaveCount(0);
   if(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth+1))throw new Error('Horizontal overflow');
   await page.locator('main').screenshot({path:join(evidence,lang+'-'+width+'.png')});
   await page.getByRole('button',{name:'missing',exact:true}).click();await expect(cell(labels[lang][0])).toHaveText('—');await expect(cell(labels[lang][1])).toHaveText('—');
   await page.getByRole('button',{name:'cancelled',exact:true}).click();await expect(cell(labels[lang][1])).toHaveText('—');
  }
 }
 if(errors.length)throw new Error(errors.join('\n'));console.log(JSON.stringify({passed:true,languages:3,widths:[1440,390],states:['valid','missing','cancelled'],consoleErrors:errors,evidence}));
}finally{await browser?.close();await server.close();}
