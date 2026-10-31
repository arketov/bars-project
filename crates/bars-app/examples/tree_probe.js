// Программная проверка настоящего окна и IPC; физические жесты проверяются отдельно.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const results = [];
  const wait = (predicate, label) => new Promise((resolve, reject) => {
    const end = performance.now() + 12000;
    const check = () => { const value = predicate(); if (value) resolve(value); else if (performance.now() > end) reject(Error(label)); else setTimeout(check, 30); };
    check();
  });
  const assert = (ok, name) => { if (!ok) throw Error(name); results.push(name); };
  const click = label => { const button = document.querySelector(`button[aria-label="${label}"]`); if (!button) throw Error(label); button.click(); };
  const submit = async name => {
    const form = await wait(() => document.querySelector('dialog[open] form'), 'диалог');
    const input = form.querySelector('input');
    input.value = name; input.dispatchEvent(new Event('input', {bubbles:true}));
    await new Promise(resolve => setTimeout(resolve, 0));
    form.requestSubmit();
    await wait(() => !document.querySelector('dialog[open]'), 'завершение создания');
  };
  try {
    const row = await wait(() => document.querySelector('[data-folder-path="Пустая"]'), 'пустая папка');
    const project = document.querySelector('[data-project-path]').dataset.projectPath;
    for (const theme of ['light', 'dark']) {
      document.documentElement.dataset.theme = theme;
      document.querySelector('.proj').dispatchEvent(new MouseEvent('contextmenu', {bubbles:true, cancelable:true, clientX:60, clientY:90}));
      const rename = await wait(() => [...document.querySelectorAll('[role=menuitem]')].find(item => item.textContent.startsWith('Переименовать…')), 'переименование проекта в меню');
      rename.click();
      const form = await wait(() => document.querySelector('dialog[open] form'), 'диалог имени проекта');
      const input = form.querySelector('input');
      assert(document.activeElement === input && input.selectionEnd === input.value.length, `фокус имени проекта: ${theme}`);
      assert(form.textContent.includes('Папка и путь останутся прежними'), `подсказка переименования: ${theme}`);
      const name = `Мои знания ${theme}`;
      input.value = name; input.dispatchEvent(new Event('input', {bubbles:true}));
      await new Promise(resolve => setTimeout(resolve, 0));
      form.requestSubmit();
      await wait(() => !document.querySelector('dialog[open]'), 'сохранение имени проекта');
      await wait(() => document.querySelector('.proj b')?.textContent === name, 'новое имя проекта');
      const output = await invoke('bars_call', {method:'projects',params:{}});
      assert(output.projects[0].name === name && output.projects[0].path === project, `настоящее имя и неизменный путь: ${theme}`);
    }
    assert(!!row.querySelector('input[type=checkbox]'), 'пустая папка видна до индексации с галочкой');
    row.dispatchEvent(new MouseEvent('contextmenu', {bubbles:true, cancelable:true, clientX:40, clientY:100}));
    await wait(() => document.querySelector('[role=menuitem]'), 'меню пустой папки');
    assert(document.querySelector('[role=menu]').textContent.includes('Открыть в Проводнике'), 'меню пустой папки содержит Проводник');
    document.dispatchEvent(new KeyboardEvent('keydown', {key:'Escape', bubbles:true}));
    const target = await invoke('folder_target', {project, docsRoot:'docs', path:'Пустая'});
    const copied = await invoke('folder_action', {id:target.id, action:'path'});
    assert(copied.absolute.includes('Пустая') && !copied.missing, 'настоящий допуск и путь пустой папки');
    for (const theme of ['light', 'dark']) {
      document.documentElement.dataset.theme = theme;
      click('Новый документ в корне документации');
      await submit(`Документ-${theme}`);
      await wait(() => document.querySelector(`[data-document-path="docs/Документ-${theme}.md"]`), 'созданный файл');
      await wait(() => [...document.querySelectorAll('.cm-content')].some(content => content.getClientRects().length && content.textContent.includes(`Документ-${theme}`)), 'новый документ в редакторе');
      assert(true, `редактор после создания: ${theme}`);
      assert(document.querySelectorAll('.tree input[type=checkbox]').length >= 2, `галочки при открытом документе: ${theme}`);
    }
    click('Новая папка (модуль)');
    await submit('Созданная');
    await wait(() => document.querySelector('[data-folder-path="Созданная"]'), 'созданная папка');
    const index = await invoke('document_read', {project, path:'docs/Созданная/index.md'});
    assert(index.text.includes('title: Созданная') && index.text.includes('# Созданная'), 'index.md папки с frontmatter и заголовком');
    const start = performance.now();
    await invoke('tree_probe_external');
    await wait(() => document.querySelector('[data-folder-path="Внешняя"]'), 'внешняя папка');
    results.push(`внешняя пустая папка: ${Math.round(performance.now()-start)} мс`);
    const folder = document.querySelector('[data-folder-path="Пустая"]');
    folder.querySelector('.nm').click();
    const draft = await wait(() => {
      const content = document.querySelector('section[data-document-path="docs/Пустая/index.md"] .cm-content');
      return content?.getAttribute('contenteditable') === 'true' ? content : null;
    }, 'редактируемый пустой index.md');
    const tree = await invoke('tree_read', {project});
    assert(!tree.files.some(file => file.path === 'docs/Пустая/index.md'), 'просмотр пустой папки не создаёт index.md');
    draft.focus();
    document.execCommand('insertText', false, '# Файл папки');
    await wait(() => document.querySelector('section[data-document-path="docs/Пустая/index.md"] [data-status="saved"]'), 'автосохранение файла папки');
    const written = await invoke('document_read', {project, path:'docs/Пустая/index.md'});
    assert(written.revision !== null && written.text.includes('# Файл папки'), 'первая правка создаёт index.md на диске');
    assert(document.querySelector('.tree button').textContent.includes('Новые темы'), 'Новые темы — первая строка');
    await invoke('tree_probe_finish', {result:{success:true, results}});
  } catch (error) { await invoke('tree_probe_finish', {result:{success:false, results, error:String(error)}}); }
})();
