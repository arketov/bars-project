(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const call = (method, params = {}) => invoke('bars_call', { method, params });
  const out = {success: false, checks: [], model: 'FakeProvider', native: navigator.userAgent};
  const assert = (ok, text) => { if (!ok) throw Error(text); out.checks.push(text); };
  const wait = async (read, text) => { const until = performance.now() + 25000; while (performance.now() < until) { const value = await read(); if(value) return value; await new Promise(r=>setTimeout(r,75)); } throw Error(text); };
  try {
    const {projects} = await call('projects');
    const project = projects[0].path, path = 'docs/Тема.md', target = {project, cwd:project};
    await wait(async()=> {const a=await call('activity'); return !a.running && !a.queue.length;}, 'Начальная индексация');
    const before = await invoke('document_read', {project,path});
    const header = '| Номер | Описание |\n|---|---|';
    const body = header + '\n' + Array.from({length:25},(_,i)=>`| ISS-${i} | ${'описание '.repeat(19)} |`).join('\n');
    const text = '# Реестр\n' + body;
    const saved = await invoke('document_write',{project,path,text,baseRevision:before.revision,eol:'lf',bom:false});
    assert(saved.status === 'saved','Реестр записан через продуктовую команду редактора');
    await call('reindex',{...target,full:true});
    const result = await wait(async()=> {
      const r=await call('search',{...target,query:'ISS-24',limit:20,min_score:0,diagnostic:true});
      return r.results.length>=3 && r.results.every(h=>h.text.startsWith(header)) ? r : false;
    },'Новая нарезка не появилась в настоящем воркере');
    assert(result.results.every(h=>[...h.text.replace(/ \.\.\.\(truncated\)$/,'')].length<=1500),'Все части реестра укладываются в окно вместе с шапкой');
    assert(result.results.some(h=>h.text.includes('| ISS-24 |')),'Хвост старого длинного чанка доступен в поиске');
    assert(result.results.every(h=>h.first_line===1 || text.split('\n')[h.first_line-1].startsWith('| ISS-')),'Строки продолжений указывают на собственные данные');
    const button=[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='Поиск'); if(button) button.click();
    const query=await wait(()=>document.querySelector('textarea[aria-label="Поисковый запрос"]'),'Поле поиска');
    query.value='ISS-24';query.dispatchEvent(new Event('input',{bubbles:true}));
    query.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));
    await wait(()=>document.querySelectorAll('.card[data-document-path]').length>=3,'Карточки новых чанков не показаны');
    assert(!!document.querySelector('.card .text table'),'Шапка и строки чанка отрисованы таблицей в собранном WebView2');
    out.parts=result.results.map(h=>({first_line:h.first_line,last_line:h.last_line,chars:[...h.text].length}));
    out.success=true;
  } catch(e) {out.error=String(e.stack||e);}
  await invoke('workspace_probe_finish',{result:out});
})();
