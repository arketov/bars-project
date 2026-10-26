// workspace_probe <каталог результата> <этот файл>: изолированные WebView2, воркер и диск.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const report = { success: false, checks: [] };
  const wait = async (read, label) => {
    const end = performance.now() + 15000;
    while (performance.now() < end) {
      const value = await read(); if (value) return value;
      await new Promise(resolve => setTimeout(resolve, 30));
    }
    throw Error(label);
  };
  const assert = (value, label) => { if (!value) throw Error(label); report.checks.push(label); };
  const button = text => [...document.querySelectorAll('button')].find(b => b.textContent.trim() === text);
  try {
    const { projects } = await invoke('bars_call', { method: 'projects', params: {} });
    const project = projects[0].path, path = 'docs/Тема.md';
    const read = () => invoke('document_read', { project, path });
    const chooseOther = async () => {
      document.querySelector('.proj').click();
      const other = await wait(() => [...document.querySelectorAll('.pitem')].find(p => p.dataset.projectPath !== project), 'Нет второго проекта');
      other.click();
    };
    await wait(() => document.querySelector('.tree .caret'), 'Нет дерева');
    for (const caret of document.querySelectorAll('.tree .caret')) caret.click();
    (await wait(() => document.querySelector('.tree .filename'), 'Нет файла')).click();
    const editor = await wait(() => document.querySelector('.cm-content[contenteditable="true"]'), 'Нет редактора');
    await invoke('bars_call', {method:'register',params:{path:project + '/docs'}});
    editor.focus(); document.execCommand('insertText', false, 'мой черновик ');
    // Внешняя запись опережает автосохранение; сверка на фокусе создаёт конфликт.
    const before = await read();
    await invoke('document_write',{project,path,text:'# Версия на диске\n',baseRevision:before.revision,eol:'lf',bom:false});
    window.dispatchEvent(new Event('focus'));
    await wait(() => button('Взять версию с диска'), 'Не возник конфликт');
    document.querySelector('[aria-label="Глобальный поиск"]').click();
    const field = await wait(() => document.querySelector('textarea[aria-label="Поисковый запрос"]'), 'Нет поиска');
    field.value = 'документ'; field.dispatchEvent(new Event('input',{bubbles:true}));
    (await wait(() => { const go = button('Найти'); return go && !go.disabled ? go : null; }, 'Кнопка поиска не включилась')).click();
    const warning = await wait(() => [...document.querySelectorAll('[role="alert"]')].find(p => p.textContent.includes('Результаты по записанной версии')), 'Нет предупреждения');
    await wait(() => document.querySelector('.semantic-title'), 'Поиск не запущен');
    assert(warning.compareDocumentPosition(document.querySelector('.semantic-title')) & Node.DOCUMENT_POSITION_FOLLOWING, 'Предупреждение над обоими разделами поиска');
    warning.querySelector('button').click();
    await wait(() => !document.querySelector('dialog[aria-label="Глобальный поиск"][open]'), 'Переход к черновику не закрыл поиск');
    assert(document.querySelector('.cm-content').textContent.includes('мой черновик'), 'Ссылка возвращает к несохранённому тексту');
    document.querySelector('[aria-label="Глобальный поиск"]').click();
    await wait(() => document.querySelector('dialog[aria-label="Глобальный поиск"][open]'), 'Нет модального поиска перед сменой проекта');
    await chooseOther();
    await wait(() => document.querySelector('[role="alertdialog"]'), 'Нет диалога смены проекта');
    await wait(() => document.querySelector('dialog[role="alertdialog"]:modal'), 'Диалог правок не вышел в модальный слой');
    assert(document.querySelector('dialog[role="alertdialog"]').contains(document.activeElement), 'Диалог правок доступен поверх модального поиска');
    assert(document.querySelector('.proj').dataset.projectPath === project, 'Конфликт удерживает прежний проект');
    button('Вернуться').click();
    await wait(() => !document.querySelector('[role="alertdialog"]'), 'Отмена не закрыла диалог');
    assert(document.querySelector('.cm-content').textContent.includes('мой черновик'), 'Отмена сохраняет вкладку и черновик');
    const searchClosed = new Promise(resolve => document.querySelector('dialog[aria-label="Глобальный поиск"]').addEventListener('close', resolve, {once:true}));
    document.querySelector('[aria-label="Закрыть поиск"]').click();
    await searchClosed;
    document.querySelector('.proj').dispatchEvent(new MouseEvent('contextmenu', {bubbles:true,cancelable:true,clientX:100,clientY:100}));
    (await wait(() => [...document.querySelectorAll('[role="menuitem"]')].find(item => item.textContent.trim() === 'Удалить проект'), 'Нет удаления в меню проекта')).click();
    await wait(() => document.querySelector('dialog[aria-labelledby="delete-title"][open]'), 'Нет подтверждения удаления');
    button('Удалить проект').click();
    await wait(() => document.querySelector('dialog[role="alertdialog"]:modal'), 'Диалог правок скрыт подтверждением удаления');
    assert(document.querySelector('dialog[role="alertdialog"]').contains(document.activeElement), 'Диалог правок доступен поверх подтверждения удаления');
    button('Вернуться').click();
    await wait(() => !document.querySelector('[role="alertdialog"]') && !button('Отмена').disabled, 'Отмена не вернула подтверждение удаления');
    assert((await read()).text === '# Версия на диске\n', 'Отмена удаления сохраняет проект и файл на диске');
    button('Отмена').click();
    await wait(() => !document.querySelector('dialog[aria-labelledby="delete-title"]'), 'Подтверждение удаления не закрылось');
    await chooseOther();
    await wait(() => button('Сохранить рядом и закрыть'), 'Нет третьего выбора');
    button('Сохранить рядом и закрыть').click();
    await wait(() => document.querySelector('.proj')?.dataset.projectPath !== project, 'Смена не продолжилась после копии');
    assert((await read()).text === '# Версия на диске\n', 'Чужая версия на диске сохранена');
    const tree = await invoke('tree_read',{project});
    const copy = tree.files.find(file => file.path.includes('(конфликт'));
    assert(copy && (await invoke('document_read',{project,path:copy.path})).text.includes('мой черновик'), 'Копия черновика создана рядом на диске и содержит правки');
    report.success = true;
  } catch (error) { report.error = String(error); report.text = document.body.innerText.slice(0, 2500); }
  await invoke('workspace_probe_finish', {result:report});
})();
