// Программная приёмка настройки TEI в продуктовом WebView2, отдельный профиль.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const wait = (predicate, label) => new Promise((resolve, reject) => {
    const deadline = performance.now() + 15000;
    const check = () => predicate() ? resolve() : performance.now() > deadline ? reject(Error(label)) : requestAnimationFrame(check);
    check();
  });
  const button = (root, label) => [...root.querySelectorAll('button')].find(b => b.textContent.trim() === label);
  const assert = (ok, label) => { if (!ok) throw Error(label); };
  const report = {success:false, checks:[], user_agent:navigator.userAgent};
  try {
    await wait(() => button(document, 'Панели ▾'), 'Не загружен UI');
    button(document, 'Панели ▾').click();
    await wait(() => button(document, 'Настройки'), 'Нет настроек в меню');
    button(document, 'Настройки').click();
    await wait(() => document.querySelector('.settings-screen h1')?.textContent === 'Модели', 'Не открылись модели');
    const pane = document.querySelector('.settings-screen');
    const teiButtons = () => [...pane.querySelectorAll('button')].filter(b => b.textContent.trim() === 'TEI');
    teiButtons()[0].click();
    await wait(() => pane.textContent.includes('Корень сервера TEI'), 'Не выбран embed TEI');
    // Включение rerank доступно до выбора его источника.
    const rerankToggle = pane.querySelector('[role="switch"][aria-label="Rerank"]');
    if (rerankToggle?.getAttribute('aria-checked') === 'false') rerankToggle.click();
    await wait(() => teiButtons().length === 2, 'Нет rerank TEI');
    teiButtons()[1].click();
    await wait(() => [...pane.querySelectorAll('input')].some(i => i.value === 'http://127.0.0.1:8081'), 'Нет отдельного порта rerank');
    const save = button(pane, 'Сохранить');
    assert(save && !save.disabled, 'Нельзя сохранить TEI');
    save.click();
    const call = (method, params = {}) => invoke('bars_call', {method, params});
    let saved;
    await wait(() => save.disabled && save.textContent.trim() === 'Сохранить', 'Не завершено сохранение');
    saved = await call('settings_get');
    assert(saved.settings.embed.source === 'tei' && saved.settings.rerank.source === 'tei' && saved.settings.rerank.enabled, 'IPC сохранил неверные источники');
    assert(saved.settings.embed.base_url === 'http://127.0.0.1:8080' && saved.settings.rerank.base_url === 'http://127.0.0.1:8081', 'IPC сохранил неверные адреса');
    report.settings = saved.settings;
    report.checks.push('TEI: independent embed/rerank selection, ports, UI save, real IPC readback');
    for (const width of [1440,1024,760]) {
      await invoke('workspace_probe_size', {width});
      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      assert(teiButtons().every(b => b.getBoundingClientRect().width > 0), `Источники скрыты: ${width}`);
    }
    report.checks.push('TEI controls present at 1440/1024/760; layout readback only');
    report.success = true;
  } catch (error) { report.error = String(error?.stack ?? error); }
  await invoke('workspace_probe_finish', {result:report});
})();
