// Программная проверка собранного окна Tauri/WebView2, изолированные проект и профиль.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const checks = [];
  const assert = (value, label) => { if (!value) throw Error(label); checks.push(label); };
  const wait = async (read, label) => {
    const end = performance.now() + 45000;
    while (performance.now() < end) { const value = await read(); if (value) return value; await new Promise(resolve => setTimeout(resolve, 100)); }
    throw Error(label);
  };
  const button = text => [...document.querySelectorAll('button')].find(b => b.textContent.trim() === text);
  try {
    const projects = await invoke('bars_call', { method:'projects', params:{} });
    const project = projects.projects[0].path;
    const call = (method, params={}) => invoke('bars_call', {method, params:{cwd:project,project,...params}});
    await wait(async () => {const s=await call('graph_status');return s.snapshot && !s.active_job && s.state==='ready';},'Начальный граф');
    const search = await call('search',{query:'индексация',min_score:0,limit:10});
    assert(search.results.length > 0,'Настоящий поиск воркера вернул результаты');
    const r={id:'native',text:'индексация',scope:{modules:[],files:[]},score_kind:search.applied.score_kind,
      hits:search.results.map(h=>({path:h.path,title:h.title,heading:h.heading_chain,first:h.first_line,last:h.last_line,score:h.score})),
      depth:1,fanout:4,min_strength:0.5,max_nodes:200,max_edges:800,max_bytes:262144,snapshot:null};
    const start=performance.now(); const slice=await call('graph_query_slice',{request:r}); const elapsed=performance.now()-start;
    assert(slice.physical.method==='query-v1','Tauri передаёт QueryGraphSlice');
    assert(slice.results.every(r=>r.node!==null),'Результаты настоящего поиска найдены в снимке');
    assert(slice.used_bytes<=r.max_bytes,'Весь ответ укладывается в байтовый предел');
    const d0=await call('graph_query_slice',{request:{...r,depth:0,snapshot:slice.physical.snapshot}});
    assert(d0.physical.nodes.length===d0.results.length,'Глубина ноль не добавляет окружение');
    try {await call('graph_query_slice',{request:{...r,snapshot:'expired'}});throw Error('Снимок принят');}
    catch(e){assert(e.data?.kind==='snapshot_expired','Старый снимок отклонён');}
    await wait(()=>document.querySelector('textarea[aria-label="Поисковый запрос"]'),'Поле поиска');
    await wait(()=>document.querySelector('.pg.mini')?.dataset.loaded==='true','Начальная загрузка мини-графа');
    const input=document.querySelector('textarea[aria-label="Поисковый запрос"]');
    input.value='индексация';input.dispatchEvent(new Event('input',{bubbles:true}));
    await wait(()=>button('Найти') && !button('Найти').disabled,'Поле запроса применено');
    button('Найти').click();
    await wait(()=>document.querySelector('.pg.mini .qplate .qt')?.textContent.includes('индексация'),'Мини-граф запроса');
    assert(true,'Завершённый поиск включает мини-граф запроса');
    button('Граф').click();
    const full=await wait(()=>document.querySelector('.pg:not(.mini) .qbar'),'Полный граф запроса');
    assert(full.textContent.includes('индексация'),'Полный граф получает срез запроса');
    const scope=full.closest('.pg');
    scope.querySelector('[role="radio"]').click();
    await wait(()=>Number(scope.dataset.nodes)===slice.results.length+1,'Глубина ноль в окне');
    assert(true,'Переключение глубины получает Rust-срез');
    [...scope.querySelectorAll('button')].find(b=>b.textContent.trim()==='Общий вид').click();
    await wait(()=>!scope.querySelector('.qbar'),'Возврат в общий вид');
    assert(!!document.querySelector('.pg.mini .qplate'),'Возврат одной панели сохраняет другую');
    await invoke('graph_lifecycle_probe_finish',{result:{success:true,checks,elapsed_ms:elapsed,native:navigator.userAgent,slice}});
  } catch(error) {
    await invoke('graph_lifecycle_probe_finish',{result:{success:false,checks,error:error.message??String(error),stack:error.stack,body:document.body.innerText.slice(0,9000),panels:[...document.querySelectorAll('.pg')].map(p=>({data:{...p.dataset},text:p.innerText.slice(0,2000)}))}});
  }
})();
