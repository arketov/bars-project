// workspace_probe <папка результата> <этот файл>; изолированный профиль, только программные действия.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const report = JSON.parse(sessionStorage.getItem('zone-report') || '{"success":false,"checks":[]}');
  const phase = Number(sessionStorage.getItem('zone-phase') || 0);
  const wait = async (read, label) => {
    const end = performance.now() + 12000;
    while (performance.now() < end) { const v = await read(); if (v) return v; await new Promise(r => setTimeout(r, 40)); }
    throw Error(label);
  };
  const pause = () => new Promise(r => setTimeout(r, 200));
  const assert = (v, label) => { if (!v) throw Error(label); report.checks.push(label); };
  try {
    const { projects } = await invoke('bars_call', { method: 'projects', params: {} });
    const project = projects[0].path, key = `bars-workspace-v3:${project}`, old = `bars-workspace-v2:${project}`;
    const saved = () => JSON.parse(localStorage.getItem(key));
    await wait(() => document.querySelector('.workspace .proj')?.dataset.projectPath === project && saved(), 'Проект не открыт');
    const groups = () => document.querySelectorAll('.dock .dv-groupview').length;
    const leaves = node => node.type === 'leaf' ? [node] : node.data.flatMap(leaves);
    const open = async () => {
      await wait(() => document.querySelector('.tree .caret, .tree .filename'), 'Нет дерева');
      if (!document.querySelector('.tree .filename')) for (const caret of document.querySelectorAll('.tree .caret')) caret.click();
      (await wait(() => document.querySelector('.tree .filename'), 'Нет файла')).click();
      await wait(() => document.querySelector('.document .cm-content'), 'Нет документа'); await pause();
    };
    const right = () => document.querySelector('[data-workspace-panel^="document:"]').closest('.dv-groupview');
    const close = async () => {
      document.querySelector('[data-workspace-panel^="document:"] .dv-default-tab-action').click();
      await wait(() => !document.querySelector('.document'), 'Документ не закрылся'); await pause();
    };
    const reload = n => { sessionStorage.setItem('zone-phase', n); sessionStorage.setItem('zone-report', JSON.stringify(report)); location.reload(); };
    if (phase === 0) {
      assert(groups() === 1, 'Первый запуск: только центр, без пустой правой зоны');
      await open();
      assert(groups() === 2, 'Документ создаёт правую зону');
      const width = right().getBoundingClientRect().width;
      assert(Math.abs(width - document.querySelector('.dock').clientWidth * .4) < 3, 'Начальная ширина — 40% рабочей области');
      await close(); assert(groups() === 1, 'Последняя вкладка удаляет правую зону');
      await open(); assert(Math.abs(right().getBoundingClientRect().width - width) < 3, 'Повторное открытие сохраняет ширину');
      const value = saved(); value.version = 2; delete value.zones;
      for (const node of leaves(value.layout.grid.root)) if (node.data.views.some(id => id.startsWith('document:'))) {
        node.data.views.forEach(id => delete value.layout.panels[id]); node.data.views = []; delete node.data.activeView;
        node.size = 530;
      } else node.size = value.layout.grid.width - 530;
      value.layout.activeGroup = leaves(value.layout.grid.root)[0].data.id;
      localStorage.setItem(old, JSON.stringify(value)); localStorage.removeItem(key); reload(1); return;
    }
    if (phase === 1) {
      assert(saved().version === 3 && groups() === 1, 'v2 → v3 удаляет пустую правую группу');
      assert(saved().zones.rightWidth === 530 && localStorage.getItem(old), 'Миграция сохраняет ширину и исходную v2');
      await open(); assert(Math.abs(right().getBoundingClientRect().width - 530) < 3, 'Документ использует мигрированную ширину');
      reload(2); return;
    }
    await wait(() => document.querySelector('.document .cm-content'), 'Документ не восстановлен'); await pause();
    assert(groups() === 2 && document.querySelector('.document').textContent.includes('Настоящий документ'), 'v3 восстанавливает вкладку и перечитывает документ');
    for (const width of [760, 1090, 1110, 1440]) {
      await invoke('workspace_probe_size', { width });
      await wait(() => Math.abs(innerWidth - width) < 3, 'Размер окна'); await pause();
      assert(saved().zones.rightWidth === 530, `Ширина ${width}: память широкого окна сохранена`);
      if (width < 1100) assert(getComputedStyle(document.querySelector('.narrow-documents')).display !== 'none', `Ширина ${width}: переключатель документов доступен`);
    }
    assert(Math.abs(right().getBoundingClientRect().width - 530) < 3, 'После расширения вернулась ширина 530');
    await close(); assert(groups() === 1, 'После восстановления последняя вкладка снова удаляет зону');
    report.success = true;
  } catch (error) { report.error = String(error.stack || error); }
  await invoke('workspace_probe_finish', { result: report });
})();
