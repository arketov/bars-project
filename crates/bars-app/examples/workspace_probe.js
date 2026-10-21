// Интеграционные проверки собранного приложения; программные действия, не физическая приёмка.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const frames = () => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const wait = (predicate, label) => new Promise((resolve, reject) => {
    const deadline = performance.now() + 20000;
    const check = () => { const value = predicate(); if (value) resolve(value); else if (performance.now() > deadline) reject(Error(label)); else requestAnimationFrame(check); };
    check();
  });
  const assert = (condition, label) => { if (!condition) throw Error(label); };
  const button = text => [...document.querySelectorAll('button')].find(b => b.textContent.trim() === text);
  const report = { success: false, native: navigator.userAgent, checks: [] };
  try {
    assert(await invoke('keyboard_probe_accelerators') === false, 'В WebView2 остались браузерные сочетания');
    report.checks.push('release host: browser accelerator setting read back as disabled');
    await wait(() => document.querySelector('.tree .caret'), 'Нет дерева из воркера');
    for (const caret of document.querySelectorAll('.tree .caret')) caret.click();
    const file = await wait(() => document.querySelector('.tree .filename'), 'Нет файла в индексе');
    file.click();
    await wait(() => document.querySelector('.document .md')?.textContent.includes('Настоящий документ'), 'Не прочитан настоящий документ');
    assert(document.querySelectorAll('[data-panel-kind="document"]').length === 1, 'Число документов');
    assert(document.querySelector('[role="tab"] [data-document-path]'), 'Нет меню документа на вкладке');
    report.checks.push('worker read, document tab');
    document.querySelector('[aria-label="Исходник / рендер"]').click(); await frames();
    assert(document.querySelector('.document pre')?.textContent.includes('Настоящий документ'), 'Нет исходника');
    document.querySelector('[aria-label="Скрыть структуру"]').click(); await frames();
    assert(document.querySelector('[title="Показать структуру"]'), 'Нет повторного открытия структуры');
    document.querySelector('[title="Показать структуру"]').click(); await frames();
    report.checks.push('source toggle, outline hide/show');
    for (const [title, kind] of [['Логи','logs'], ['Анализ MCP','mcp'], ['Настройки','settings'], ['Индексация','indexing'], ['Новые темы','topics']]) {
      button('Панели ▾').click(); await frames(); button(title).click(); await frames();
      assert(document.querySelector('.workspace').dataset.activePanel === kind && document.querySelector(`[data-panel-kind="${kind}"]`), `Не открылась панель ${title}`);
      if (kind === 'settings') await wait(() => document.querySelector('[data-panel-kind="settings"] h1')?.textContent === 'Модели', 'Не прочитаны настройки');
      if (kind === 'mcp') await wait(() => document.querySelector('[data-panel-kind="mcp"] .call-list[aria-busy="false"]'), 'Не прочитана история MCP');
    }
    report.checks.push('service panels');
    button('Поиск').click(); await frames();
    const searchTab = document.querySelector('[data-workspace-panel="search"]').closest('[role="tab"]');
    const tabs = [...searchTab.closest('[role="tablist"]').querySelectorAll('[role="tab"]')];
    searchTab.focus(); searchTab.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true })); await frames();
    const lastId = tabs.at(-1).querySelector('[data-workspace-panel]').dataset.workspacePanel;
    assert(document.querySelector('.workspace').dataset.activePanel === lastId && document.activeElement === tabs.at(-1), 'End не активировал последнюю вкладку');
    tabs.at(-1).dispatchEvent(new KeyboardEvent('keydown', { key: 'Home', bubbles: true })); await frames();
    assert(document.querySelector('.workspace').dataset.activePanel === 'search', 'Home не активировал поиск');
    assert(!document.querySelector('[data-workspace-panel="navigation"]') && document.querySelector('.column aside .tree'), 'Навигация не постоянная колонка');
    document.querySelector('.column aside').dispatchEvent(new MouseEvent('pointerdown', { button: 0, bubbles: true })); await frames();
    assert(document.querySelector('.tree input[type="checkbox"]'), 'Фокус дерева отключил область поиска');
    button('Поиск').click(); await frames();
    report.checks.push('keyboard activation, navigation column preserves search scope');
    // ISS-21: перенос вкладки событиями указателя (dndStrategy pointer), не через API.
    const groupsOf = () => [...document.querySelectorAll('.dock .dv-groupview')].map(g => [...g.querySelectorAll('[data-workspace-panel]')].map(n => n.dataset.workspacePanel));
    const dragTab = async (id, groupIndex, fy) => {
      const tab = document.querySelector(`[data-workspace-panel="${id}"]`).closest('.dv-tab'), from = tab.getBoundingClientRect();
      const to = document.querySelectorAll('.dock .dv-groupview')[groupIndex].getBoundingClientRect();
      const sx = from.left + from.width / 2, sy = from.top + from.height / 2, tx = to.left + to.width / 2, ty = to.top + to.height * fy;
      const send = (type, x, y, target) => (target ?? document.elementFromPoint(x, y) ?? document.body).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, pointerId: 7, pointerType: 'mouse', button: 0, buttons: type === 'pointerup' ? 0 : 1, isPrimary: true }));
      send('pointerdown', sx, sy, tab);
      for (let i = 1; i <= 20; i++) { send('pointermove', sx + (tx - sx) * i / 20, sy + (ty - sy) * i / 20); await frames(); }
      send('pointerup', tx, ty); await frames(); await frames();
    };
    const docId = document.querySelector('[data-panel-kind="document"]') && [...document.querySelectorAll('[data-workspace-panel^="document:"]')][0].dataset.workspacePanel;
    assert(document.querySelectorAll('.dock .dv-groupview').length === 2 && groupsOf()[1].includes(docId), 'Документ открыт не в правой зоне');
    await dragTab(docId, 0, 0.5);
    assert(groupsOf()[0].includes(docId) && groupsOf().length === 2 && groupsOf()[1].length === 0, 'Вкладка не перенесена указателем или правая зона пропала');
    await dragTab(docId, 1, 0.5);
    assert(groupsOf()[1].includes(docId), 'Вкладка не вернулась в правую зону');
    await dragTab('search', 1, 0.93);
    assert(groupsOf().length === 3, 'Перенос к краю не разделил зону');
    await dragTab('search', 0, 0.5);
    assert(groupsOf().length === 2 && groupsOf()[0].includes('search'), 'Пустая часть разделённой зоны не убрана');
    report.checks.push('pointer drag: tab to other zone and back, empty right zone kept, edge split and merge (ISS-21, synthetic PointerEvent)');
    report.geometry = [];
    for (const width of [1440, 1024, 760]) {
      await invoke('workspace_probe_size', { width }); await frames();
      button('Панели ▾').click(); await frames(); button('Настройки').click(); await frames();
      const settingsPane = document.querySelector('[data-panel-kind="settings"]');
      for (const control of settingsPane.querySelectorAll('.field input, .field select')) {
        const rect = control.getBoundingClientRect(), bounds = settingsPane.getBoundingClientRect();
        if (rect.width) assert(rect.left >= bounds.left - 1 && rect.right <= bounds.right + 1, 'Поле настроек за границей панели');
      }
      button('К поиску').click(); await frames();
      assert(document.querySelector('.workspace').dataset.activePanel === 'search', 'Возврат из настроек не открыл поиск');
      if (width < 1100) {
        button('Документы').click(); await frames();
        const rect = document.querySelector('.document').getBoundingClientRect();
        const left = document.querySelector('.dock').getBoundingClientRect().left;
        assert(Math.abs(rect.left - left) < 3 && rect.right >= innerWidth - 2, 'Документ не занимает узкую рабочую область');
        button('Поиск').click(); await frames();
      }
      for (let mode = 0; mode < 2; mode++) {
        await wait(() => getComputedStyle(document.body).backgroundColor === (document.documentElement.dataset.mode === 'light' ? 'rgb(245, 244, 246)' : 'rgb(19, 17, 21)'), 'Тема не применилась после перехода');
        const style = getComputedStyle(document.body);
        assert(document.documentElement.scrollWidth <= innerWidth + 1, 'Переполнение окна');
        const navigation = document.querySelector('.column aside').getBoundingClientRect();
        assert(navigation.width > 100 && navigation.height > 100, 'Потеря навигации');
        const graph = document.querySelector('.mini-graph .graph-canvas')?.getBoundingClientRect();
        assert(graph && graph.width > 50 && graph.height > 20, 'Мини-граф не помещается в колонку');
        const footer = document.querySelector('.column .statusbar').getBoundingClientRect();
        const column = document.querySelector('.column').getBoundingClientRect();
        assert(Math.abs(footer.bottom - column.bottom) < 2 && footer.top >= graph.bottom - 1, 'Статус расположен не под мини-графом');
        assert(Math.abs(footer.left - column.left) < 2 && Math.abs(footer.width - column.width) < 2, 'Статус вышел за ширину колонки');
        const filter = document.querySelector('[placeholder="Фильтр документов"]').getBoundingClientRect();
        assert(filter.height <= 25, 'Фильтр документов слишком высокий');
        const query = document.querySelector('.ask textarea');
        assert(query.wrap === 'off' && query.scrollHeight <= query.clientHeight + 1 && getComputedStyle(query).overflowY === 'hidden', 'Поле запроса переносится или показывает скролл');
        const brand = document.querySelector('.workspace-brand').getBoundingClientRect();
        assert(brand.left <= 16 && brand.top <= 8, 'Бренд не в левом верхнем углу');
        const tools = [...document.querySelectorAll('.mini-graph .graph-map-tools button')];
        assert(tools.length === 5 && tools.every(tool => tool.querySelector('svg') && tool.getAttribute('aria-label') && tool.getAttribute('title') && tool.getBoundingClientRect().height === 20) && document.querySelector('.mini-graph .graph-map-tools').getBoundingClientRect().height <= 26, 'Неверная панель иконок графа');
        assert(document.querySelector('[aria-label="Сбросить область поиска"]'), 'Нет компактного сброса области');
        const launcher = document.querySelector('[aria-label="Глобальный поиск"]');
        const layoutBefore = Object.keys(localStorage).filter(k => k.startsWith('bars-workspace-v2:')).map(k => localStorage.getItem(k)).join();
        const treeNode = document.querySelector('aside');
        launcher.focus(); launcher.click(); await frames();
        const modal = document.querySelector('dialog[aria-label="Глобальный поиск"]');
        assert(modal.open && modal.querySelector('textarea') === query && modal.querySelector('aside') === treeNode, 'Не перенесены общие узлы поиска');
        assert(document.activeElement === query && document.querySelectorAll('.ask textarea').length === 1, 'Нет единственного поля с фокусом');
        const modalBounds = modal.getBoundingClientRect();
        assert(modalBounds.left >= 0 && modalBounds.right <= innerWidth && modalBounds.bottom <= innerHeight, 'Модальный слой вышел за окно');
        assert(query.getBoundingClientRect().width <= 640, 'Поле глобального поиска шире макета');
        const treeBounds = treeNode.getBoundingClientRect();
        if (width <= 850) {
          assert(treeBounds.height === 0, 'Дерево не скрыто на узком окне');
          modal.querySelector('.scope-toggle').click(); await frames();
          assert(treeNode.getBoundingClientRect().height > 100, 'Область не раскрылась');
        }
        const focusable = [...modal.querySelectorAll('button,input,select,textarea,[tabindex="0"],a[href]')].filter(n => !n.matches(':disabled') && n.getClientRects().length);
        focusable.at(-1).focus(); focusable.at(-1).dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }));
        assert(document.activeElement === focusable[0], 'Tab вышел за слой');
        query.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: query.getBoundingClientRect().left + 15, clientY: query.getBoundingClientRect().top + 15 })); await frames();
        assert(modal.querySelector('.cmenu'), 'Контекстное меню осталось под модальным слоем');
        modal.querySelector('.close-search').click(); await frames();
        assert(!modal.open && document.activeElement === launcher, 'Не закрыт слой или потерян фокус');
        assert(document.querySelector('[data-panel-kind="search"] textarea') === query && document.querySelector('.column aside') === treeNode, 'Не возвращены общие узлы');
        assert(layoutBefore === Object.keys(localStorage).filter(k => k.startsWith('bars-workspace-v2:')).map(k => localStorage.getItem(k)).join(), 'Открытие поиска изменило раскладку');
        report.geometry.push({ width: innerWidth, mode: document.documentElement.dataset.mode, background: style.backgroundColor, navigation: navigation.width, graph: [graph.width, graph.height] });
        document.querySelector('[title="Светлая / тёмная тема"]').click(); await frames();
      }
    }
    report.checks.push('both themes at 1440/1024/760, navigation column with mini-graph, compact filter and status below mini-graph');
    report.checks.push('global search in both themes at 1440/1024/760: shared DOM, focus cycle/return, responsive scope, context menu, unchanged layout');
    button('Панели ▾').click(); await frames(); button('Настройки').click(); await frames();
    document.querySelector('[aria-label="Глобальный поиск"]').click(); await frames();
    const modal = document.querySelector('dialog[aria-label="Глобальный поиск"]');
    const query = modal.querySelector('textarea');
    query.value = 'Настоящий документ'; query.dispatchEvent(new Event('input', { bubbles: true })); await frames();
    modal.querySelector('.go').click();
    await wait(() => modal.querySelector('.card'), 'Нет результата настоящего поиска через IPC');
    const card = modal.querySelector('.card'), checkbox = modal.querySelector('input[type="checkbox"]');
    if (checkbox) checkbox.click(); await frames();
    card.querySelector('.chead').click(); await frames();
    assert(!card.classList.contains('open'), 'Карточка не свернулась');
    modal.querySelector('.close-search').click(); await frames();
    document.querySelector('[aria-label="Глобальный поиск"]').click(); await frames();
    assert(modal.querySelector('.card') === card && !card.classList.contains('open') && query.value === 'Настоящий документ', 'Состояние поиска потеряно');
    if (checkbox) assert(checkbox.checked, 'Область потеряна');
    card.querySelector('.chead').click(); await frames(); card.querySelector('.cfoot .btn').click();
    await wait(() => !modal.open && document.activeElement?.matches('.document header'), 'Переход из глобальной выдачи не передал фокус документу');
    assert(document.querySelector('.document .hit'), 'Найденные строки не подсвечены');
    report.checks.push('IPC search with isolated FakeProvider, shared card expansion/scope/query, document read/highlight and focus from modal');
    const saved = Object.keys(localStorage).find(k => k.startsWith('bars-workspace-v2:') && localStorage.getItem(k).includes('docs/Тема.md'));
    assert(saved, 'Нет сохранённой раскладки документа');
    const json = JSON.parse(localStorage.getItem(saved));
    assert(json.version === 2 && !localStorage.getItem(saved).includes('Настоящий документ'), 'Неверное содержимое раскладки');
    report.checks.push('persisted reference without document text');
    // Клавиатура 1.3.2: синтетические DOM-события и настоящий IPC изолированного воркера.
    const key = (code, extra = {}) => document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { code, key: code === 'KeyF' ? 'А' : code, ctrlKey: true, shiftKey: true, bubbles: true, cancelable: true, ...extra }));
    key('KeyP'); await frames();
    const palette = document.querySelector('dialog[aria-label="Список действий"]');
    assert(palette.open && palette.querySelector('[aria-disabled="true"]'), 'Нет списка команд / будущих команд');
    palette.querySelector('input').value = 'Сочетания клавиш'; palette.querySelector('input').dispatchEvent(new Event('input', { bubbles:true })); await frames();
    palette.querySelector('input').dispatchEvent(new KeyboardEvent('keydown', { key:'Enter', bubbles:true, cancelable:true }));
    await wait(() => document.querySelector('section[aria-label="Сочетания клавиш"]'), 'Нет раздела клавиатуры'); await frames();
    const page = document.querySelector('section[aria-label="Сочетания клавиш"]');
    assert(!page.textContent.includes('Профиль сохраняется только'), 'Вместо IPC профиля заглушка');
    const row = [...page.querySelectorAll('.binding-row')].find(r => r.querySelector('b').textContent === 'Глобальный поиск');
    row.querySelector('button').click(); await frames();
    const recorder = document.querySelector('dialog[aria-label="Запись сочетания"]');
    recorder.dispatchEvent(new KeyboardEvent('keydown', { code:'KeyG', key:'П', ctrlKey:true, shiftKey:true, bubbles:true, cancelable:true })); await frames();
    assert(!modal.open, 'Запись сочетания открыла поиск');
    [...recorder.querySelectorAll('button')].find(b => b.textContent === 'Принять').click(); await frames();
    [...page.querySelectorAll('button')].find(b => b.textContent === 'Сохранить сочетания').click();
    await wait(() => page.textContent.includes('Ревизия 1'), 'Карта не сохранена через воркер');
    const stored = await invoke('keybindings_get');
    assert(stored.profile.platforms.windows['search.open'].code === 'KeyG', 'Неверная карта на диске');
    assert(document.querySelector('[aria-label="Глобальный поиск"]').title.includes('Ctrl+Shift+G'), 'Подсказка не обновилась');
    key('KeyF'); await frames(); assert(!modal.open, 'Старое сочетание осталось активно');
    key('KeyG'); await frames(); assert(modal.open, 'Новое сочетание не вызвало поиск');
    modal.querySelector('.close-search').click(); await frames();
    let rejected = false;
    try { await invoke('keybindings_save', { platform:'windows', bindings:{}, expectedRevision:0, recovery:null }); } catch { rejected = true; }
    assert(rejected, 'Устаревшая ревизия затёрла профиль');
    assert((await invoke('keybindings_get')).profile.revision === 1, 'Конфликт изменил ревизию');
    await invoke('keybindings_save', { platform:'windows', bindings:{}, expectedRevision:1, recovery:null }); await frames();
    for (const width of [1440,1024,760]) {
      await invoke('workspace_probe_size', { width }); await frames();
      for (let mode=0;mode<2;mode++) {
        assert(document.documentElement.scrollWidth <= innerWidth + 1, 'Переполнение окна настройками клавиатуры');
        const rect = page.getBoundingClientRect();
        for (const control of page.querySelectorAll('input,select,button')) { const r=control.getBoundingClientRect(); if(r.width) assert(r.left>=rect.left-1 && r.right<=rect.right+1, 'Клавиатурная настройка за границей панели: ' + JSON.stringify({width,control:control.outerHTML.slice(0,160),left:r.left,right:r.right,pageLeft:rect.left,pageRight:rect.right})); }
        key('KeyP'); await frames(); assert(palette.open, 'Список действий не открылся');
        palette.querySelector('[aria-label="Закрыть список действий"]').click(); await frames();
        document.querySelector('[title="Светлая / тёмная тема"]').click(); await frames();
      }
    }
    report.checks.push('keyboard: command palette, future commands, settings, RU physical code, recording isolation, IPC save/read, live hint, rebind, stale revision rejection; both themes 1440/1024/760');
    report.success = true;
  } catch (error) { report.error = String(error); report.text = document.body.innerText.slice(0, 1500); }
  await invoke('workspace_probe_finish', { result: report });
})();
