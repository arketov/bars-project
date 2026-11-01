// workspace_probe <каталог результата> <этот файл>: настоящий UI и изолированный воркер.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const result = { success: false, checks: [], native: navigator.userAgent };
  const call = (method, params = {}) => invoke('bars_call', { method, params });
  const wait = async (read, label) => {
    const deadline = performance.now() + 15000;
    while (performance.now() < deadline) {
      const value = await read(); if (value) return value;
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    throw Error(label);
  };
  const assert = (ok, label) => { if (!ok) throw Error(label); result.checks.push(label); };
  const button = label => [...document.querySelectorAll('.settings-screen button')].find(b => b.textContent.trim() === label);
  try {
    const initial = await call('settings_get');
    assert(initial.settings.indexing.edit_pause_seconds === 15, 'Воркер без настроек возвращает 15 секунд');
    (await wait(() => document.querySelector('button[title="Настройки"]'), 'Нет окна')).click();
    await wait(() => document.querySelector('.settings-screen fieldset:not([disabled])'), 'Нет настроек');
    button('Индексация').click();
    const field = await wait(() => document.querySelector('#edit-pause'), 'Нет поля паузы');
    assert(field.value === '15', 'В собранном окне умолчание 15 секунд');
    field.value = '30'; field.dispatchEvent(new Event('input', { bubbles: true }));
    await wait(() => !button('Сохранить').disabled, 'Изменение не принято');
    button('Сохранить').click();
    await wait(async () => (await call('settings_get')).settings.indexing.edit_pause_seconds === 30 && button('Сохранить')?.disabled === true, 'Пауза не сохранена');
    assert(field.value === '30', 'Явные 30 секунд сохраняются');
    button('К поиску').click();
    await wait(() => !document.querySelector('.settings-screen'), 'Настройки не закрылись');
    document.querySelector('button[title="Настройки"]').click();
    await wait(() => document.querySelector('.settings-screen fieldset:not([disabled])'), 'Настройки не открылись повторно');
    button('Индексация').click();
    assert((await wait(() => document.querySelector('#edit-pause'), 'Нет повторного поля')).value === '30', 'Повторное открытие сохраняет явное значение');
    result.success = true;
  } catch (error) { result.error = String(error?.stack ?? error); }
  await invoke('workspace_probe_finish', { result });
})();
