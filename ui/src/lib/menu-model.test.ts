import { describe, expect, it } from 'vitest';
import { documentMenu, menuTargetAt, nestedEditors, treeMenu, type DocumentMenuTarget } from './menu-model';
import { catalog } from './keybindings';

describe('Модель меню дерева ISS-57', () => {
  it.each(['file', 'module', 'dir'] as const)('%s: порядок групп, уникальные команды, удаление последним', kind => {
    const items = treeMenu({ kind, path: 'тема', state: 'ready', canStyle: true, copySelection: true });
    const groups = [...new Set(items.map(item => item.group))];
    expect(groups).toEqual(kind === 'file'
      ? ['edit', 'open', 'modify', 'path', 'style', 'index', 'delete']
      : ['edit', 'create', 'modify', 'path', 'style', 'index', 'delete']);
    expect(new Set(items.map(item => item.id)).size).toBe(items.length);
    expect(items.at(-1)).toMatchObject({ id: 'target.delete', danger: true });
    for (const item of items.filter(item => item.group !== 'edit')) expect(catalog.some(c => c.id === item.id)).toBe(true);
  });
  it('виртуальный корень: только создать, путь и индекс; глубокая папка без оформления', () => {
    const root = treeMenu({kind:'module',path:'.',state:'ready',canStyle:true});
    expect([...new Set(root.map(i => i.group))]).toEqual(['create','path','index']);
    expect(root.some(i => i.danger)).toBe(false);
    expect(treeMenu({kind:'dir',path:'а/б/в',state:'ready'}).some(i => i.group === 'style')).toBe(false);
  });
  it.each(['unavailable','reindex_required'])('%s: проверка видна, с причиной; правка доступна', state => {
    const items = treeMenu({kind:'file',path:'а.md',state});
    expect(items.find(i => i.action === 'check')).toMatchObject({disabled:true,hint:expect.any(String)});
    expect(items.find(i => i.action === 'rename')?.disabled).toBe(false);
  });
  it('запуск воркера блокирует правку, но не прячет пункты', () => {
    const items = treeMenu({kind:'dir',path:'а',state:'starting'});
    for (const id of ['file.create','target.rename','target.move','target.delete','target.checkIndex']) {
      expect(items.find(i => i.id === id)).toMatchObject({disabled:true,hint:expect.stringContaining('запускается')});
    }
  });
  it('сочетание берётся при построении, отсутствие назначения — пустая строка', () => {
    const target = {kind:'file' as const,path:'а.md',state:'ready'};
    expect(treeMenu(target).find(i => i.action === 'rename')?.shortcut).toBe('');
    expect(treeMenu(target, id => id === 'target.rename' ? 'Ctrl+F2' : '').find(i => i.action === 'rename')?.shortcut).toBe('Ctrl+F2');
  });
  it('вложенный объект останавливает наследование меню файла', () => {
    const file = document.createElement('div'); file.dataset.documentPath='docs/а.md';
    file.innerHTML='<a data-menu-kind="link"><span>ссылка</span></a>';
    expect(menuTargetAt(file.querySelector('span'))).toBe(file.querySelector('a'));
  });
});

