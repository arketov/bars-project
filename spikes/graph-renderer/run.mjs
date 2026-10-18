import {chromium} from 'playwright';
import {writeFile} from 'node:fs/promises';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
const server=spawn(process.execPath,['../../ui/node_modules/vite/bin/vite.js','--host','127.0.0.1','--port','1433','--strictPort'],{stdio:['ignore','pipe','pipe'],windowsHide:true});
let browser;
try{
 for(let i=0;i<100;i++){try{if((await fetch('http://127.0.0.1:1433')).ok)break;}catch{}await new Promise(r=>setTimeout(r,100));if(i===99)throw Error('Vite недоступен');}
 browser=await chromium.launch({executablePath:'C:/Program Files/Google/Chrome/Application/chrome.exe',headless:true});
 const page=await browser.newPage({viewport:{width:1440,height:900},deviceScaleFactor:1});
 await page.goto('http://127.0.0.1:1433');await page.waitForFunction(()=>typeof window.probe==='function');
 const environment=await page.evaluate(()=>{const canvas=document.createElement('canvas'),gl=canvas.getContext('webgl2'),info=gl?.getExtension('WEBGL_debug_renderer_info');return{userAgent:navigator.userAgent,webgl:!!gl,renderer:info?gl.getParameter(info.UNMASKED_RENDERER_WEBGL):null};});
 const rows=[];for(const n of [250,1000,2000])for(const renderer of ['cytoscape','sigma'])for(let run=0;run<3;run++){
  const result=await page.evaluate(({renderer,n})=>window.probe(renderer,n),{renderer,n});rows.push({...result,run});console.log(JSON.stringify(rows.at(-1)));
 }
 await writeFile('../../target/stage33-probe/renderer-results.json',JSON.stringify({environment,fixture:'spiral-binary-tree-plus-neighbor-v1',headless:true,rows},null,2));
}finally{await browser?.close();server.kill();await once(server,'exit').catch(()=>{});}
