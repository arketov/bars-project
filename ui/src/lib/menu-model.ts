import type { MenuAction, MenuItem } from './contextmenu';
import { cannotMoveTab, noOtherTabs, noRightTabs } from './workspace-tabs';

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
  undo: ['text.undo', 'edit', 0], redo: ['text.redo', 'edit', 1], cut: ['text.cut', 'edit', 2],
  paste: ['text.paste', 'edit', 4], selectAll: ['text.selectAll', 'edit', 5],
  closeTab: ['tab.close', 'tab', 0], closeOthers: ['tab.closeOthers', 'tab', 1],
  closeRight: ['tab.closeRight', 'tab', 2], moveTab: ['tab.move', 'tab', 3], documentSub: ['document.menu', 'open', 3],
  copy: ['text.copy', 'edit', 3], open: ['target.open', 'open', 0],
  newWindow: ['document.newWindow', 'open', 1], editors: ['target.openExternal', 'open', 2],
  newDoc: ['file.create', 'create', 0], newDir: ['folder.create', 'create', 1],
  rename: ['target.rename', 'modify', 0], move: ['target.move', 'modify', 1],
  reveal: ['target.reveal', 'path', 0], path: ['target.copyPath', 'path', 1],
  appearance: ['target.style', 'style', 0], check: ['target.checkIndex', 'index', 0],
  checkProject: ['target.checkIndex', 'index', 0], trash: ['target.delete', 'delete', 0],
  copyLink: ['link.copy', 'path', 2], createTarget: ['link.createTarget', 'create', 2],
  goHeading: ['heading.goTo', 'open', 0], copyHeading: ['heading.copyLink', 'path', 2],
  copyTopic: ['topics.copyQuery', 'path', 2], closeTopic: ['topics.delete', 'delete', 0],
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

export type DocumentPlace = 'card' | 'tab' | 'header' | 'render' | 'editor';
export const documentPlaces: readonly DocumentPlace[] = ['card', 'tab', 'header', 'render', 'editor'];
export type TextCommand = 'undo' | 'redo' | 'cut' | 'copy' | 'paste' | 'selectAll';
export interface DocumentMenuTarget {
  path: string;
  place: DocumentPlace;
  state: string;
  workerDown?: boolean;
  mac?: boolean;
  /** Документ есть среди файлов дерева: переименование, перемещение и удаление идут по узлу дерева. */
  inTree: boolean;
  /** Документ открыт во вкладке Dockview; без вкладки (карточка, отдельное окно) группы `tab` нет. */
  tab?: { others: number; right: number; canMove: boolean };
  /** Выделен текст вне редактора: в меню карточки и рендера появляется «Копировать». */
  copySelection?: boolean;
  /** Редактор: причина недоступности каждой команды правки; нет записи — команда доступна. */
  text?: Partial<Record<TextCommand, string>>;
}
const textLabels: Record<TextCommand, string> = {
  undo: 'Отменить', redo: 'Повторить', cut: 'Вырезать', copy: 'Копировать', paste: 'Вставить', selectAll: 'Выделить всё',
};
/** Штатные сочетания правки текста: меню их только показывает, назначения принадлежат ОС. */
function textShortcut(command: TextCommand, mac?: boolean): string {
  if (command === 'redo') return mac ? '⇧⌘Z' : 'Ctrl+Y';
  const key = { undo: 'Z', cut: 'X', copy: 'C', paste: 'V', selectAll: 'A' }[command];
  return mac ? `⌘${key}` : `Ctrl+${key}`;
}
function item(action: MenuAction, label: string, extra: { reason?: string; explanation?: string; shortcut?: string } = {}): ModelItem {
  const [id, group, order] = definitions[action]!;
  return { id, group, order, action, label, danger: group === 'delete', disabled: !!extra.reason,
    hint: extra.reason || extra.explanation, shortcut: extra.shortcut ?? '' };
}
/** Группа `tab`: четыре пункта вкладки документа и служебной панели с причинами недоступности. */
function tabItems(tab: NonNullable<DocumentMenuTarget['tab']>, hint: (id: string) => string): ModelItem[] {
  return [item('closeTab', 'Закрыть', { shortcut: hint('tab.close') }),
    item('closeOthers', 'Закрыть другие', { reason: tab.others ? undefined : noOtherTabs, shortcut: hint('tab.closeOthers') }),
    item('closeRight', 'Закрыть справа', { reason: tab.right ? undefined : noRightTabs, shortcut: hint('tab.closeRight') }),
    item('moveTab', 'Переместить вкладку…', { reason: tab.canMove ? undefined : cannotMoveTab, shortcut: hint('tab.move') })];
}
/**
 * Меню документа на вкладке, в шапке, на карточке, в рендере и в редакторе: один набор групп на все места.
 * Редактор — правка текста и подменю «Документ ▸» с группами вкладки; `nested` строит это подменю.
 */
