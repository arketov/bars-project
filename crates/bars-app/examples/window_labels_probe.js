// Продуктовый ui/dist в изолированном workspace_probe; профиль пользователя не используется.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const checks = [];
  const assert = (value, name) => { if (!value) throw Error(name); checks.push(name); };
  const wait = async (read, name) => {
    const end = performance.now() + 35000;
    while (performance.now() < end) {
      const result = await read();
      if (result) return result;
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    throw Error(name);
  };
  const button = name => [...document.querySelectorAll('button')].find(b => b.textContent.trim() === name);
  try {
    const projects = await invoke('bars_call', { method: 'projects', params: {} });
    const project = projects.projects[0].path;
    await wait(() => document.querySelector('textarea[aria-label="Поисковый запрос"]'), 'Поле поиска');
    await wait(async () => (await invoke('bars_call', { method: 'status', params: { cwd: project, project } })).state === 'ready', 'Готовый индекс');
    const input = document.querySelector('textarea[aria-label="Поисковый запрос"]');
    input.value = 'Проверка панелей'; input.dispatchEvent(new Event('input', { bubbles: true }));
    const mode = document.querySelector('[aria-label="Режим поиска"]');
    mode.value = 'mix'; mode.dispatchEvent(new Event('change', { bubbles: true }));
    await wait(() => document.querySelector('.related-option input'), 'Параметр связанных источников');
    if (document.querySelector('.related-option input').checked) document.querySelector('.related-option input').click();
    button('Найти').click();
    await wait(() => document.querySelector('.graph-search-info'), 'Смена режима поиска');
    await wait(() => document.querySelector('.card'), 'Карточка результата');
    const text = document.querySelector('.results').textContent;
    assert(text.includes('Связанные источники выключены'), 'Известная причина смены режима словами');
    assert(text.includes('Оценка по близости'), 'Вид оценки словами');
    assert(!/include_related|\bmix\b|\bhybrid\b|\bcosine\b|\brerank\b/.test(text), 'Коды отсутствуют в видимом тексте выдачи');
    assert(!!document.querySelector('[title="Связи отключены: include_related=false"]'), 'Исходная диагностика в подсказке');
    document.querySelector('.proj b').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    const menu = await wait(() => document.querySelector('.cmenu'), 'Меню проекта');
    assert(menu.textContent.includes('Проверить проект'), 'Меню проекта использует словарь');
    menu.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await invoke('workspace_probe_finish', { result: { success: true, checks, native: navigator.userAgent } });
  } catch (error) {
    await invoke('workspace_probe_finish', { result: { success: false, checks, error: String(error), text: document.body.textContent.slice(-4000) } });
  }
})();
