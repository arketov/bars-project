// Настоящий UI/IPC, изолированные диск и профиль. Не имитация физической клавиатуры.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke, checks=[];
  const wait = (predicate,label) => new Promise((resolve,reject) => {
    const end=performance.now()+15000;
    const check=()=>{const value=predicate();if(value)resolve(value);else if(performance.now()>end)reject(Error(label));else setTimeout(check,30);}; check();
  });
  const assert=(ok,label)=>{if(!ok)throw Error(label);checks.push(label);};
  const field=()=>document.querySelector('textarea[aria-label="Поисковый запрос"]');
  const input=(node,value)=>{node.value=value;node.dispatchEvent(new Event('input',{bubbles:true}));};
  async function query(value) {field().focus();input(field(),value);field().dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));await wait(()=>document.querySelector('.text-results')&&!document.querySelector('.text-results [role="status"]'),'текстовая выдача');}
  try {
    await wait(field,'поиск загрузился');
    const projects=await invoke('bars_call',{method:'projects',params:{}}), project=projects.projects[0].path;
    const rpc=params=>invoke('bars_call',{method:'text_search',params:{project,query:'привет',request_id:crypto.randomUUID(),...params}});
    const disk=await rpc({});assert(disk.hits_total===2,'IPC: Unicode без регистра');
    assert(disk.documents[0].hits[0].column===4,'IPC: координата после emoji в UTF-16');
    assert((await rpc({case_sensitive:true})).hits_total===1,'IPC: учёт регистра');
    try {await rpc({query:'[',regex:true});throw Error('не отклонено');} catch(e) {assert(e.data?.kind==='invalid_regex','IPC: ошибка regex с указателем позиции');}
    await query('привет');
    assert(document.querySelectorAll('.text-results .occurrence').length===2,'UI: два вхождения независимо от смысловой выдачи');
    document.querySelector('.text-results .occurrence').click();
    await wait(()=>document.querySelector('.cm-editor .cm-hit'),'диапазон редактора');
    assert(document.querySelector('.cm-editor .cm-hit').textContent==='ПрИвЕт','UI: точная подсветка диапазона, не всей строки');
    const read=await invoke('document_read',{project,path:'docs/Тема.md'});
    // Правка через настоящий редактор: поиск должен сбросить буфер до embed.
    const editor=await wait(()=>document.querySelector('.cm-content[contenteditable="true"]'),'редактор готов к вводу');editor.focus();
    document.getSelection().selectAllChildren(editor);
    assert(document.execCommand('insertText',false,'# Проверка\nСВЕЖЕЕ_СЛОВО_23\n'),'WebView2 принял insertText');
    await wait(()=>document.querySelector('.document')?.textContent.includes('Изменено'),'CodeMirror передал правку в буфер');
    await query('СВЕЖЕЕ_СЛОВО_23');
    await wait(()=>document.querySelector('.text-results mark')?.textContent==='СВЕЖЕЕ_СЛОВО_23','свежий текст');
    const saved=await invoke('document_read',{project,path:'docs/Тема.md'});
    assert(saved.text.includes('СВЕЖЕЕ_СЛОВО_23'),'UI → запись → text_search: свежий буфер на диске');
    const samples=[];for(let i=0;i<30;i++){const start=performance.now();await rpc({query:'СВЕЖЕЕ_СЛОВО_23'});samples.push(performance.now()-start);}
    const sorted=[...samples].sort((a,b)=>a-b);assert(sorted[28]<300,'IPC на фикстуре: p95 < 300 мс');
    for(const theme of ['light','dark']) {
      document.documentElement.dataset.theme=theme;
      for(const width of [1440,1024,760]) {
        await invoke('text_search_probe_size',{width}); await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
        const pane=document.querySelector('.text-results');assert(pane.scrollWidth<=pane.clientWidth+1,`${theme}/${width}: текстовая выдача без переполнения`);
      }
    }
    await invoke('text_search_probe_finish',{result:{success:true,checks,samples,p95_ms:sorted[28]}});
  } catch(e) {await invoke('text_search_probe_finish',{result:{success:false,checks,error:String(e),stack:e.stack}});}
})();