export function documentMenu(target: DocumentMenuTarget, hint: (id: string) => string = () => '', nested = false): ModelItem[] {
  const out: ModelItem[] = [];
  const busy = target.workerDown ? 'Нет связи с воркером. Дождитесь подключения.'
    : target.state === 'starting' ? 'Воркер запускается. Подождите несколько секунд.' : undefined;
  const index = busy || (target.state === 'unavailable' ? 'Проект недоступен. Восстановите доступ к папке.'
    : target.state === 'reindex_required' ? 'Нужно согласие на смену модели.' : undefined);
  const editing = target.place === 'editor' && !nested;
  if (editing) {
    for (const command of ['undo', 'redo', 'cut', 'copy', 'paste', 'selectAll'] as const) {
      out.push(item(command, textLabels[command], { reason: target.text?.[command], shortcut: textShortcut(command, target.mac) }));
    }
    out.push(item('documentSub', 'Документ', { shortcut: '' }));
    out.at(-1)!.children = documentMenu({ ...target, place: 'tab' }, hint, true);
    return groupMenu(out);
  }
  if (target.copySelection && (target.place === 'card' || target.place === 'render')) {
    out.push(item('copy', 'Копировать', { shortcut: textShortcut('copy', target.mac) }));
  }
  if (target.tab && target.place !== 'card') out.push(...tabItems(target.tab, hint));
  if (target.place === 'card') out.push(item('open', 'Открыть'));
  out.push(item('newWindow', 'Открыть в новом окне', { shortcut: hint('document.newWindow') }));
  // Одна глубина подменю: внутри «Документ ▸» редакторы раскрывает меню при открытии подменю.
  if (!nested) out.push(item('editors', 'Открыть в …', { reason: busy }));
  const change = busy || (target.inTree ? undefined : 'Документа нет в дереве проекта.');
  out.push(item('rename', 'Переименовать…', { reason: change }), item('move', 'Переместить…', { reason: change }),
    item('reveal', target.mac ? 'Показать в Finder' : 'Показать в Проводнике', { reason: busy }),
    item('path', 'Скопировать путь', { reason: busy }),
    item('check', 'Проверить индекс', { reason: index }),
    item('trash', 'Удалить…', { reason: change }));
  return groupMenu(out);
}
/** Редакторы внутри подменю «Документ ▸»: пункты верхнего уровня, а не вложенное подменю. */
export function nestedEditors(editors: readonly { id: string; label: string }[]): ModelItem[] {
  return editorMenu(editors).map((editor, order) => ({ ...editor, order: 2 + order / 100,
    label: editor.editor === 'default' ? 'Открыть в программе по умолчанию' : `Открыть в ${editor.label}` }));
}

/** Меню вкладки служебной панели: только группа `tab`. */
export function panelTabMenu(tab: NonNullable<DocumentMenuTarget['tab']>, hint: (id: string) => string = () => ''): ModelItem[] {
  return groupMenu(tabItems(tab, hint));
}

export interface LinkMenuTarget {
  /** Документ-цель найден среди файлов проекта. */
  resolved: boolean;
  /** Путь для «Создать документ» допустим. */
  canCreate: boolean;
  workerDown?: boolean;
  copySelection?: boolean;
  mac?: boolean;
}
/** Меню ссылки в тексте и исходящей строки панели «Ссылки»: цель есть — открыть и скопировать, битая — создать и скопировать. */
export function linkMenu(target: LinkMenuTarget): ModelItem[] {
  const out: ModelItem[] = [];
  const busy = target.workerDown ? 'Нет связи с воркером. Дождитесь подключения.' : undefined;
  if (target.copySelection) out.push(item('copy', 'Копировать', { shortcut: textShortcut('copy', target.mac) }));
  if (target.resolved) {
    out.push(item('open', 'Открыть цель'), item('newWindow', 'Открыть в новом окне', { reason: busy }));
  } else out.push(item('createTarget', 'Создать документ', { reason: busy || (target.canCreate ? undefined : 'Недопустимый путь ссылки') }));
  out.push(item('copyLink', 'Скопировать ссылку'));
  return groupMenu(out);
}

