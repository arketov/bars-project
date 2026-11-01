// Продуктовый ui/dist, настоящий IPC/поиск, изолированный профиль workspace_probe.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke, checks = [], geometry = [];
  const frames = () => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const wait = async (read, name) => {
    const end = performance.now() + 30000;
    while (performance.now() < end) { const value = await read(); if (value) return value; await new Promise(resolve => setTimeout(resolve, 50)); }
    throw Error(name);
  };
  const assert = (value, name) => { if (!value) throw Error(name); checks.push(name); };
  const button = name => [...document.querySelectorAll('button')].find(b => b.textContent.trim() === name);
  const input = () => document.querySelector('textarea[aria-label="Поисковый запрос"]');
  const summary = () => document.querySelector('.query-parameters summary');
  const set = (node, value) => { node.value = value; node.dispatchEvent(new Event('input', { bubbles: true })); };
  const search = async query => {
    set(input(), query); button('Найти').click(); await frames();
    await wait(() => ![...document.querySelectorAll('.results [role="status"]')].some(node => node.textContent.includes('Ищем')), 'Поиск не завершён');
    await frames();
  };
  try {
    const project = (await invoke('bars_call', { method: 'projects', params: {} })).projects[0].path;
    await wait(() => input(), 'Поле запроса');
    await wait(async () => (await invoke('bars_call', { method: 'status', params: { cwd: project, project } })).state === 'ready', 'Индекс не готов');
    assert(!document.querySelector('.query-parameters').open, 'Параметры свёрнуты по умолчанию');
    assert(!document.querySelector('.text-empty'), 'До первого ответа пустого итога нет');
    summary().click(); await frames();
    document.querySelector('[aria-label="Учитывать регистр"]').click(); await frames();
    summary().click(); await frames();
    assert(summary().textContent.includes('регистр'), 'Скрытый активный параметр назван словами');
    summary().click(); document.querySelector('[aria-label="Учитывать регистр"]').click(); summary().click();
    await search('Настоящий');
    const occurrence = await wait(() => document.querySelector('.occurrence'), 'Нет настоящего текстового совпадения');
    occurrence.click();
    const label = await wait(() => document.querySelector('.cm-found-label'), 'Нет подписи в CodeMirror');
    assert(label.textContent === 'строка 7 · найдено', 'Подпись настоящего текстового вхождения');
    assert(document.querySelector('.cm-hit').textContent === 'Настоящий', 'Точное выделение символов сохранено');
    document.querySelector('[aria-label="Исходник / рендер"]').click(); await frames();
    assert(!!document.querySelector('#hit'), 'Рендер сохраняет найденный отрывок');
    document.querySelector('[aria-label="Исходник / рендер"]').click(); await frames();
    assert(!!document.querySelector('.cm-found-label'), 'Возврат в исходник сохраняет подпись');
    const content = document.querySelector('.cm-content'); content.focus();
    const selection = getSelection(), range = document.createRange(); range.selectNodeContents(content); range.collapse(false); selection.removeAllRanges(); selection.addRange(range);
    document.execCommand('insertText', false, '!'); await frames();
    assert(!document.querySelector('.cm-found-label') && !document.querySelector('.cm-hit'), 'Первая правка снимает подпись и фон');
    document.querySelector('[aria-label="Исходник / рендер"]').click(); await frames();
    assert(!document.querySelector('#hit'), 'Рендер после правки не возвращает подсветку');
    await wait(async () => (await invoke('document_read', { project, path: 'docs/Тема.md' })).text.endsWith('!'), 'Правка не записана');
    const saved = await invoke('document_read', { project, path: 'docs/Тема.md' });
    assert(!saved.text.includes('· найдено'), 'Служебная подпись не записана в Markdown');
    button('Поиск')?.click(); await frames();
    await search('нетсовпадений927463');
    const empty = await wait(() => document.querySelector('.text-empty'), 'Нет пустого итога');
    assert(!document.querySelector('.text-results') && empty.textContent === 'В тексте: вхождений не найдено', 'Полный пустой итог одной строкой');
    assert(!!(document.querySelector('.semantic-title').compareDocumentPosition(empty) & Node.DOCUMENT_POSITION_FOLLOWING), 'Пустой итог после смысловой выдачи');
    summary().click(); document.querySelector('[aria-label="Регулярное выражение"]').click(); await frames();
    await search('[');
    assert(!!document.querySelector('.text-results [role="alert"]') && !document.querySelector('.text-empty'), 'Ошибка выражения остаётся над смысловой выдачей');
    document.querySelector('[aria-label="Регулярное выражение"]').click(); summary().click();
    const pane = document.querySelector('.ask').closest('main');
    document.querySelector('button[aria-label="Глобальный поиск"]').click(); await frames();
    assert(document.querySelector('dialog[open] .ask').closest('main') === pane, 'Глобальный поиск использует тот же компонент');
    summary().focus(); summary().dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }));
    assert(document.activeElement.classList.contains('scope-toggle') || document.activeElement.classList.contains('close-search'), 'Tab из свёрнутых параметров замыкается в диалоге');
    for (const width of [1024, 760]) {
      await invoke('workspace_probe_size', { width }); await frames();
      for (const mode of ['light', 'dark']) {
        document.documentElement.dataset.mode = mode; await frames();
        const ask = document.querySelector('.ask'), bounds = ask.getBoundingClientRect();
        assert(ask.scrollWidth <= ask.clientWidth + 1, `Карточка без переполнения ${width}/${mode}`);
        geometry.push({ width, mode, card: bounds.width, scroll: ask.scrollWidth, client: ask.clientWidth });
      }
    }
    await invoke('workspace_probe_finish', { result: { success: true, checks, geometry, native: navigator.userAgent } });
  } catch (error) {
    await invoke('workspace_probe_finish', { result: { success: false, checks, geometry, error: String(error), text: document.body.textContent.slice(-4000) } });
  }
})();
