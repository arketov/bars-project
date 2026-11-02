// Продуктовый WebView2 и настоящий изолированный worker/FakeProvider; без рабочего профиля.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const checks = [];
  const assert = (value, label) => { if (!value) throw Error(label); checks.push(label); };
  const wait = async (read, label) => {
    const until = performance.now() + 35000;
    while (performance.now() < until) { const value = await read(); if (value) return value; await new Promise(resolve => setTimeout(resolve, 100)); }
    throw Error(label);
  };
  try {
    const { projects } = await invoke("bars_call", { method: "projects", params: {} });
    const project = projects[0].path;
    const call = request => invoke("bars_call", { method: "graph_query", params: { project, cwd: project, request } });
    const text = "# Основания\n\nЕсли выбран CPU, Т-1 зависит от Т-2.\n\nТ-2 использует Т-3.\n\nТ-4 не использует Т-3.\n";
    const saved = await invoke("document_write", { project, path: "docs/Факты.md", text, baseRevision: null, eol: "lf", bom: false });
    assert(saved.status === "saved", "Создан изолированный корпус оснований");
    await invoke("bars_call", { method: "reindex", params: { project, cwd: project, full: false } });
    await wait(async () => { const s = await invoke("bars_call", { method: "graph_status", params: { project, cwd: project } }); return s.source_count >= 2 && s.freshness === "current" && !s.active_job; }, "Граф не построен");
    const endpoint = label => ({ label, kind: "entity" });
    const requests = {
      relations: { operation: "relations", query: "Т-2" },
      paths: { operation: "paths", from: endpoint("Т-1"), to: endpoint("Т-3"), direction: "outgoing" },
    };
    const relation = await call(requests.relations);
    const root = relation.nodes.find(n => n.label === "Т-3").reference;
    requests.impact = { operation: "impact", roots: [root] };
    for (const operation of ["paths", "impact"]) {
      const reply = await call(requests[operation]);
      const path = reply.items.find(item => item.path?.steps.length === 2)?.path;
      assert(path?.complete && path.nodes.length === 3, `${operation}: полная цепочка из двух переходов`);
      for (const [i, step] of path.steps.entries()) {
        const edge = reply.assertions.find(edge => edge.id === step.edge);
        assert(edge && edge.evidence.length && edge.polarity === "positive", `${operation}: шаг ${i + 1} положительный и обоснованный`);
        for (const id of edge.evidence) {
          const proof = reply.evidence.find(proof => proof.id === id);
          const bytes = new TextEncoder().encode(text);
          assert(proof && new TextDecoder().decode(bytes.slice(proof.span.start, proof.span.end)) === proof.quote, `${operation}: точная цитата шага ${i + 1}`);
        }
      }
      assert(reply.assertions.some(edge => edge.qualifiers.condition), `${operation}: условие сохранено`);
    }
    const negative = await call({ ...requests.paths, from: endpoint("Т-4") });
    assert(!negative.items.length, "Отрицание не становится путём по умолчанию");
    const timings = {};
    for (const [operation, request] of Object.entries(requests)) {
      await call(request);
      const samples = [];
      for (let i = 0; i < 20; i++) { const started = performance.now(); await call(request); samples.push(performance.now() - started); }
      samples.sort((a, b) => a - b);
      timings[operation] = { runs: 20, p50_ms: samples[9], p95_ms: samples[18], max_ms: samples[19] };
    }
    const input = await wait(() => document.querySelector('textarea[aria-label="Поисковый запрос"]'), "Нет поля поиска");
    const mode = document.querySelector('select[aria-label="Режим поиска"]');
    mode.value = "graph"; mode.dispatchEvent(new Event("change", { bubbles: true }));
    input.value = "Т-2"; input.dispatchEvent(new Event("input", { bubbles: true }));
    (await wait(() => [...document.querySelectorAll("button")].find(b => b.textContent.trim() === "Найти" && !b.disabled), "Поиск не готов")).click();
    const bundle = await wait(() => document.querySelector(".graph-search-bundle"), "Нет списка оснований");
    assert(bundle.querySelector("pre") && bundle.textContent.includes("Т-2"), "Собранный интерфейс показывает реальные цитаты отношений");
    await invoke("workspace_probe_finish", { result: { success: true, checks, timings, native: navigator.userAgent, limits: "Малый синтетический корпус, тёплый IPC, FakeProvider; не приёмка качества extraction или большого графа" } });
  } catch (error) {
    await invoke("workspace_probe_finish", { result: { success: false, checks, error: String(error), stack: error?.stack, body: document.body.innerText.slice(-2000) } });
  }
})();
