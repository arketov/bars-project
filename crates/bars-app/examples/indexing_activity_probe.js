// Изолированный workspace_probe: настоящий UI, WebView2 и обработчик воркера.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const report = { success: false, checks: [] };
  const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
  const assert = (value, label) => { if (!value) throw Error(label); report.checks.push(label); };
  const wait = async (read, label) => {
    const end = performance.now() + 15_000;
    while (performance.now() < end) { if (await read()) return; await pause(100); }
    throw Error(label);
  };
  try {
    const { projects } = await invoke('bars_call', { method: 'projects', params: {} });
    const project = projects[0].path;
    const call = (method, params = {}) => invoke('bars_call', { method, params });
    const status = () => call('status', { cwd: project, project });
    await wait(async () => (await status()).state === 'ready', 'Начальная индексация');
    assert(!(await status()).watching, 'Скрытое окно не включает слежение');
    await invoke('workspace_probe_visibility', { visible: true });
    await wait(async () => (await status()).watching, 'UI не подтвердил открытый проект');
    assert((await status()).watching, 'Настоящий UI включает слежение открытого проекта');
    await invoke('workspace_probe_visibility', { visible: false });
    await call('project_observe', { window: 'renderer-id', project });
    assert(!(await status()).watching, 'Tauri снимает наблюдение скрытого окна');
    await pause(11_000);
    assert(!(await status()).watching, 'Фоновый сигнал скрытого WebView не восстанавливает слежение');
    const activity = await call('activity');
    assert(activity.queue.length === 0 && activity.running === null, 'Неизменённый проект не остаётся в очереди');
    report.success = true;
  } catch (error) { report.error = String(error?.stack ?? error); }
  await invoke('workspace_probe_finish', { result: report });
})();
