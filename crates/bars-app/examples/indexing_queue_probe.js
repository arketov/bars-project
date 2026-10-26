// workspace_probe: настоящий UI и обработчик, изолированный профиль, очередь на паузе.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const result = { success: false, checks: [] };
  const wait = async (read, label) => {
    const deadline = performance.now() + 15_000;
    while (performance.now() < deadline) {
      const value = await read(); if (value) return value;
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    throw Error(label);
  };
  const assert = (value, label) => { if (!value) throw Error(label); result.checks.push(label); };
  const call = (method, params = {}) => invoke('bars_call', { method, params });
  try {
    await invoke('workspace_probe_visibility', { visible: true });
    const { projects } = await call('projects');
    const project = projects[0].path, path = 'docs/Тема.md';
    const target = { project, cwd: project };
    await wait(async () => { const a = await call('activity'); return !a.running && !a.queue.length; }, 'Начальная индексация');
    await call('index_pause');
    const before = await invoke('document_read', { project, path });
    const saved = await invoke('document_write', { project, path, text: '# Изменённая тема\n\nНовая версия для проверки очереди.\n', baseRevision: before.revision, eol: 'lf', bom: false });
    assert(saved.status === 'saved', 'Правка записана через настоящий IPC');
    await call('reindex', target);
    const queued = await wait(async () => {
      const events = await call('operation_events');
      return events.active.find(op => op.project === project && op.kind === 'files');
    }, 'Нет отдельного обновления документов');
    assert(queued.state === 'queue_paused', 'Проверка закончилась независимо от паузы обновлений');
    assert(queued.docs_total === 1, 'Обновление показывает число найденных документов');
    for (let i = 0; i < 3; i++) {
      await call('reindex', target);
      await wait(async () => !(await call('operation_events')).active.some(op => op.project === project && op.kind === 'scan'), 'Повторный scan не закончен');
      const updates = (await call('operation_events')).active.filter(op => op.project === project && ['check', 'files', 'rebuild'].includes(op.kind));
      assert(updates.length === 1 && updates[0].id === queued.id, `Повтор ${i + 1}: одна запись обновления с прежним ID`);
    }
    (await wait(() => document.querySelector('button.bell'), 'Нет уведомлений')).click();
    const panel = await wait(() => document.querySelector('#notifications-panel'), 'Нет панели');
    await wait(() => panel.textContent.includes('Обновление документов'), 'Нет названия этапа обновления');
    assert(!panel.textContent.includes('Проверка индекса'), 'Уведомления отличают проверку файлов от обновления');
    assert(panel.textContent.includes('документов: 1'), 'Карточка очереди показывает объём обновления');
    await call('index_resume');
    await wait(async () => { const a = await call('activity'); return !a.running && !a.queue.length; }, 'Обновление не завершилось');
    const history = await call('operation_events', { epoch: (await call('operation_events')).epoch, after: 0 });
    const completed = history.completed.find(op => op.id === queued.id);
    assert(completed?.state === 'success' && completed.report?.changed === 1, 'Одна операция применила изменение одного документа');
    result.success = true;
  } catch (error) { result.error = String(error?.stack ?? error); }
  await invoke('workspace_probe_finish', { result });
})();