describe('Модель меню документа 3.3.5', () => {
  const base: DocumentMenuTarget = { path: 'docs/а.md', place: 'tab', state: 'ready', inTree: true, tab: { others: 2, right: 1, canMove: true } };
  const groups = (target: DocumentMenuTarget) => [...new Set(documentMenu(target).map(item => item.group))];
  it.each(['tab', 'header', 'render'] as const)('%s: вкладка, затем набор документа без «Открыть», удаление последним', place => {
    const items = documentMenu({ ...base, place });
    expect(groups({ ...base, place })).toEqual(['tab', 'open', 'modify', 'path', 'index', 'delete']);
    expect(items.some(item => item.action === 'open')).toBe(false);
    expect(items.find(item => item.action === 'closeTab')).toBeTruthy();
    expect(items.at(-1)).toMatchObject({ id: 'target.delete', danger: true });
    expect(items.some(item => item.group === 'style')).toBe(false);
    expect(new Set(items.map(item => item.id)).size).toBe(items.length);
    for (const item of items.filter(item => item.group !== 'edit')) expect(catalog.some(c => c.id === item.id) || item.id.startsWith('document.')).toBe(true);
  });
  it('карточка: «Открыть» есть, вкладки нет, выделение добавляет «Копировать» сверху', () => {
    const card: DocumentMenuTarget = { ...base, place: 'card', copySelection: true };
    expect(groups(card)).toEqual(['edit', 'open', 'modify', 'path', 'index', 'delete']);
    expect(documentMenu(card)[0]).toMatchObject({ action: 'copy', shortcut: 'Ctrl+C' });
    expect(documentMenu({ ...card, tab: undefined, copySelection: false }).map(i => i.action).slice(0, 2)).toEqual(['open', 'newWindow']);
  });
  it('без вкладки (отдельное окно документа) группы tab нет', () => {
    expect(groups({ ...base, tab: undefined })).not.toContain('tab');
  });
  it('серые пункты вкладки несут причину и остаются в меню', () => {
    const items = documentMenu({ ...base, tab: { others: 0, right: 0, canMove: false } });
    for (const action of ['closeOthers', 'closeRight', 'moveTab']) expect(items.find(i => i.action === action)).toMatchObject({ disabled: true, hint: expect.any(String) });
    expect(items.find(i => i.action === 'closeTab')?.disabled).toBe(false);
  });
  it('документ вне дерева: изменить и удалить серые с причиной, остальное доступно', () => {
    const items = documentMenu({ ...base, inTree: false });
    for (const id of ['target.rename', 'target.move', 'target.delete']) expect(items.find(i => i.id === id)).toMatchObject({ disabled: true, hint: expect.stringContaining('дереве') });
    expect(items.find(i => i.id === 'target.copyPath')?.disabled).toBe(false);
  });
  it('запуск воркера блокирует изменение и проверку, состояние unavailable — только проверку', () => {
    const starting = documentMenu({ ...base, state: 'starting' });
    for (const id of ['target.rename', 'target.move', 'target.delete', 'target.checkIndex', 'target.reveal']) expect(starting.find(i => i.id === id)?.disabled).toBe(true);
    expect(starting.find(i => i.id === 'tab.close')?.disabled).toBe(false);
    const down = documentMenu({ ...base, state: 'unavailable' });
    expect(down.find(i => i.id === 'target.checkIndex')?.disabled).toBe(true);
    expect(down.find(i => i.id === 'target.rename')?.disabled).toBe(false);
  });
  it('редактор: сначала шесть команд правки, затем одно подменю «Документ» с группами вкладки', () => {
    const items = documentMenu({ ...base, place: 'editor', text: { undo: 'Нечего отменять', cut: 'Нет выделенного текста' } });
    expect(items.map(i => i.action)).toEqual(['undo', 'redo', 'cut', 'copy', 'paste', 'selectAll', 'documentSub']);
    expect(items.find(i => i.action === 'undo')).toMatchObject({ disabled: true, hint: 'Нечего отменять', shortcut: 'Ctrl+Z' });
    expect(items.find(i => i.action === 'redo')).toMatchObject({ disabled: false, shortcut: 'Ctrl+Y' });
    const sub = items.at(-1)!;
    expect(sub.group).toBe('open');
    expect([...new Set(sub.children!.map(i => i.group))]).toEqual(['tab', 'open', 'modify', 'path', 'index', 'delete']);
    expect(sub.children!.some(i => i.action === 'editors' || i.children)).toBe(false);
    expect(documentMenu({ ...base, place: 'editor', mac: true }).find(i => i.action === 'redo')?.shortcut).toBe('⇧⌘Z');
  });
  it('редакторы раскрываются плоско внутри «Документ»; по умолчанию — первым', () => {
    const nested = nestedEditors([{ id: 'code', label: 'VS Code' }]);
    expect(nested.map(i => i.label)).toEqual(['Открыть в программе по умолчанию', 'Открыть в VS Code']);
    expect(nested.every(i => i.id === 'target.openExternal' && i.group === 'open')).toBe(true);
  });
  it('сочетание вкладки берётся из реестра при построении', () => {
    const items = documentMenu(base, id => id === 'tab.close' ? 'Ctrl+W' : '');
    expect(items.find(i => i.action === 'closeTab')?.shortcut).toBe('Ctrl+W');
    expect(items.find(i => i.action === 'rename')?.shortcut).toBe('');
  });
});

