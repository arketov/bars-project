window.__BARS_BENCH_WAIT=true;
// Одна и та же программа для прежнего и нового UI; только изолированный workspace_probe.
(async () => {
  const direct = window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const wait = async (read, label, timeout = 120000) => {
    const end = performance.now() + timeout;
    while (performance.now() < end) { const value = await read(); if (value) return value; await sleep(100); }
    throw Error(label);
  };
  let counts = {}, bytes = 0, times = [], measuring = false, phase = 0;
  const encoder = new TextEncoder();
  const nativeFetch = window.fetch.bind(window);
  window.fetch = async (url, options) => {
    const uri = new URL(typeof url === "string" ? url : url.url ?? String(url));
    const command = decodeURIComponent(uri.pathname.slice(1));
    const monitored = measuring && uri.hostname === "ipc.localhost" && !command.startsWith("workspace_probe_");
    const currentPhase = phase, began = performance.now();
    let name = command;
    if (monitored && command === "bars_call") {
      try { name = JSON.parse(typeof options.body === "string" ? options.body : new TextDecoder().decode(options.body)).method; } catch { name = "bars_call"; }
    }
    if (monitored) counts[name] = (counts[name] ?? 0) + 1;
    const response = await nativeFetch(url, options);
    if (monitored) {
      const nativeJson = response.json.bind(response);
      response.json = async () => {
        const value = await nativeJson();
        if (measuring && phase === currentPhase) { bytes += encoder.encode(JSON.stringify(value) ?? "").length; times.push(performance.now() - began); }
        return value;
      };
    }
    return response;
  };
  const seconds = window.__BARS_BENCH_SECONDS ?? 60;
  const report = { success: false, scenario_seconds: seconds, samples: [], rpc_scope: "main WebView; CPU/IO/memory include native and all WebViews", bytes_kind: "UTF8 re-encoded JSON response bodies, excluding headers; encoding instrumentation included in WebView CPU", runtime: navigator.userAgent };
  try {
    await direct("workspace_probe_progress",{result:{stage:"reading_projects"}});
    const { projects } = await direct("bars_call", { method: "projects", params: {} });
    const project = projects[0].path;
    const call = (method, params = {}) => direct("bars_call", { method, params: { project, cwd: project, ...params } });
    await direct("workspace_probe_progress",{result:{stage:"waiting_for_index"}});
    let lastProgress=0;
    await wait(async () => {
      const activity = await call("activity");
      if(Date.now()-lastProgress>1000){lastProgress=Date.now();await direct("workspace_probe_progress",{result:{stage:"waiting_for_index",done:activity.running?.docs_done,total:activity.running?.docs_total,queue:activity.queue.length}});}
      return !activity.running && activity.queue.length === 0;
    }, "Индексация не завершена",900000);
    const stopGraph = async () => {
      for (let i = 0; i < 100; i++) {
        const graph = await call("graph_status");
        if (!graph.active_job) return;
        await call("graph_cancel", { job_id: graph.active_job }); await sleep(100);
      }
      throw Error("Не остановлен граф контрольного проекта");
    };
    await direct("workspace_probe_progress",{result:{stage:"stopping_graph"}});
    await stopGraph();
    await direct("workspace_probe_progress",{result:{stage:"reading_tree"}});
    report.documents = (await call("tree")).documents.length;
    if (report.documents !== (window.__BARS_BENCH_DOCUMENTS ?? 10000)) throw Error(`Число документов: ${report.documents}`);
    if(window.__BARS_BENCH_WAIT){await direct("workspace_probe_progress",{result:{stage:"prepared_waiting_for_gate",documents:report.documents}});await wait(()=>direct("workspace_probe_gate"),"Не получено разрешение измерителя",900000);}
    for (const scenario of (window.__BARS_BENCH_SCENARIOS ?? ["one", "hidden", "four"])) {
      await direct("workspace_probe_progress",{result:{scenario,stage:"warmup",samples:report.samples}});
      await direct("workspace_probe_windows", { count: scenario === "four" ? 4 : 1, visible: scenario !== "hidden" });
      await sleep(window.__BARS_BENCH_WARMUP_MS ?? 15000); await stopGraph(); await sleep(2000);
      for (let sample = 0; sample < (window.__BARS_BENCH_SAMPLES ?? 3); sample++) {
        await direct("workspace_probe_progress",{result:{scenario,sample,stage:"measuring",samples:report.samples}});
        const before = await direct("workspace_probe_metrics");
        counts = {}; bytes = 0; times = []; phase++; measuring = true;
        await sleep(seconds * 1000); measuring = false;
        const after = await direct("workspace_probe_metrics");
        const sorted = times.toSorted((a, b) => a - b);
        const processDelta = native => after.processes.filter(p => p.native === native).reduce((result, p) => {
          const old = before.processes.find(v => v.pid === p.pid);
          if (old) { result.cpu_seconds += p.cpu - old.cpu; result.logical_read_bytes += p.read - old.read; }
          else result.new_processes.push(p.pid);
          result.private_bytes += p.private; result.working_set_bytes += p.working;
          return result;
        }, { cpu_seconds: 0, logical_read_bytes: 0, private_bytes: 0, working_set_bytes: 0, new_processes: [] });
        report.samples.push({ scenario, sample, wall_seconds: (after.time - before.time) / 1000, main_rpc: counts, main_json_bytes: bytes,
          main_rpc_p95_ms: sorted[Math.floor(sorted.length * .95)] ?? 0, native: processDelta(true), webviews: processDelta(false),
          coordinator_before: before.coordinator, coordinator_after: after.coordinator, corpus_hash: after.corpus_hash,
          concurrent_compilers: [...new Set([...before.compilers, ...after.compilers])],
          exited_processes: before.processes.filter(p => !after.processes.some(v => v.pid === p.pid)).map(p => p.pid) });
        if (scenario === "one" && Object.keys(counts).length === 0) throw Error("RPC instrumentation did not observe IPC fetch");
        report.detected_ui = counts.window_snapshot ? "subscription" : (counts.tree_read ? "polling" : report.detected_ui);
      }
    }
    const query = {query:"Текст контрольного корпуса 42",modules:[],files:[],limit:5,min_score:0,_window:{purpose:"diagnostic"}};
    for(let i=0;i<3;i++)await call("search",query);
    const searchMs=[];let searchResult;
    for(let i=0;i<20;i++){const began=performance.now();searchResult=await call("search",query);searchMs.push(performance.now()-began);}
    if(!searchResult.results.length)throw Error("Контрольный поиск не вернул результатов");
    const ordered=searchMs.toSorted((a,b)=>a-b);
    report.search={scenario:"four",provider:"FakeProvider; fixed query, 3 warmups + 20 calls",query,applied:searchResult.applied,rerank_error:searchResult.rerank_error,settings_rerank_enabled:(await call("settings_get")).settings.rerank.enabled,milliseconds:searchMs,p50_ms:ordered[9],p95_ms:ordered[18],results:searchResult.results.map(r=>({path:r.path,source_hash:r.source_hash}))};
    report.success = true;
  } catch (error) { report.error = String(error); }
  await direct("workspace_probe_finish", { result: report });
})();
