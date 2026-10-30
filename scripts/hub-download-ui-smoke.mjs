import { createServer } from 'vite';
import { chromium, expect } from '@playwright/test';
import { mkdir, writeFile, rm } from 'node:fs/promises';
const fixture='.soliton-runtime/hub-ui-fixture.html';
await mkdir('.soliton-runtime',{recursive:true});
await writeFile(fixture,`<div id="root"></div><script type="module">
import React from 'react';import{createRoot}from'react-dom/client';import{connect}from'/src/client.ts';import{ModelLibraryDialog}from'/src/ModelLibraryDialog.tsx';import'/src/style.css';
await connect();createRoot(document.getElementById('root')).render(React.createElement(ModelLibraryDialog,{language:'ja',directories:[],locked:false,engineOffline:true,onDirectories:async()=>{},onUse:async()=>{},onClose:()=>{}}));
</script>`);
const server=await createServer({server:{host:'127.0.0.1',port:5188,strictPort:true}});await server.listen();let browser;
try{
 browser=await chromium.launch({executablePath:process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
 const page=await browser.newPage({viewport:{width:720,height:900}});const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.addInitScript(()=>{window.__TAURI_INTERNALS__={transformCallback:cb=>{window.progressCallback=cb;return 1;},invoke:async(cmd)=>{
 if(cmd==='connection')return{url:'http://127.0.0.1:5188',token:'fixture'};
 if(cmd==='download_huggingface_file'){window.progressCallback({payload:{received:50,total:100}});return new Promise(()=>{});}return 1;
 }};});
 await page.route('**/studio/models**',r=>r.fulfill({json:{roots:[],files:[],limited:false}}));
 await page.goto('http://127.0.0.1:5188/'+fixture);
 await page.getByLabel('Hugging Face URL',{exact:false}).first().fill('https://huggingface.co/example/repo');
 await page.getByRole('button',{name:'ダウンロード',exact:true}).click();
 await expect(page.getByText('50%',{exact:true})).toBeVisible();
 await expect(page.getByRole('button',{name:'停止',exact:true})).toBeVisible();
 await expect(page.getByRole('progressbar')).toHaveAttribute('value','50');
 await page.screenshot({path:'.soliton-runtime/hub-download-ui.png'});
 if(errors.length)throw new Error(errors.join('\n'));
 console.log('Download progress row, percentage, stop control and narrow layout rendered');
}finally{await browser?.close();await server.close();await rm(fixture,{force:true});}
