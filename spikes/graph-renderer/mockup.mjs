import {chromium} from 'playwright';
import {createServer} from 'node:http';
import {readFile,mkdir,writeFile} from 'node:fs/promises';
import {resolve,extname,sep} from 'node:path';
const root=resolve('../..'),dir=resolve(root,'target/stage33-probe/mockup');await mkdir(dir,{recursive:true});
const server=createServer(async(req,res)=>{try{const path=resolve(root,'.'+decodeURIComponent(new URL(req.url,'http://localhost').pathname));if(!path.startsWith(root+sep))throw Error();const data=await readFile(path);res.setHeader('Content-Type',({'.html':'text/html','.js':'text/javascript','.css':'text/css'}[extname(path)])??'application/octet-stream');res.end(data);}catch{res.writeHead(404).end();}});
await new Promise(r=>server.listen(1434,'127.0.0.1',r));let browser;
const results=[];
try{
 browser=await chromium.launch({executablePath:'C:/Program Files/Google/Chrome/Application/chrome.exe',headless:true});const page=await browser.newPage({viewport:{width:1440,height:1000}});
 const errors=[];page.on('pageerror',error=>errors.push(error.message));
 await page.goto('http://127.0.0.1:1434/docs/features/window/mockups/graph.html');
 await page.locator('[data-node]').first().waitFor();
 const check=(name,ok)=>{if(!ok)throw Error(name);results.push({name,passed:true});};
 check('документы',await page.locator('#scene .node').count()===8);
 await page.uncheck('.scope input');check('пустой scope',await page.locator('#empty').isVisible()&&!(await page.locator('#canvas').isVisible()));await page.check('.scope input');
 for(const [view,count] of [['sections',16],['chunks',16],['entities',8],['mixed',32]]){await page.selectOption('#view',view);check('вид '+view,await page.locator('#scene .node').count()===count);}
 await page.selectOption('#view','documents');
 for(const op of ['relations','paths','impact','context','explore','audit']){await page.click(`[data-op="${op}"]`);check('операция '+op,(await page.locator('#detail').innerText()).length>40);}
 await page.click('[data-op="paths"]');await page.selectOption('#target','worker');check('путь от выбранного объекта', (await page.locator('#pathDetail').innerText()).includes('Согласие на переиндексацию → Воркер и индекс'));
 await page.locator('#detail [data-source]').first().click();check('диалог исходника',await page.locator('#source').evaluate(e=>e.open));await page.keyboard.press('Escape');check('Escape закрывает исходник',!(await page.locator('#source').evaluate(e=>e.open)));
 await page.click('#rebuild');check('задание принято',(await page.inputValue('#scenario'))==='building');await page.click('#rebuild');check('задание отменено',(await page.inputValue('#scenario'))==='cancelled');await page.click('#rebuild');check('повторный запуск',(await page.inputValue('#scenario'))==='building');
 for(const state of ['partial','not_built','cancelled','stale','error','empty','ambiguous','conflict','hub','long','renderer']){await page.selectOption('#scenario',state);check('состояние '+state,(await page.locator('#notice').innerText()).length>25);if(state==='hub')check('250 настоящих узлов фикстуры',await page.locator('#scene .node').count()===250);if(state==='renderer')check('список при отказе renderer',await page.locator('#nodeList').isVisible());}
 await page.selectOption('#scenario','partial');await page.click('#list');const node=page.locator('#nodeList button').first();await node.focus();await page.keyboard.press('Enter');check('клавиатурный выбор',(await page.locator('#title').innerText())==='Согласие на переиндексацию');
 check('SVG скрыт в режиме списка',!(await page.locator('#canvas').isVisible()));await page.click('#list');check('карта видна без списка',await page.locator('#canvas').isVisible()&&!(await page.locator('#nodeList').isVisible()));
 for(const theme of ['light','dark']){if(theme==='dark')await page.click('#theme');for(const width of [1440,760]){await page.setViewportSize({width,height:1000});await page.screenshot({path:resolve(dir,`${theme}-${width}.png`),fullPage:true});check(`${theme} ${width}: без горизонтального переполнения`,await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));}}
 check('без ошибок JavaScript',errors.length===0);await writeFile(resolve(dir,'checks.json'),JSON.stringify({environment:await page.evaluate(()=>navigator.userAgent),headless:true,fixture:true,results,errors},null,2));console.log(`${results.length} checks passed`);
}finally{await browser?.close();await new Promise(r=>server.close(r));}
