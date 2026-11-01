// Настоящий UI и IPC, только собственный проект workspace_probe/FakeProvider.
(async () => {
  const invoke=window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);
  const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const checks=[], freshness=[];
  const assert=(ok,label)=>{if(!ok)throw Error(label);checks.push(label);void invoke("workspace_probe_progress",{result:{checks:[...checks]}}).catch(()=>{});};
  const wait=async(read,label,timeout=10000)=>{const end=performance.now()+timeout;while(performance.now()<end){const value=await read();if(value)return value;await sleep(30);}throw Error(label);};
  let binding=null;
  const nativeFetch=window.fetch.bind(window);
  window.fetch=async(url,options)=>{
    const response=await nativeFetch(url,options);
    if(String(url).includes("/window_observe")){
      const json=response.json.bind(response);response.json=async()=>{const value=await json();if(value)binding=value;return value;};
    }
    return response;
  };
  const report={success:false,checks,freshness_ms:freshness};
  try{
    const {projects}=await invoke("bars_call",{method:"projects",params:{}});const project=projects[0].path;
    const text=()=>document.querySelector(".tree")?.textContent??"";
    const paths=()=>JSON.stringify([...document.querySelectorAll(".tree [data-tree-node]")].map(node=>node.dataset.treeNode).sort());
    await wait(()=>document.querySelector(".tree .caret"),"Нет корня дерева настоящего проекта");
    for(const caret of document.querySelectorAll(".tree .caret"))caret.click();
    await wait(()=>text().includes("Тема"),"Нет первого снимка настоящего проекта");
    // Listener установлен раньше снимка; получение повторной подписки идёт через штатный heartbeat.
    if(!binding){await invoke("window_observe",{project:null,parts:["disk","index","status"]});await wait(()=>binding,"Heartbeat не восстановил подписку",15000);}
    assert(binding.project===project,"Первый снимок и восстановление подписки привязаны к проекту");
    const created=await invoke("entry_create_doc",{project,parent:"docs",name:"Новая заметка"});assert(created.status==="ok","Собственная команда создала документ");
    const own=performance.now();await wait(()=>text().includes("Новая заметка"),"Собственное создание не видно",2000);freshness.push(performance.now()-own);
    const external=async action=>{const start=performance.now();await invoke("workspace_probe_external",{action});return start;};
    let start=await external("create");await wait(()=>text().includes("External.v2"),"Не видна внешняя папка с точкой",2000);freshness.push(performance.now()-start);
    start=await external("rename");await wait(()=>text().includes("Renamed.v3")&&!text().includes("External.v2"),"Не видно внешнее переименование",2000);freshness.push(performance.now()-start);
    start=await external("remove");await wait(()=>!text().includes("Renamed.v3"),"Не видно внешнее удаление",2000);freshness.push(performance.now()-start);
    assert(true,"Внешние create/rename/delete папки с точкой видны до 2 секунд");
    const before=paths();await external("break_root");
    const error=await invoke("window_snapshot",{subscription:binding.subscription,known:{},force:true});
    assert(error.parts.disk.state==="error","Отказ диска не публикуется как пустой успешный снимок");
    await sleep(200);assert(paths()===before,"При отказе сохраняются прежние файлы и папки дерева");
    await external("restore_root");await invoke("window_snapshot",{subscription:binding.subscription,known:{},force:true});
    await wait(()=>text().includes("Новая заметка"),"Дерево не восстановилось");
    assert(true,"Успешная сверка восстанавливает дерево после ошибки");
    const file=[...document.querySelectorAll(".tree .filename")].find(node=>node.textContent.includes("Тема"));file?.click();
    const content=await wait(()=>document.querySelector('.document .cm-content[contenteditable="true"]'),"Нет готового редактора документа");
    await wait(()=>content.textContent.includes("Настоящий документ"),"Документ ещё не загружен в редактор");
    content.focus();await sleep(50);assert(document.execCommand("insertText",false,"Проверка сохранности буфера "),"CodeMirror принял ввод");await sleep(50);
    const draft=content.textContent;assert(draft.includes("Проверка сохранности"),"В редактор внесена пробная правка");
    await invoke("window_snapshot",{subscription:binding.subscription,known:{},force:true});
    assert(content.textContent===draft,"Обновление дерева не заменяет буфер редактора");
    await invoke("workspace_probe_windows",{count:4,visible:true});await sleep(2000);
    const four=await invoke("workspace_probe_metrics");assert(four.coordinator.projects===1,"Четыре окна разделяют один проект координатора");
    await invoke("workspace_probe_windows",{count:4,visible:false});await sleep(1500);
    const hidden=await invoke("workspace_probe_metrics");await external("create");await sleep(1500);const still=await invoke("workspace_probe_metrics");
    assert(hidden.coordinator.projects===0&&hidden.coordinator.cache_bytes===0,"Последнее скрытое окно освобождает общий снимок");
    assert(JSON.stringify(hidden.coordinator.builds)===JSON.stringify(still.coordinator.builds),"У скрытого проекта нет фоновых обходов");
    assert(!text().includes("External.v2"),"Скрытое дерево ещё не получило внешнюю правку");
    const shown=performance.now();await invoke("workspace_probe_windows",{count:1,visible:true});await wait(()=>text().includes("External.v2")&&text().includes("Новая заметка"),"Показ окна не восстановил свежий снимок",2000);report.show_freshness_ms=performance.now()-shown;
    assert(true,"Показ окон восстанавливает подписки и пропущенные изменения");
    report.success=true;
  }catch(error){report.error=String(error);report.failure={hidden:document.hidden,tree:document.querySelector(".tree")?.textContent,binding,metrics:await invoke("workspace_probe_metrics")};}
  await invoke("workspace_probe_finish",{result:report});
})();
