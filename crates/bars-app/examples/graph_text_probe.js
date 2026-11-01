// Настоящий собранный UI + изолированный worker/FakeProvider; пользовательский профиль не затрагивается.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const checks = [];
  const assert = (value, label) => { if (!value) throw Error(label); checks.push(label); };
  const wait = async (read, label) => {
    const deadline = performance.now() + 35000;
    while (performance.now() < deadline) { const value = await read(); if (value) return value; await new Promise(resolve => setTimeout(resolve, 100)); }
    throw Error(label);
  };
  try {
    const projects = await invoke("bars_call", {method:"projects",params:{}});
    const project = projects.projects[0].path;
    const call = (method, params={}) => invoke("bars_call",{method,params:{cwd:project,project,...params}});
    for (const [path,text] of [["docs/Факты.md","# Основания\n\nТ-1 зависит от Т-2.\n\nТ-2 зависит от Т-3.\n"],["docs/Применение.md","# Применение\n\n[Основания](Факты.md)\n\nТ-3 использует Т-4.\n"]]) {
      const result=await invoke("document_write",{project,path,text,baseRevision:null,eol:"lf",bom:false});
      assert(result.status==="saved",`Создан временный документ ${path}`);
    }
    await call("reindex",{full:false});
    await wait(async()=>{const status=await call("graph_status");return status.source_count>=3 && status.freshness==="current" && !status.active_job;},"Не построен граф временных документов");
    const relations=await call("graph_query",{text:true,request:{operation:"relations",query:"Т-1"}});
    assert(relations.items.some(item=>item.type==="relation"),"Tauri получил настоящие отношения воркера");
    const input=await wait(()=>document.querySelector('textarea[aria-label="Поисковый запрос"]'),"Нет поля запроса");
    const mode=document.querySelector('select[aria-label="Режим поиска"]');
    mode.value="mix";mode.dispatchEvent(new Event("change",{bubbles:true}));
    input.value="Основания";input.dispatchEvent(new Event("input",{bubbles:true}));
    await wait(()=>[...document.querySelectorAll("button")].find(button=>button.textContent.trim()==="Найти" && !button.disabled),"Не готов поиск").then(button=>button.click());
    const bundle=await wait(()=>document.querySelector(".graph-search-bundle"),"Нет списка связанных источников");
    assert(bundle.textContent.includes("Связанные источники"),"Связи отделены от основных результатов");
    assert(!bundle.textContent.includes('"requested"') && !bundle.textContent.includes('"evidence"'),"Список читаемый, без сырого DTO");
    assert(!/[a-f0-9]{64}/.test(bundle.textContent),"Нет внутренних хешей в списке");
    assert(bundle.querySelectorAll("article").length>0,"Показаны настоящие связанные пакеты");
    assert(bundle.querySelectorAll("pre").length>0,"Показаны цитаты оснований");
    await invoke("workspace_probe_finish",{result:{success:true,native:navigator.userAgent,checks}});
  } catch(error) { await invoke("workspace_probe_finish",{result:{success:false,checks,error:String(error),stack:error?.stack,body:document.body.innerText.slice(-2500)}}); }
})();
