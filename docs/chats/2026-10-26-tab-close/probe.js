// workspace_probe <изолированная папка> <этот файл>: продуктовый ui/dist, настоящий WebView2.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const report = { success: false, native: navigator.userAgent, checks: [] };
  const wait = async (read, label) => {
    const end = performance.now() + 15000;
    while (performance.now() < end) {
      const value = await read(); if (value) return value;
      await new Promise(resolve => setTimeout(resolve, 30));
    }
    throw Error(label);
  };
  const assert = (value, label) => { if (!value) throw Error(label); report.checks.push(label); };
  const middle = node => {
    const down = new MouseEvent('mousedown', { button: 1, bubbles: true, cancelable: true });
    node.dispatchEvent(down);
    if (!down.defaultPrevented) throw Error('Не отменена автопрокрутка');
    node.dispatchEvent(new MouseEvent('auxclick', { button: 1, bubbles: true, cancelable: true }));
  };
  try {
    await invoke('workspace_probe_size', { width: 1024 });
    const projects = await invoke('bars_call', { method: 'projects', params: {} });
    const project = projects.projects[0].path;
    const read = path => invoke('document_read', { project, path });
    const write = async (path, text) => {
      const before = await read(path);
      const result = await invoke('document_write', { project, path, text, baseRevision: before.revision, eol: 'lf', bom: false });
      if (result.status !== 'saved') throw Error(JSON.stringify(result));
    };
    for (let i = 0; i < 9; i++) await write(`docs/tab-${i}.md`, `# Проверочный документ ${i}\n`);
    await wait(() => document.querySelector('.tree .caret'), 'Нет дерева');
    document.querySelector('.tree .caret').click();
    const files = await wait(() => {
      const nodes = [...document.querySelectorAll('.tree .filename')].filter(n => n.textContent.includes('Tab '));
      return nodes.length === 9 && nodes;
    }, 'Нет девяти документов');
    const tabs = () => document.querySelectorAll('[data-workspace-panel^="document:"]');
    for (const [index, node] of files.entries()) {
      node.click();
      await wait(() => tabs().length === index + 1, `Не открылась вкладка ${index}`);
    }
    await wait(() => tabs().length === 9, 'Не открылись девять вкладок');
    const dropdown = await wait(() => document.querySelector('.dv-tabs-overflow-dropdown-root'), 'Нет переполнения');
    dropdown.click();
    const popup = await wait(() => document.querySelector('.dv-tabs-overflow-container'), 'Нет списка');
    popup.querySelector('.dv-default-tab-action').click();
    await wait(() => !document.querySelector('.dv-tabs-overflow-container') && tabs().length === 8, 'Крестик оставил список или вкладку');
    assert(true, 'Крестик закрывает скрытую вкладку и список');
    (await wait(() => document.querySelector('.dv-tabs-overflow-dropdown-root'), 'Нет повторного переполнения')).click();
    middle((await wait(() => document.querySelector('.dv-tabs-overflow-container'), 'Нет повторного списка')).querySelector('[data-workspace-panel]'));
    await wait(() => !document.querySelector('.dv-tabs-overflow-container') && tabs().length === 7, 'Колёсико оставило список или вкладку');
    assert(true, 'Средняя кнопка закрывает строку списка без автопрокрутки');
    const selected = document.querySelector('.dv-active-tab [data-document-path]');
    const path = selected.dataset.documentPath;
    const editor = await wait(() => document.querySelector(`section[data-document-path="${path}"] .cm-content[contenteditable="true"]`), 'Нет редактора');
    await write(path, '# Внешняя версия\n');
    editor.focus();
    if (!document.execCommand('insertText', false, 'Моя правка')) throw Error('Нет ввода');
    middle(selected);
    const dialog = await wait(() => document.querySelector('[role="alertdialog"]'), 'Нет вопроса о несохранённом');
    [...dialog.querySelectorAll('button')].find(b => b.textContent === 'Вернуться').click();
    await wait(() => !document.querySelector('[role="alertdialog"]'), 'Диалог не закрыт');
    assert(selected.isConnected && editor.textContent.includes('Моя правка'), 'Отмена сохраняет вкладку и правки');
    middle(selected);
    const again = await wait(() => document.querySelector('[role="alertdialog"]'), 'Нет повторного вопроса');
    [...again.querySelectorAll('button')].find(b => b.textContent === 'Закрыть без записи').click();
    await wait(() => !selected.isConnected, 'Отказ от записи не закрыл вкладку');
    assert((await read(path)).text === '# Внешняя версия\n', 'Закрытие без записи сохраняет внешнюю версию на диске');
    report.success = true;
  } catch (error) { report.error = String(error); report.text = document.body.innerText.slice(0, 1500); }
  await invoke('workspace_probe_finish', { result: report });
})();
