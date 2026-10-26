// Собранный ui/dist, настоящий Tauri/WebView2 и изолированный временный проект.
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
  const buttons = () => [...document.querySelectorAll('.primary [role="menuitem"]')];
  const item = text => buttons().find(b => b.textContent.trim().startsWith(text));
  const key = (name, code = name) => window.dispatchEvent(new KeyboardEvent('keydown', {key:name,code,bubbles:true,cancelable:true}));
  const close = async () => { key('Escape'); await wait(() => !document.querySelector('.cmenu'), 'Меню не закрылось'); };
  const open = async row => {
    row.dispatchEvent(new MouseEvent('contextmenu', {bubbles:true,cancelable:true,clientX:innerWidth-1,clientY:innerHeight-1}));
    return wait(() => document.querySelector('.cmenu'), 'Меню не открылось');
  };
  try {
    await wait(() => document.querySelector('.tree [data-tree-node="."]'), 'Нет дерева');
    const root = document.querySelector('.tree [data-tree-node="."]');
    await open(root);
    assert(!item('Удалить') && !item('Переименовать') && !item('Цвет'), 'Корневые документы: неприменимые команды скрыты');
    assert(document.querySelectorAll('.primary [role="separator"]').length === 2, 'Корень: три непустые группы с двумя разделителями');
    item('Новая папка').click();
    const dialog = await wait(() => document.querySelector('dialog[open] input'), 'Нет создания папки');
    dialog.value = 'Модуль'; dialog.dispatchEvent(new Event('input', {bubbles:true}));
    const create = [...document.querySelectorAll('dialog[open] button')].find(b => b.textContent.trim() === 'Создать');
    await wait(() => !create.disabled, 'Создание недоступно'); create.click();
    await wait(() => document.querySelector('.tree [data-tree-node="Модуль"]'), 'Папка не создана через IPC');
    await wait(() => !document.querySelector('dialog[open]'), 'Диалог не закрылся');
    await new Promise(resolve => setTimeout(resolve, 80));
    const module = document.querySelector('.tree [data-tree-node="Модуль"]');
    await open(module);
    assert(buttons().at(-1).textContent.includes('Удалить') && buttons().at(-1).classList.contains('danger'), 'Удаление — последняя опасная группа');
    assert(document.querySelectorAll('.primary [role="separator"]').length === 5, 'Папка: шесть групп без лишних разделителей');
    assert(item('Переименовать').querySelector('.shortcut').textContent === 'F2', 'Сочетание F2 из реестра');
    assert(item('Переместить').querySelector('.shortcut').textContent === '', 'Нет назначения — нет текста сочетания');
    await close(); assert(document.activeElement === module, 'Escape возвращает фокус строке');
    key('F2');
    await wait(() => document.querySelector('dialog[open] input'), 'F2 не открыл переименование');
    assert([...document.querySelectorAll('dialog[open]')].some(d => d.textContent.includes('Переименовать')), 'F2 вызывает тот же диалог через реестр');
    [...document.querySelectorAll('dialog[open] button')].find(b => b.textContent.trim() === 'Отмена').click();
    await wait(() => !document.querySelector('dialog[open]'), 'Диалог не закрылся');
    await new Promise(resolve => setTimeout(resolve, 80));
    await open(root); item('Показать в').focus(); key('ArrowRight');
    assert(!document.querySelector('.sub'), 'Правая стрелка на обычном пункте не открывает подменю'); await close();
    root.parentElement.querySelector('.caret').click();
    const file = await wait(() => document.querySelector('.tree [data-menu-kind="file"]'), 'Нет файла');
    await open(file); const editorParent = item('Открыть в …'); editorParent.focus(); key('ArrowRight');
    await wait(() => document.querySelector('.sub button'), 'Нет подменю программ');
    key('Escape'); await wait(() => !document.querySelector('.sub'), 'Escape не закрыл подменю');
    assert(document.querySelector('.cmenu') && document.activeElement === editorParent, 'Первый Escape возвращает в родительское меню');
    await close();
    file.focus();
    window.dispatchEvent(new KeyboardEvent('keydown',{key:'P',code:'KeyP',ctrlKey:true,shiftKey:true,bubbles:true,cancelable:true}));
    const palette = await wait(() => document.querySelector('dialog[aria-label="Список действий"][open]'), 'Нет списка команд');
    const query = palette.querySelector('input'); query.value = 'target.openExternal'; query.dispatchEvent(new Event('input',{bubbles:true}));
    await wait(() => palette.querySelector('[role="option"]'), 'Нет команды выбора программы');
    query.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true,cancelable:true}));
    await wait(() => document.querySelector('.sub button'), 'Команда не открыла программы');
    await new Promise(resolve => setTimeout(resolve,80));
    assert(document.querySelector('.sub button'), 'Закрытие списка команд не закрывает открытое им подменю');
    key('Escape'); await wait(() => !document.querySelector('.sub'), 'Не закрылось подменю'); await close();
    for (const mode of ['light','dark']) {
      document.documentElement.dataset.mode = mode;
      await invoke('workspace_probe_size',{width:620});
      const menu = await open(file); const rect = menu.getBoundingClientRect();
      assert(rect.left >= 0 && rect.right <= innerWidth && rect.top >= 0 && rect.bottom <= innerHeight, `${mode}: меню внутри узкого окна`);
      assert(getComputedStyle(buttons().at(-1)).color !== getComputedStyle(buttons()[0]).color, `${mode}: опасное действие выделено цветом`);
      await close();
    }
    report.success = true;
  } catch (error) { report.error = String(error?.stack ?? error); }
  await invoke('workspace_probe_finish', {result:report});
})();
