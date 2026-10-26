// Настоящий Tauri/worker, временные проект и профиль; программная проверка без физического ввода.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const checks = [];
  const assert = (value, message) => { if (!value) throw Error(message); checks.push(message); };
  const wait = async (read, label) => {
    const end = performance.now() + 45000;
    while (performance.now() < end) {
      const value = await read();
      if (value) return value;
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    throw Error(label);
  };
  try {
    const projects = await invoke('bars_call', { method: 'projects', params: {} });
    const project = projects.projects[0].path;
    const call = (method, params = {}) => invoke('bars_call', { method, params: { cwd: project, project, ...params } });
    const ready = async () => {
      const status = await call('graph_status');
      return !status.active_job && status.snapshot && status.state === 'ready' ? status : null;
    };
    const before = await wait(ready, 'Начальный граф');
    assert(before.layers.structure.state === 'ready', 'Структура готова');
    assert(before.layers.semantic.state === 'partial', 'Смысловая полнота не завышена');
    assert(before.progress.phase === 'publish', 'Прогресс дошёл до публикации');
    assert(before.file_errors_total === 0, 'Ошибок файлов нет');
    await invoke('graph_lifecycle_probe_external');
    const after = await wait(async () => { const s = await ready(); return s && s.snapshot !== before.snapshot ? s : null; }, 'Автообновление после правки');
    assert(after.layers.structure.generation !== before.layers.structure.generation, 'Новое поколение структуры после правки');
    try {
      await call('graph_query', { request: { operation: 'graph', snapshot: before.snapshot } });
      throw Error('Старый снимок принят');
    } catch (error) {
      assert(error.data?.kind === 'snapshot_expired', 'Старый снимок отклонён');
    }
    const accepted = await call('graph_build', { layer: 'structure' });
    assert(accepted.accepted && accepted.job_id, 'Явная перестройка принята');
    const rebuilt = await wait(ready, 'Перестройка');
    assert(rebuilt.snapshot === after.snapshot, 'Неизменные исходники дают тот же снимок');
    assert(rebuilt.layers.semantic.generation === after.layers.semantic.generation && rebuilt.layers.similarity.generation === after.layers.similarity.generation, 'Выбранный слой сохраняет другие актуальные поколения');
    await invoke('graph_lifecycle_probe_stale');
    assert((await call('graph_status')).state === 'stale', 'Граф устарел без правок файлов');
    const scan = await call('reindex', { full: false, accept_model_change: false });
    assert(scan.accepted, 'Проверка без переиндексации принята');
    const checked = await wait(ready, 'Scan без изменений восстановил граф');
    assert(checked.snapshot === rebuilt.snapshot, 'Scan без правок восстановил тот же снимок');
    const graphButton = [...document.querySelectorAll('button')].find(button => button.textContent.trim() === 'Граф');
    assert(graphButton, 'В собранном окне доступен вход в граф');
    graphButton.click();
    await wait(() => graphButton.classList.contains('active') && document.querySelector('canvas'), 'Карта графа в собранном окне');
    assert(document.querySelector('canvas'), 'Карта открылась после автообновления');
    await invoke('graph_lifecycle_probe_finish', { result: { success: true, native: navigator.userAgent, checks, before, after, rebuilt, checked } });
  } catch (error) {
    await invoke('graph_lifecycle_probe_finish', { result: { success: false, checks, error: error.message ?? String(error), detail: error, stack: error.stack } });
  }
})();
