// Программная проверка настоящего WebView2; не физическая приёмка установленного приложения.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const report = { success: false, probeRevision: 'audits-3.8.3-real-v3', native: navigator.userAgent, checks: [] };
  let deadlineMs=15000;
  const assert = (v,m) => { if (!v) throw Error(m); };
  const frame = () => new Promise(resolve => requestAnimationFrame(resolve));
  const wait = async (test,label) => { const end=performance.now()+deadlineMs; do { const value=await test(); if(value)return value; await frame(); } while(performance.now()<end); throw Error(label); };
  const call = (method,params={}) => invoke('bars_call',{method,params});
  const btn = (root,text) => [...root.querySelectorAll('button')].find(b=>b.textContent.trim()===text);
  let project;
  try {
    const real=await invoke('audits_probe_real');
    if(real) deadlineMs=60000;
    report.realModels=real;
    project=await invoke('audits_probe_project');
    const indexStarted=performance.now();
    await wait(async()=>{const s=await call('status',{cwd:project}); return s.state==='ready' && s.pending===0;},'индекс fixture не готов');
    report.initialReadyMs=performance.now()-indexStarted;
    await wait(()=>document.querySelector('.topics-row[title="Битые ссылки и повторы в документации"]'),'нет строки аудитов');
    document.querySelector('.topics-row[title="Битые ссылки и повторы в документации"]').click();
    const page=await wait(()=>document.querySelector('section[aria-label="Аудиты"]'),'экран аудитов не открыт');
    const kind=name=>document.querySelector(`section[aria-label="Аудиты"] article[aria-label="${name}"]`);
    await wait(()=>kind('Противоречия'),'нет числового вида');
    for(const [id,name] of [['broken_links','Битые ссылки'],['duplicates','Дубли'],['contradictions','Противоречия']]) {
      btn(kind(name),'Запустить').click();
      await wait(async()=>{const list=await call('audit_list',{project});const a=list.audits.find(a=>a.kind===id);return a?.last_status==='complete';},`не завершён ${id}`);
      await wait(()=>btn(kind(name),'Показать находки')&&!btn(kind(name),'Показать находки').disabled,`нет результата ${id}`);
      btn(kind(name),'Показать находки').click();
      if(id!=='duplicates') await wait(()=>kind(name).querySelector('.finding'),`нет находки ${id}`);
      report.checks.push(`${id}: launch through UI / real worker / full result`);
    }
    assert(kind('Противоречия').textContent.includes('30 ↔ 60'),'нет объяснения значений');
    assert(kind('Противоречия').querySelectorAll('.subject').length===2,'нет обеих ссылок');
    btn(kind('Противоречия'),'Игнорировать').click();
    await wait(()=>!kind('Противоречия').querySelector('.finding'),'игнор не скрыл находку');
    const filter=[...page.querySelectorAll('label')].find(l=>l.textContent.trim()==='Игнорируемые').querySelector('input');
    filter.click(); await wait(()=>btn(kind('Противоречия'),'Вернуть'),'игнорируемая находка не показана');
    const restore=btn(kind('Противоречия'),'Вернуть');
    report.restoreInitiallyDisabled=restore.disabled;
    await wait(()=>!restore.disabled,'кнопка возврата не стала доступна после записи игнора');
    restore.click();await wait(()=>btn(kind('Противоречия'),'Игнорировать'),'возврат не применён');
    report.checks.push('ignore / show ignored / restore through UI and persistent API');
    const select=kind('Противоречия').querySelector('select');select.value='weekly';select.dispatchEvent(new Event('change',{bubbles:true}));
    await wait(async()=>(await call('audit_list',{project})).audits.find(a=>a.kind==='contradictions').schedule==='weekly','расписание не записано');
    report.checks.push('weekly schedule stored by worker');
    const before=(await call('audit_list',{project})).audits.find(a=>a.kind==='broken_links');
    report.beforeNotification=before;
    await invoke('audits_probe_add_link');
    const runButton=btn(kind('Битые ссылки'),'Запустить');
    assert(runButton && !runButton.disabled,'повторный запуск недоступен: предыдущее действие экрана ещё не завершено');
    btn(kind('Битые ссылки'),'Запустить').click();
    await wait(async()=>{const a=(await call('audit_list',{project})).audits.find(a=>a.kind==='broken_links');report.afterNotification=a;return a.last_status==='complete' && a.result_run_id!==before.result_run_id;},'нет нового полного запуска битых ссылок');
    assert(report.afterNotification.new_count===1,'воркер не отметил единственную новую ссылку');
    await wait(()=>Number(document.querySelector('button[aria-label^="Уведомления, непрочитанных:"]')?.getAttribute('aria-label').split(':').at(-1))>0,'нет уведомления о новой находке');
    document.querySelector('button[aria-label^="Уведомления, непрочитанных:"]').click();
    const notice=await wait(()=>btn(document,'Открыть аудиты →'),'нет перехода из центра уведомлений');notice.click();
    report.checks.push('new finding notification / jump to audits');
    for(const theme of ['dark','light']) for(const width of [1440,1024,760]) {
      document.documentElement.dataset.theme=theme;await invoke('audits_probe_size',{width});await frame();await frame();
      assert(document.documentElement.scrollWidth<=innerWidth+1,`${theme}/${width}: horizontal overflow`);
      assert(kind('Противоречия').getBoundingClientRect().width>100,`${theme}/${width}: lost audit card`);
      report.checks.push(`${theme}/${width}: layout`);
    }
    kind('Противоречия').querySelector('.subject').click();
    await wait(()=>document.querySelector('.cm-content')?.textContent.includes('Время ожидания ответа сервера'),'находка не открыла исходный документ в редакторе');
    report.checks.push('source document opens in product editor through product Tauri commands');
    if(real) {
      await invoke('audits_probe_size',{width:1440});
      await call('reindex',{cwd:project,full:true});
      await wait(async()=>{const s=await call('status',{cwd:project});const tree=await call('tree',{cwd:project});return s.state==='ready' && s.pending===0 && tree.documents.some(d=>d.path==='docs/new.md');},'исходный индекс не готов для сбора тем');
      const long='Аудит журналов возврата в боксы и предыдущей попытки входа результаты сессий';
      const short='журналы возврата боксы';
      assert(await invoke('audits_probe_mcp_denied'),'очистка тем доступна MCP');
      const mcp=await invoke('audits_probe_mcp_search',{query:long});
      assert(mcp.results.length===0,'MCP fixture: ожидался успешный пустой поиск');
      const windowSearch=await call('search',{cwd:project,query:short,min_score:1,rerank:false,_window:{request_id:'probe-real-window'}});
      assert(windowSearch.results.length===0,'Window fixture: ожидался успешный пустой поиск');
      const list=await wait(async()=>{const l=await call('new_topics_list',{project});return l.topics.reduce((n,t)=>n+t.observations,0)===2 && l;},'обращения не записаны');
      assert(list.topics.length===1 && list.topics[0].variants===2 && list.topics[0].sources.length===2,'ISS-19: обращения не объединены или источник потерян');
      report.realTopicsBefore=list.topics[0];
      await call('search',{cwd:project,query:short,min_score:1,rerank:false,_window:{purpose:'diagnostic'}});
      assert((await call('new_topics_list',{project})).topics[0].observations===2,'диагностика увеличила спрос');
      report.checks.push('real bge-m3 / named MCP pipe + Window search / one topic, two variants and origins; diagnostic excluded');
      await invoke('audits_probe_add_knowledge');
      await call('reindex',{cwd:project,full:true});
      await wait(async()=>{const s=await call('status',{cwd:project});const tree=await call('tree',{cwd:project});return s.state==='ready' && s.pending===0 && tree.documents.some(d=>d.path==='docs/knowledge.md');},'новый документ не проиндексирован');
      assert((await call('new_topics_list',{project})).topics[0].observations===2,'переиндексация потеряла тему');
      // Обычный search не принимает rerank. Загрузку инициирует штатная проверка знаний.
      const fresh=await call('new_topics_get',{project,id:list.topics[0].id});
      report.initialKnowledgeCheck=await call('new_topics_check',{project,id:fresh.id,revision:fresh.revision});
      await wait(async()=>{const m=await call('models_info');return m.runtime?.rerank?.state==='ready';},'rerank не готов');
      report.readyModels=(await call('models_info')).runtime;
      document.querySelector('.topics-row[title="Вопросы, для которых не нашлись материалы"]').click();
      const row=await wait(()=>document.querySelector(`[data-new-topic-id="${list.topics[0].id}"]`),'тема не появилась в окне');row.click();
      const check=await wait(()=>btn(document,'Проверить знания'),'нет проверки знаний');check.click();
      const found=await wait(async()=>{const d=await call('new_topics_get',{project,id:list.topics[0].id});report.realCheck=d;return d.status==='materials_found' && d;},'проверка не нашла добавленный документ');
      assert(found.observations===2,'проверка темы увеличила спрос');
      const resolve=await wait(()=>btn(document,'Знаний достаточно'),'нет решения пользователя');
      await wait(()=>!resolve.disabled,'решение пока недоступно');resolve.click();
      await wait(async()=>{const d=await call('new_topics_get',{project,id:list.topics[0].id});return d.status==='resolved';},'решение пользователя не сохранилось');
      report.checks.push('real model native flow: MCP → topic → document → full reindex preserves topic → rerank knowledge check → user resolves');
    }
    report.success=true;
  } catch(error) {
    report.error=String(error?.stack??error);
    report.bell=document.querySelector('button[aria-label^="Уведомления, непрочитанных:"]')?.outerHTML;
    if(project) try {report.audits=await call('audit_list',{project});} catch {}
    if(project) try {report.status=await call('status',{cwd:project});report.models=(await call('models_info')).runtime;report.topics=await call('new_topics_list',{project});} catch {}
  }
  await invoke('audits_probe_finish',{result:report});
})();
