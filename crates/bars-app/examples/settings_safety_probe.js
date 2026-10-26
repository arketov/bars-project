// workspace_probe <каталог результата> <этот файл>: продуктовый ui/dist и изолированный воркер.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const report = { success: false, checks: [], native: navigator.userAgent };
  const wait = async (read, label) => {
    const end = performance.now() + 15000;
    while (performance.now() < end) { const value = await read(); if (value) return value; await new Promise(resolve => setTimeout(resolve, 30)); }
    throw Error(label);
  };
  const assert = (ok, label) => { if (!ok) throw Error(label); report.checks.push(label); };
  const button = text => [...(document.querySelector('dialog:modal') ?? document.querySelector('.settings-screen') ?? document).querySelectorAll('button')].find(b => b.textContent.trim() === text && b.getClientRects().length);
  const get = () => invoke('bars_call', { method: 'settings_get', params: {} });
  const outside = async changes => { const base = await get(); const output = await invoke('bars_call', { method: 'settings_save', params: { base: base.settings, expected_revision: base.revision, changes } }); if (output.status !== 'saved') throw Error('Внешняя запись не прошла'); };
  const field = title => [...document.querySelectorAll('.settings-screen .field')].find(row => row.querySelector('b')?.textContent === title)?.querySelector('input');
  const type = (title, value) => { const input = field(title); if (!input || input.disabled) throw Error('Недоступное поле ' + title); input.value = String(value); input.dispatchEvent(new Event('input', { bubbles: true })); };
  const open = async () => {
    document.querySelector('button[title="Настройки"]').click();
    await wait(() => document.querySelector('.settings-screen fieldset:not([disabled])'), 'Настройки не прочитались');
    button('Поиск').click();
    await wait(() => field('Результатов без реранка'), 'Нет параметров поиска');
  };
  const saved = async results => wait(async () => (await get()).settings.search.results === results && button('Сохранить')?.disabled, 'Не записаны настройки');
  try {
    await wait(() => document.querySelector('button[title="Настройки"]'), 'Нет окна');
    await open(); button('К поиску').click();
    await wait(() => !document.querySelector('.settings-screen'), 'Чистый экран не закрылся');
    await outside({ 'search.results': 7 });
    await open();
    assert(Number(field('Результатов без реранка').value) === 7, 'Повторное открытие перечитывает файл');
    type('Результатов без реранка', 9);
    await outside({ 'search.candidates': 41 });
    button('Сохранить').click(); await saved(9);
    assert((await get()).settings.search.candidates === 41 && Number(field('Кандидатов от каждого поиска').value) === 41, 'Слияние сохраняет внешнее поле, ответ обновляет форму');
    type('Результатов без реранка', 11); type('Кандидатов от каждого поиска', 43);
    await outside({ 'search.results': 8 });
    button('Сохранить').click();
    await wait(() => button('Взять с диска'), 'Нет конфликта');
    assert((await get()).settings.search.results === 8 && (await get()).settings.search.candidates === 41, 'Конфликт не пишет ни одного поля');
    button('Взять с диска').click();
    await wait(() => !button('Взять с диска'), 'Конфликт не разрешён');
    assert(Number(field('Результатов без реранка').value) === 8 && Number(field('Кандидатов от каждого поиска').value) === 43, 'Выбор диска сохраняет другие мои правки');
    button('Сохранить').click(); await saved(8);
    type('Результатов без реранка', 12);
    await outside({ 'search.results': 10 });
    button('Сохранить').click(); await wait(() => button('Оставить моё'), 'Нет повторного конфликта');
    await outside({ 'search.results': 13 });
    button('Оставить моё').click();
    await wait(() => [...document.querySelectorAll('[role="alert"]')].some(node => node.textContent.includes('на диске 13')), 'Повторная сверка не показала новую версию');
    assert((await get()).settings.search.results === 13, 'Новый конфликт защищён при выборе моей версии');
    button('Оставить моё').click(); await saved(12);
    type('Результатов без реранка', 14); button('К поиску').click();
    await wait(() => document.querySelector('dialog[aria-label="Несохранённые настройки"]:modal'), 'Нет модального вопроса ухода');
    assert(document.activeElement.textContent === 'Остаться', 'Безопасный фокус в модальном вопросе');
    button('Остаться').click(); await wait(() => !button('Остаться'), 'Отмена ухода');
    assert(!!document.querySelector('.settings-screen') && Number(field('Результатов без реранка').value) === 14, 'Отмена ухода сохраняет черновик');
    button('К поиску').click(); await wait(() => button('Не сохранять'), 'Нет вопроса повторного ухода');
    button('Не сохранять').click(); await wait(() => !document.querySelector('.settings-screen'), 'Не закрылись настройки');
    assert((await get()).settings.search.results === 12, 'Уход без сохранения не меняет диск');
    await open(); type('Результатов без реранка', 15); button('К поиску').click();
    const dialog = await wait(() => document.querySelector('dialog[aria-label="Несохранённые настройки"]:modal'), 'Нет вопроса сохранения');
    [...dialog.querySelectorAll('button')].find(b => b.textContent === 'Сохранить').click();
    await wait(() => !document.querySelector('.settings-screen'), 'Сохранение не продолжило уход');
    assert((await get()).settings.search.results === 15, 'Уход с сохранением пишет настройки');
    report.success = true;
  } catch (error) { report.error = String(error); report.text = document.body.innerText.slice(0, 4000); }
  await invoke('workspace_probe_finish', { result: report });
})();
