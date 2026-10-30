import { createServer } from 'vite';
import { chromium, expect } from '@playwright/test';
import { writeFile, mkdir, rm } from 'node:fs/promises';
const fixture = '.soliton-runtime/repair-fixture.html';
await mkdir('.soliton-runtime', { recursive: true });
await writeFile(fixture, `<div id="root"></div><script type="module">
import React from 'react'; import {createRoot} from 'react-dom/client';
import {InstallationRepair} from '/src/InstallationRepair.tsx'; import '/src/style.css';
createRoot(document.getElementById('root')).render(React.createElement(InstallationRepair,{language:new URLSearchParams(location.search).get('lang')||'ja'}));
</script>`);
const server = await createServer({ server: { host:'127.0.0.1',port:5187,strictPort:true } });
await server.listen();
let browser;
try {
  browser = await chromium.launch({ executablePath:process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless:true });
  const page = await browser.newPage();
  const errors=[]; page.on('pageerror', e=>errors.push(e.message));
  for (const [code,lang,heading,button] of [['space','ja','設置用の空き容量が不足しています','再試行'],['permission','en','Cannot write to the installation folder','Retry'],['permission','zh','无法写入安装文件夹','重试']]) {
    await page.addInitScript(({code})=>{window.retries=0;window.__TAURI_INTERNALS__={invoke:async(cmd)=>{
      if(cmd==='installation_status')return {code,path:'/example/Soliton',detail:'Test installation failure',requiredBytes:2147483648};
      if(cmd==='retry_installation'){window.retries++;throw new Error('Test retry failure');}
    }};},{code});
    await page.goto('http://127.0.0.1:5187/'+fixture+'?lang='+lang);
    await expect(page.getByText(heading,{exact:true})).toBeVisible();
    await page.getByRole('button',{name:button,exact:true}).click();
    await expect.poll(()=>page.evaluate(()=>window.retries)).toBe(1);
    await expect(page.getByRole('button',{name:button,exact:true})).toBeEnabled();
    await expect(page.getByText('/example/Soliton',{exact:true})).toBeVisible();
    await page.screenshot({path:'.soliton-runtime/repair-'+code+'-'+lang+'.png'});
  }
  if(errors.length)throw new Error(errors.join('\n'));
  console.log('Space/permission repair UI, retry and three-language checks passed');
} finally { await browser?.close();await server.close();await rm(fixture,{force:true}); }
