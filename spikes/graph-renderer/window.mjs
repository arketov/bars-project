import {chromium} from 'playwright';
import {mkdir,writeFile,readFile} from 'node:fs/promises';
const browser=await chromium.launch({executablePath:'C:/Program Files/Google/Chrome/Application/chrome.exe',headless:true});
const page=await browser.newPage({viewport:{width:1440,height:1000}}),errors=[],checks=[];
page.on('pageerror',e=>errors.push(e.message));
const check=(name,ok)=>{if(!ok)throw Error(name);checks.push({name,passed:true});};
const dir='../../target/stage33-probe/window';await mkdir(dir,{recursive:true});
try{
 await page.goto('http://127.0.0.1:1420/');await page.getByRole('button',{name:'◇ Граф',exact:true}).click();
 await page.locator('.graph-page').waitFor();await page.locator('.graph-canvas canvas').first().waitFor();
 await page.waitForFunction(()=>document.querySelector('.graph-toolbar>span')?.textContent.includes('6 объектов'));
 check('настоящий компонент строит карту',await page.locator('.graph-canvas canvas').count()>0);
 const fixtures=JSON.parse(await readFile('../../ui/src/lib/fixtures/graph.json','utf8'));
 for(const view of ['sections','chunks','entities','mixed','documents']){
  await page.getByLabel('Вид',{exact:true}).selectOption(view);
  await page.waitForFunction(n=>document.querySelector('.graph-toolbar>span')?.textContent.startsWith(n+' объектов'),fixtures['graph:'+view].nodes.length);
  check('вид '+view,true);
 }
 await page.getByRole('button',{name:'Списком',exact:true}).click();await page.locator('.graph-list button').first().click();
 check('выбор объекта открывает инспектор',(await page.locator('.graph-inspector').innerText()).includes('revision:'));
 await page.getByRole('button',{name:'Закрепить позицию',exact:true}).click();check('pin',await page.getByRole('button',{name:'Освободить позицию',exact:true}).isVisible());
 await page.getByRole('button',{name:'Освободить позицию',exact:true}).click();check('unpin',await page.getByRole('button',{name:'Закрепить позицию',exact:true}).isVisible());
 for(const [op,label] of [['relations','Связи'],['paths','Путь'],['impact','Влияние'],['context','Контекст'],['explore','Обзор']]){
  await page.locator('.graph-controls').first().getByRole('combobox').selectOption(op);
  if(op==='context')await page.locator('.graph-controls input').first().fill('Т-25 требует Т-26');
  await page.getByRole('button',{name:'Показать',exact:true}).click();await page.getByRole('button',{name:'Показать',exact:true}).waitFor();
  check('операция '+label,!(await page.locator('.graph-error').count()));
 }
 check('тематические группы',await page.getByRole('button',{name:'Раскрыть группу',exact:true}).count()>0);
 await page.getByRole('button',{name:'Раскрыть группу',exact:true}).first().click();
 await page.locator('.graph-list button').first().click();
 await page.getByRole('button',{name:'Открыть основание ↗',exact:true}).first().click();
 check('quote снимка',await page.locator('.graph-source pre').innerText()!=='');await page.keyboard.press('Escape');
 check('Escape закрывает источник',!(await page.locator('.graph-source').evaluate(d=>d.open)));
 await page.getByRole('button',{name:'Списком',exact:true}).click();
 for(const theme of ['light','dark']){
  await page.evaluate(mode=>{document.documentElement.dataset.mode=mode;},theme);
  for(const width of [1440,760]){await page.setViewportSize({width,height:1000});await page.screenshot({path:`${dir}/${theme}-${width}.png`,fullPage:true});check(`геометрия ${theme} ${width}`,await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));}
 }
 await page.getByRole('button',{name:'⌕ Поиск',exact:true}).click();check('renderer уничтожен',await page.locator('.graph-canvas canvas').count()===0);
 for(const mode of ['mix','graph']){await page.locator('.retrieval-mode select').selectOption(mode);await page.locator('textarea').fill('Т-25 требует Т-26');await page.getByRole('button',{name:/Найти/}).click();await page.locator('.graph-search-bundle').first().waitFor();check('поиск '+mode,await page.locator('.graph-search-bundle').count()>0);}
 check('graph не объясняет отсутствие scored chunks порогом',!(await page.locator('.empty.none').count()));
 await page.locator('.graph-search-bundle details').first().click();
 check('основание отношения доступно прямо в поиске',(await page.locator('.graph-search-bundle pre').first().innerText()).length>0);
 await page.locator('.retrieval-mode select').selectOption('mix');await page.getByLabel('Добавлять связанные источники',{exact:true}).uncheck();await page.getByRole('button',{name:/Найти/}).click();
 await page.waitForFunction(()=>document.querySelector('.graph-search-info')?.textContent.includes('include_related=false'));
 check('выключение связей убирает пакеты',await page.locator('.graph-search-bundle').count()===0);
 check('без runtime ошибок',errors.length===0);console.log(JSON.stringify({checks,errors},null,2));await writeFile(`${dir}/checks.json`,JSON.stringify({fixture:true,headless:true,checks,errors},null,2));
}finally{if(errors.length)console.error(errors);await browser.close();}
