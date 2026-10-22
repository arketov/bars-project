// Интеграционные проверки физического графа в собранном приложении; программные действия, не физическая приёмка.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const frames = () => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const wait = (predicate, label, seconds = 25) => new Promise((resolve, reject) => {
    const deadline = performance.now() + seconds * 1000;
    const check = () => { const value = predicate(); if (value) resolve(value); else if (performance.now() > deadline) reject(Error(label)); else setTimeout(check, 50); };
    check();
  });
  const assert = (condition, label) => { if (!condition) throw Error(label); };
  const button = text => [...document.querySelectorAll('button')].find(b => b.textContent.trim() === text);
  const store = window.sessionStorage;
  const projects = await invoke('graph_probe_projects');
  const stage = Number(store.getItem('graphProbeStage') || '1');
  const report = JSON.parse(store.getItem('graphProbeReport') || 'null') ?? { success: false, native: navigator.userAgent, checks: [], projects: {} };
  const path = projects[stage - 1];
  const name = path.split(/[\\/]/).at(-1);
  try {
    if (localStorage.getItem('bars-project') !== path) {
      localStorage.setItem('bars-project', path);
      store.setItem('graphProbeStage', String(stage));
      store.setItem('graphProbeReport', JSON.stringify(report));
      location.reload();
      return;
    }
    const metrics = { name };
    // Мини-граф ждёт индексацию копии документации и автоматическое построение графа воркером.
    const mini = await wait(() => { const p = document.querySelector('.pg.mini'); return p && p.dataset.loaded === 'true' && Number(p.dataset.nodes) > 0 ? p : null; }, `${name}: мини-граф не получил срез`, 140);
    let sawRunning = false;
    const motion = setInterval(() => { if (mini.dataset.motion === 'running') sawRunning = true; }, 20);
    metrics.mini = { nodes: Number(mini.dataset.nodes), links: Number(mini.dataset.links), mode: mini.dataset.mode };
    assert(metrics.mini.nodes > 50, `${name}: мало документов в мини-графе`);
    report.checks.push(`${name}: мини-граф получил срез из воркера (${metrics.mini.nodes} документов, ${metrics.mini.links} ссылок)`);

    // Срез по настоящему IPC: контракт, индексы рёбер, пределы и устаревший снимок.
    const call = (method, request) => invoke('bars_call', { method, params: { cwd: path, project: path, request } });
    const started = performance.now();
    const slice = await call('graph_physical', {});
    metrics.ipcMs = Math.round(performance.now() - started);
    assert(slice.method === 'physical-v1' && slice.schema_version === 1, 'Метод среза');
    assert(slice.nodes.length === slice.shown.documents && slice.shown.documents === slice.totals.documents, 'Не все документы в срезе по умолчанию');
    assert(slice.edges.every(e => e.from >= 0 && e.to >= 0 && e.from < slice.nodes.length && e.to < slice.nodes.length), 'Индекс ребра вне узлов');
    assert(slice.used_bytes <= slice.applied.max_bytes, 'Срез превысил байтовый предел');
    const byKind = kind => slice.edges.filter(e => e.kind === kind);
    metrics.slice = { documents: slice.totals.documents, chunks: slice.totals.chunks, links: byKind('markdown_link').length, linksTotal: slice.totals.links, documentPairs: byKind('document_similarity').length, documentPairsTotal: slice.totals.document_pairs, bytes: slice.used_bytes, truncated: slice.truncated, layers: Object.fromEntries(Object.entries(slice.layers).map(([k, v]) => [k, v.state])) };
    assert(byKind('markdown_link').length > 0 && byKind('markdown_link').every(e => e.strength === 1 && e.raw === null), 'Ссылки');
    assert(byKind('document_similarity').every(e => e.raw !== null && e.raw >= 0.5 && e.strength >= 0 && e.strength <= 1), 'Близость документов');
    const small = await call('graph_physical', { limits: { max_documents: 5, max_chunks: 500, max_edges: 4000, max_bytes: 1048576 } });
    assert(small.truncated && small.shown.documents === 5 && small.limit_reasons.length > 0, 'Усечение по числу документов');
    let rejected = '';
    try { await call('graph_physical', { snapshot: 'старый' }); } catch (error) { rejected = JSON.stringify(error); }
    assert(rejected.includes('снимок'), 'Устаревший снимок не отклонён');
    report.checks.push(`${name}: IPC graph_physical — ${metrics.slice.documents} документов, ${metrics.slice.links} ссылок, ${metrics.slice.documentPairs} пар близости, ${metrics.slice.bytes} байт за ${metrics.ipcMs} мс; усечение и устаревший снимок`);

    // Физика идёт в Web Worker приложения: движение начато и закончено ответом worker (idle ставит только его порция).
    assert(performance.getEntriesByType('resource').some(e => e.name.includes('physics.worker')), 'Нет загрузки physics.worker');
    await wait(() => mini.dataset.motion === 'idle' && sawRunning, `${name}: физика не прошла запуск и покой`, 60);
    clearInterval(motion);
    report.checks.push(`${name}: worker физики загружен приложением, движение началось и закончилось по ответам worker`);

    // Полный граф вкладкой: режимы, раскрытие документа порциями.
    button('Граф').click(); await frames();
    const full = await wait(() => { const p = document.querySelector('.pg:not(.mini)'); return p && p.dataset.loaded === 'true' && Number(p.dataset.nodes) > 0 ? p : null; }, `${name}: полный граф не загружен`);
    metrics.full = { nodes: Number(full.dataset.nodes), links: Number(full.dataset.links), mode: full.dataset.mode };
    assert(metrics.full.nodes === metrics.slice.documents, 'Полный граф показал не все документы');
    const mode = label => [...document.querySelectorAll('.pg:not(.mini) [role="radio"]')].find(b => b.textContent.replace(/\s+/g, ' ').trim().startsWith(label));
    mode('Все связи').click(); await frames(); await sleep(150);
    metrics.full.allLinks = Number(full.dataset.links);
    assert(full.dataset.mode === 'all' && metrics.full.allLinks >= metrics.full.links, 'Режим «Все связи»');
    mode('По смыслу').click(); await frames(); await sleep(150);
    metrics.full.meaningLinks = Number(full.dataset.links);
    assert(full.dataset.mode === 'meaning', 'Режим «По смыслу»');
    mode('Ссылки').click(); await frames(); await sleep(150);
    assert(full.dataset.mode === 'links' && Number(full.dataset.links) === metrics.full.links, 'Возврат в режим «Ссылки»');
    report.checks.push(`${name}: полный граф — ссылки ${metrics.full.links}, по смыслу ${metrics.full.meaningLinks}, все связи ${metrics.full.allLinks}`);

    const biggest = slice.nodes.reduce((a, b) => (b.chunks > a.chunks ? b : a));
    const find = document.querySelector('.pg:not(.mini) input[type="search"]');
    find.focus(); find.value = biggest.label; find.dispatchEvent(new Event('input', { bubbles: true }));
    find.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })); await frames();
    const expand = await wait(() => [...document.querySelectorAll('.pg:not(.mini) .inspector button')].find(b => b.textContent.trim().startsWith('Раскрыть чанки')), `${name}: нет действия «Раскрыть чанки»`);
    const before = Number(full.dataset.nodes);
    const t0 = performance.now();
    expand.click();
    await wait(() => Number(full.dataset.nodes) > before, `${name}: чанки не появились`);
    metrics.expandMs = Math.round(performance.now() - t0);
    const expected = Math.min(50, biggest.chunks);
    assert(Number(full.dataset.nodes) - before === expected, `${name}: ожидалось ${expected} чанков, получено ${Number(full.dataset.nodes) - before}`);
    metrics.expanded = { document: biggest.label, chunks: biggest.chunks, shown: expected };
    if (biggest.chunks > 50) assert(document.querySelector('.pg:not(.mini) .trunc')?.textContent.includes(`Показано ${expected} из ${biggest.chunks}`), 'Нет признака усечения чанков');
    report.checks.push(`${name}: раскрытие «${biggest.label}» — ${expected} из ${biggest.chunks} чанков за ${metrics.expandMs} мс`);

    // Размеры окна: граф остаётся загруженным, инспектор уходит под карту при узкой панели.
    for (const [width, height] of [[1024, 760], [760, 760], [1440, 850]]) {
      await invoke('graph_probe_size', { width, height }); await sleep(400);
      const panel = document.querySelector('.pg:not(.mini)');
      assert(panel && panel.dataset.loaded === 'true' && panel.getBoundingClientRect().width > 100, `${name}: панель на ${width}px`);
    }
    report.checks.push(`${name}: окна 1024, 760 и 1440 px без потери среза`);
    report.projects[name] = metrics;
    if (stage < projects.length) {
      store.setItem('graphProbeStage', String(stage + 1));
      store.setItem('graphProbeReport', JSON.stringify(report));
      location.reload();
      return;
    }
    report.success = true;
  } catch (error) {
    report.error = String(error && error.message ? error.message : error);
    report.stage = name;
    report.dom = document.querySelector('.pg:not(.mini)')?.outerHTML.slice(0, 400) ?? null;
  }
  await invoke('graph_probe_finish', { result: report });
})();
