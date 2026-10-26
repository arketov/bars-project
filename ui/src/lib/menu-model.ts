import type { MenuAction, MenuItem } from './contextmenu';

export const menuGroups = ['edit', 'tab', 'open', 'create', 'modify', 'path', 'style', 'index', 'delete'] as const;
export type MenuGroup = typeof menuGroups[number];
export interface ModelItem extends MenuItem {
  id: string;
  group: MenuGroup;
  order: number;
  danger?: boolean;
  shortcut?: string;
  children?: ModelItem[];
}
export interface TreeMenuTarget {
  kind: 'module' | 'dir' | 'file';
  path: string;
  state: string;
  workerDown?: boolean;
  missing?: boolean;
  canStyle?: boolean;
  copySelection?: boolean;
  mac?: boolean;
}
const definitions: Partial<Record<MenuAction, [string, MenuGroup, number]>> = {
  copy: ['text.copy', 'edit', 0], open: ['target.open', 'open', 0],
  newWindow: ['document.newWindow', 'open', 1], editors: ['target.openExternal', 'open', 2],
  newDoc: ['file.create', 'create', 0], newDir: ['folder.create', 'create', 1],
  rename: ['target.rename', 'modify', 0], move: ['target.move', 'modify', 1],
  reveal: ['target.reveal', 'path', 0], path: ['target.copyPath', 'path', 1],
  appearance: ['target.style', 'style', 0], check: ['target.checkIndex', 'index', 0],
  checkProject: ['target.checkIndex', 'index', 0], trash: ['target.delete', 'delete', 0],
};
/** Общий порядок; разделители рендерятся только между непустыми группами. */
export function groupMenu(items: ModelItem[]): ModelItem[] {
  return [...items].sort((a, b) => menuGroups.indexOf(a.group) - menuGroups.indexOf(b.group) || a.order - b.order);
}
/** Единственная глубина подменю; editor — аргумент команды, а не отдельная команда. */
export function editorMenu(editors: readonly { id: string; label: string }[]): ModelItem[] {
  return [{ id: 'default', label: 'Программа по умолчанию' }, ...editors.filter(editor => editor.id !== 'default')]
    .map((editor, order) => ({ id: 'target.openExternal', action: 'editor', label: editor.label,
      group: 'open', order, editor: editor.id }));
}
export function treeMenu(target: TreeMenuTarget, hint: (id: string) => string = () => ''): ModelItem[] {
  const out: ModelItem[] = [];
  const add = (action: MenuAction, label: string, reason?: string, explanation?: string) => {
    const [id, group, order] = definitions[action]!;
    out.push({ id, group, order, action, label, danger: group === 'delete', disabled: !!reason,
      hint: reason || explanation, shortcut: action === 'copy' ? (target.mac ? '⌘C' : 'Ctrl+C') : hint(id) });
  };
  const busy = target.workerDown ? 'Нет связи с воркером. Дождитесь подключения.'
    : target.state === 'starting' ? 'Воркер запускается. Подождите несколько секунд.' : undefined;
  const index = busy || (target.state === 'unavailable' ? 'Проект недоступен. Восстановите доступ к папке.'
    : target.state === 'reindex_required' ? 'Нужно согласие на смену модели.' : undefined);
  const file = target.kind === 'file';
  if (target.copySelection) add('copy', 'Копировать');
  if (file) {
    add('open', 'Открыть'); add('newWindow', 'Открыть в новом окне'); add('editors', 'Открыть в …', busy);
  } else {
    add('newDoc', 'Новый документ…', busy); add('newDir', 'Новая папка…', busy);
  }
  if (target.path !== '.') {
    add('rename', 'Переименовать…', busy); add('move', 'Переместить…', busy);
  }
  add('reveal', target.mac ? 'Показать в Finder' : 'Показать в Проводнике', busy || (target.missing ? 'Папка не найдена' : undefined));
  add('path', 'Скопировать путь', busy, target.missing ? 'Папка отсутствует; последний путь' : undefined);
  if (target.canStyle && target.path !== '.') add('appearance', file ? 'Цвет…' : 'Цвет и иконка…');
  add(file ? 'check' : 'checkProject', file ? 'Проверить индекс' : 'Проверить индекс проекта', index, file ? undefined : 'Проверяется весь проект');
  if (target.path !== '.') add('trash', 'Удалить…', busy);
  return groupMenu(out);
}

/** Ближайший объект останавливает поиск: вложенная цель не наследует меню документа. */
export function menuTargetAt(target: EventTarget | null): HTMLElement | null {
  return target instanceof Element ? target.closest<HTMLElement>('[data-menu-kind], [data-document-path], [data-folder-path], [data-project-path], [data-new-topic-id]') : null;
}
