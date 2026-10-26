// Запуск: workspace_probe <каталог результата> <этот файл>. Изолированный Tauri/WebView2, программный ввод.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const report = { success: false, native: navigator.userAgent, checks: [] };
  const wait = async (read, label) => {
    const end = performance.now() + 15000;
    while (performance.now() < end) {
      const value = await read();
      if (value) return value;
      await new Promise(resolve => setTimeout(resolve, 30));
    }
    throw Error(label);
  };
  const assert = (value, label) => { if (!value) throw Error(label); report.checks.push(label); };
  try {
    const projects = await invoke('bars_call', { method: 'projects', params: {} });
    const project = projects.projects[0].path, path = 'docs/Тема.md';
    const read = () => invoke('document_read', { project, path });
    const write = async text => {
      const before = await read();
      const saved = await invoke('document_write', { project, path, text, baseRevision: before.revision, eol: 'lf', bom: false });
      if (saved.status !== 'saved') throw Error(JSON.stringify(saved));
    };
    await wait(() => document.querySelector('.tree .caret'), 'Нет дерева');
    for (const caret of document.querySelectorAll('.tree .caret')) caret.click();
    (await wait(() => document.querySelector('.tree .filename'), 'Нет файла')).click();
    const editor = await wait(() => document.querySelector('.cm-content[contenteditable="true"]'), 'Нет редактора');
    const content = () => editor.textContent;
    const key = key => editor.dispatchEvent(new KeyboardEvent('keydown', { key, code: `Key${key.toUpperCase()}`, ctrlKey: true, bubbles: true, cancelable: true }));
    const type = text => { editor.focus(); if (!document.execCommand('insertText', false, text)) throw Error('insertText'); };
    const original = content();
    type('моя правка');
    await wait(async () => (await read()).text.includes('моя правка'), 'Не записалась правка');
    key('z');
    await wait(() => content() === original, 'Отмена после автосохранения');
    assert(true, 'Своя правка отменяется после автосохранения');
    key('y');
    await wait(() => content().includes('моя правка'), 'Повтор своей правки');
    await wait(async () => (await read()).text.includes('моя правка'), 'Повтор не записан');
    await write('# Внешняя версия\n');
    window.dispatchEvent(new Event('focus'));
    await wait(() => content() === '# Внешняя версия', 'Не перечитан диск');
    key('z');
    await new Promise(resolve => setTimeout(resolve, 1200));
    assert(content() === '# Внешняя версия' && (await read()).text === '# Внешняя версия\n', 'Отмена не возвращает старую версию на экран и диск');
    key('y');
    await new Promise(resolve => setTimeout(resolve, 1200));
    assert(content() === '# Внешняя версия' && (await read()).text === '# Внешняя версия\n', 'Повтор после внешнего обновления не меняет экран и диск');
    type('черновик');
    await write('# Диск при конфликте\n');
    window.dispatchEvent(new Event('focus'));
    const takeDisk = await wait(() => [...document.querySelectorAll('button')].find(button => button.textContent.trim() === 'Взять версию с диска'), 'Нет выбора версии');
    takeDisk.click();
    await wait(() => content() === '# Диск при конфликте', 'Не принята версия диска');
    key('z');
    await new Promise(resolve => setTimeout(resolve, 1200));
    assert(content() === '# Диск при конфликте' && (await read()).text === '# Диск при конфликте\n', 'Принятый диск не откатывается к черновику');
    key('y');
    await new Promise(resolve => setTimeout(resolve, 1200));
    assert(content() === '# Диск при конфликте' && (await read()).text === '# Диск при конфликте\n', 'Повтор после принятия диска не возвращает черновик');
    type(' новая правка'); key('z');
    await wait(() => content() === '# Диск при конфликте', 'Отмена новой правки');
    assert(true, 'Новая правка после принятия диска отменяется');
    report.success = true;
  } catch (error) { report.error = String(error); }
  await invoke('workspace_probe_finish', { result: report });
})();
