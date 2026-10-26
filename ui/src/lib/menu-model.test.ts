import { describe, expect, it } from 'vitest';
import { menuTargetAt, treeMenu } from './menu-model';
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
