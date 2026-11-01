// Собранное окно, настоящий IPC и изолированный FakeProvider; рабочий профиль не меняется.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const checks = [];
  const assert = (value, label) => { if (!value) throw Error(label); checks.push(label); };
  try {
    const {projects} = await invoke("bars_call", {method:"projects",params:{}});
    const project = projects[0].path;
    const call = request => invoke("bars_call", {method:"graph_query",params:{project,cwd:project,request}});
    for (const [path,text] of [["docs/target.md","# Требования\n\nТ-2 использует Т-3.\n"],["docs/dependent.md","# Поведение\n\nТ-1 зависит от Т-2.\n"]]) {
      const saved = await invoke("document_write", {project,path,text,baseRevision:null,eol:"lf",bom:false});
      assert(saved.status === "saved", `Создан ${path}`);
    }
    await invoke("bars_call", {method:"reindex",params:{project,cwd:project,full:false}});
    const deadline = performance.now() + 35000;
    let ready = false;
    while (performance.now() < deadline) {
      const status = await invoke("bars_call", {method:"graph_status",params:{project,cwd:project}});
      if (status.source_count >= 3 && status.freshness === "current" && !status.active_job) { ready = true; break; }
      await new Promise(resolve => setTimeout(resolve,100));
    }
    assert(ready, "Граф временных документов построен");
    const impact = await call({operation:"impact",documents:["target.md"]});
    assert(impact.items.length > 0 && impact.nodes.some(n => n.label === "Т-1"), "Влияние найдено по пути документа");
    const context = await call({operation:"context",query:"использует",documents:["docs/target.md"],budgets:{max_depth:0}});
    assert(context.items.length > 0 && context.sources.every(s => s.docs_path === "target.md"), "Контекст ограничен документом до ранжирования");
    const local = await call({operation:"graph",view:"mixed",local:true,documents:["target.md"]});
    assert(local.nodes.length > 0 && local.nodes.every(n => !n.namespace.includes(":/")), "Локальный граф без внутренних путей");
    const path = await call({operation:"paths",path_kind:"structural",from:{document:"target.md"},to:{document:"docs/target.md"}});
    assert(path.items.length > 0, "Концы пути принимают документы");
    let rejected = "";
    try { await call({operation:"impact",documents:["missing.md"]}); } catch (error) { rejected = JSON.stringify(error); }
    assert(rejected.includes("next:"), "Неизвестный документ даёт следующий шаг");
    assert(document.querySelector("main") || document.querySelector(".workspace"), "Собранный интерфейс открыт");
    await invoke("workspace_probe_finish", {result:{success:true,checks,native:navigator.userAgent}});
  } catch (error) {
    await invoke("workspace_probe_finish", {result:{success:false,checks,error:String(error),stack:error?.stack}});
  }
})();