/** Строки экранов: команды реестра, чьи обработчики экран зарегистрировал на строке. */
export type RowKind = 'indexing' | 'mcp' | 'notification' | 'log';
export const rowKinds: readonly RowKind[] = ['indexing', 'mcp', 'notification', 'log'];
const rowCommands: Record<RowKind, { id: string; action: MenuAction; label: string; group: MenuGroup; order: number; onlyWhenAllowed?: boolean }[]> = {
  indexing: [{ id: 'indexing.details', action: 'details', label: 'Подробности', group: 'open', order: 0 }],
  mcp: [{ id: 'mcp.details', action: 'details', label: 'Подробности', group: 'open', order: 0 }],
  notification: [
    { id: 'notification.details', action: 'details', label: 'Подробности', group: 'open', order: 0 },
    { id: 'notification.retry', action: 'retry', label: 'Повторить', group: 'open', order: 1, onlyWhenAllowed: true },
  ],
  log: [{ id: 'logs.copy', action: 'copyLog', label: 'Скопировать журнал', group: 'path', order: 0 }],
};
/**
 * `reason(id)`: `null` — у строки нет обработчика команды, пункта нет; строка — команда сейчас недоступна;
 * `undefined` — доступна. «Повторить» недоступное не показывается: повтор допустим не везде.
 */
export function rowMenu(kind: RowKind, reason: (id: string) => string | null | undefined, hint: (id: string) => string = () => '', copySelection = false, mac = false): ModelItem[] {
  const out: ModelItem[] = [];
  if (copySelection) out.push(item('copy', 'Копировать', { shortcut: textShortcut('copy', mac) }));
  for (const command of rowCommands[kind]) {
    const why = reason(command.id);
    if (why === null || (command.onlyWhenAllowed && why)) continue;
    out.push({ id: command.id, group: command.group, order: command.order, action: command.action, label: command.label,
      disabled: !!why, hint: why || undefined, shortcut: hint(command.id) });
  }
  return groupMenu(out);
}

/** Меню заголовка структуры: обработчики команд зарегистрированы на строке структуры. */
export function headingMenu(reason: (id: string) => string | null | undefined, hint: (id: string) => string = () => '', copySelection = false, mac = false): ModelItem[] {
  const out: ModelItem[] = [];
  if (copySelection) out.push(item('copy', 'Копировать', { shortcut: textShortcut('copy', mac) }));
  for (const [action, label] of [['goHeading', 'Перейти к заголовку'], ['copyHeading', 'Скопировать ссылку на заголовок']] as const) {
    const [id] = definitions[action]!;
    const why = reason(id);
    if (why !== null) out.push(item(action, label, { reason: why || undefined, shortcut: hint(id) }));
  }
  return groupMenu(out);
}

/** Строка «Новых тем»: запрос копируется, закрытие опасное и серое, пока идёт операция с темами. */
export function topicMenu(busy: boolean, copySelection = false, mac = false): ModelItem[] {
  const out: ModelItem[] = [];
  if (copySelection) out.push(item('copy', 'Копировать', { shortcut: textShortcut('copy', mac) }));
  out.push(item('copyTopic', 'Скопировать запрос'), item('closeTopic', 'Закрыть тему', { reason: busy ? 'Идёт операция с темами' : undefined }));
  return groupMenu(out);
}

/** Ближайший объект останавливает поиск: вложенная цель не наследует меню документа. */
export function menuTargetAt(target: EventTarget | null): HTMLElement | null {
  return target instanceof Element ? target.closest<HTMLElement>('[data-menu-kind], [data-document-path], [data-folder-path], [data-project-path], [data-new-topic-id]') : null;
}
