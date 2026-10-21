import { panelTitles } from './workspace-layout';
export type Platform = 'windows' | 'macos';
export type Context = 'window' | 'panel' | 'document' | 'editor' | 'graph' | 'target' | 'result' | 'topics' | 'notification' | 'mcp' | 'logs' | 'indexing';
export interface Binding { code: string; modifiers: string[] }
export type Overrides = Record<string, Binding | null>;
export interface Profile { version: number; revision: number; platforms: Partial<Record<Platform, Overrides>> }
export interface Snapshot { profile: Profile; error: string | null; recovery: string | null }
export interface Command { id: string; title: string; context: Context; category: string; shortcut?: string; future?: string }
const commands: Command[] = [];
function add(context: Context, category: string, rows: [string, string, string?, string?][]) {
  for (const [id, title, shortcut, future] of rows) commands.push({ id, title, context, category, shortcut, future });
}
add('window', 'Окно', [
  ['commands.open', 'Список действий', 'Mod+Shift+KeyP'], ['search.open', 'Глобальный поиск', 'Mod+Shift+KeyF'],
  ['search.main', 'Главный поиск'], ['document.open', 'Открыть документ по имени', 'Mod+KeyO', 'Этап 3.4'],
  ['settings.open', 'Настройки', 'Mod+Comma'], ['settings.keybindings', 'Сочетания клавиш'],
  ['project.choose', 'Выбрать проект'], ['project.add', 'Добавить проект'], ['project.checkIndex', 'Проверить актуальность индекса'], ['project.remove', 'Удалить проект из bars'],
  ['panel.open', 'Открыть панель…'], ['notifications.open', 'Уведомления'], ['layout.reset', 'Сбросить раскладку'],
  ['panel.next', 'Следующая группа', 'F6'], ['panel.previous', 'Предыдущая группа', 'Shift+F6'],
]);
for (const [kind, title] of Object.entries(panelTitles)) add('window', 'Панели', [[`panel.open.${kind}`, title]]);
add('panel', 'Вкладки', [
  ['tab.next', 'Следующая вкладка', 'Control+Tab'], ['tab.previous', 'Предыдущая вкладка', 'Control+Shift+Tab'],
  ['tab.close', 'Закрыть вкладку', 'Mod+KeyW'], ['tab.move', 'Переместить вкладку…', 'Mod+Shift+KeyM'], ['panel.resize', 'Изменить размер группы…'],
]);
add('document', 'Документ', [
  ['document.newWindow', 'Открыть документ в новом окне', '', 'Этап 3.4'], ['document.preview', 'Исходный текст / рендер', 'Mod+Shift+KeyE', 'Этап 3.4'],
  ['document.outline', 'Показать / скрыть структуру', '', 'Этап 3.4'], ['document.links', 'Входящие / исходящие ссылки', '', 'Этап 3.5'],
  ['document.find', 'Найти в документе', 'Mod+KeyF', 'Этап 3.4'], ['document.save', 'Сохранить сейчас', 'Mod+KeyS', 'Этап 3.4'], ['document.syntaxHelp', 'Шпаргалка Markdown', '', 'Этап 3.4'],
]);
add('window', 'Поиск', [['search.scope', 'Область поиска'], ['search.scopeClear', 'Сбросить область поиска'], ['search.options', 'Параметры поиска']]);
add('result', 'Результат', [['result.open', 'Открыть результат'], ['result.graph', 'Показать результат в графе', '', 'Блок 2'], ['result.expand', 'Раскрыть результат']]);
add('target', 'Файл и папка', [['target.open', 'Открыть документ'], ['target.reveal', 'Показать в ОС'], ['target.copyPath', 'Скопировать путь']]);
add('target', 'Файл и папка', ['file.create|Создать файл', 'folder.create|Создать папку', 'target.rename|Переименовать', 'target.move|Переместить', 'target.delete|Удалить'].map(row => { const [id, title] = row.split('|'); return [id, title, '', 'Этап 3.3']; }));
add('window', 'Проект', [['project.create', 'Создать проект', '', 'Этап 3.3.2']]);
add('topics', 'Новые темы', [['topics.copyQuery', 'Скопировать исходный запрос'], ['topics.rename', 'Переименовать тему'], ['topics.delete', 'Удалить тему']]);
add('notification', 'Уведомления', [['notification.details', 'Подробности уведомления'], ['notification.retry', 'Повторить операцию']]);
add('window', 'Уведомления', [['notifications.readAll', 'Прочитать уведомления'], ['notifications.clear', 'Очистить завершённые уведомления']]);
add('mcp', 'MCP', [['mcp.details', 'Подробности вызова MCP'], ['mcp.clear', 'Очистить историю MCP']]);
add('logs', 'Логи', [['logs.refresh', 'Обновить журнал'], ['logs.copy', 'Скопировать журнал'], ['logs.reveal', 'Открыть папку логов']]);
add('indexing', 'Индексация', [['indexing.details', 'Подробности задания']]);
add('window', 'Аудиты', ['links|ссылок', 'duplicates|дублей', 'contradictions|противоречий', 'topics|новых тем'].map(row => { const [id, title] = row.split('|'); return [`audit.run.${id}`, `Аудит ${title}`, '', 'Этап 3.8']; }));
add('editor', 'Редактор', [
  ['editor.bold', 'Жирный', 'Mod+KeyB'], ['editor.italic', 'Курсив', 'Mod+KeyI'], ['editor.link', 'Ссылка на документ', 'Mod+KeyK'],
  ...['heading1|Заголовок 1', 'heading2|Заголовок 2', 'heading3|Заголовок 3', 'code|Строчный код', 'codeBlock|Блок кода', 'bulletList|Маркированный список', 'numberedList|Нумерованный список', 'task|Задача', 'quote|Цитата', 'table|Таблица'].map(row => { const [id, title] = row.split('|'); return [`editor.${id}`, title] as [string, string]; }),
].map(row => [row[0], row[1], row[2], 'Этап 3.4']));
add('graph', 'Граф', ['view|Выбрать вид графа', 'filters|Фильтры графа', 'find|Найти узел', 'more|Следующая порция', 'source|Открыть источник', 'inspect|Инспектор', 'expand|Раскрыть документ', 'collapse|Свернуть документ', 'local|Локальный граф', 'project|Граф проекта', 'query|Граф запроса', 'pathStart|Первая вершина пути', 'pathEnd|Вторая вершина пути', 'pathNext|Следующий путь', 'pathPrevious|Предыдущий путь', 'impact|Влияние изменения', 'scope|Подграф как область поиска', 'context|Пакет контекста', 'pin|Закрепить узел', 'motion|Остановить движение', 'zoomIn|Увеличить граф', 'zoomOut|Уменьшить граф', 'fit|Уместить граф', 'pan|Перемещение камеры', 'list|Список графа', 'legend|Легенда'].map(row => { const [id, title] = row.split('|'); return [`graph.${id}`, title, '', 'Блок 2']; }));
export const catalog: readonly Command[] = commands;
export const platform: Platform = typeof navigator !== 'undefined' && /Mac/.test(navigator.platform) ? 'macos' : 'windows';
export function defaultBinding(command: Command, os: Platform): Binding | null {
  if (!command.shortcut) return null;
  const parts = command.shortcut.replace('Mod', os === 'macos' ? 'Meta' : 'Control').split('+');
  return { code: parts.pop()!, modifiers: parts.sort() };
}
export function bindingFor(command: Command, overrides: Overrides, os: Platform) {
  return Object.hasOwn(overrides, command.id) ? overrides[command.id] : defaultBinding(command, os);
}
export const signature = (binding: Binding | null) => binding ? `${binding.modifiers.join('+')}+${binding.code}` : '';
const ru: Record<string, string> = Object.fromEntries([..."QЙ WЦ EУ RК TЕ YН UГ IШ OЩ PЗ AФ SЫ DВ FА GП HР JО KЛ LД ZЯ XЧ CС VМ BИ NТ MЬ".split(' ')].map(pair => [`Key${pair[0]}`, pair[1]]));
export function label(binding: Binding | null): string {
  if (!binding) return 'Не назначено';
  const key = binding.code.replace(/^Key|^Digit/, '').replace('Comma', ',');
  return [...binding.modifiers.map(m => ({ Control: 'Ctrl', Meta: 'Cmd' }[m] ?? m)), key].join('+') + (ru[binding.code] ? ` (${ru[binding.code]})` : '');
}
export function fromEvent(event: KeyboardEvent): Binding | null {
  if (!event.code || event.isComposing || event.keyCode === 229 || ['Dead', 'Process'].includes(event.key) || event.getModifierState('AltGraph')) return null;
  return { code: event.code, modifiers: [event.altKey && 'Alt', event.ctrlKey && 'Control', event.metaKey && 'Meta', event.shiftKey && 'Shift'].filter(Boolean) as string[] };
}
export function invalid(binding: Binding, os: Platform): string | null {
  const { code, modifiers: mods } = binding;
  const primary = mods.includes(os === 'macos' ? 'Meta' : 'Control');
  if (mods.some(m => !['Alt', 'Control', 'Meta', 'Shift'].includes(m)) || new Set(mods).size !== mods.length || mods.join() !== [...mods].sort().join()) return 'Модификаторы должны быть уникальны и отсортированы';
  if (os === 'windows' && mods.includes('Meta')) return 'Сочетания Win принадлежат ОС';
  if (['Escape', 'Enter', 'Space', 'Backspace', 'Delete', 'Insert'].includes(code) || /^(Arrow|Shift|Control|Alt|Meta)/.test(code)) return 'Клавиша принадлежит вводу или ОС';
  if (os === 'macos' && mods.includes('Meta') && (['Tab', 'Space', 'KeyQ', 'KeyH'].includes(code) || code === 'KeyM' && mods.length === 1)) return 'Сочетание принадлежит macOS';
  if (mods.includes('Alt') && !primary) return 'Alt/Option требует Ctrl/Cmd';
  if (primary && ['KeyA', 'KeyC', 'KeyX', 'KeyV', 'KeyZ', 'KeyY'].includes(code)) return 'Сочетание защищено для редактирования текста';
  if (code === 'F5' || primary && code === 'KeyR') return 'Перезагрузка WebView не назначается';
  if (/^F([1-9]|1\d|2[0-4])$/.test(code)) return null;
  if (code === 'Tab' && mods.includes('Control') && !mods.includes('Meta') && !mods.includes('Alt')) return null;
  if (!primary || !/^(Key[A-Z]|Digit[0-9]|Comma|Period|Slash|Semicolon|Quote|BracketLeft|BracketRight|Backslash|Minus|Equal|Backquote)$/.test(code)) return 'Нужны Ctrl/Cmd и буква, цифра или знак; либо F1–F24';
  return null;
}
export function overlaps(a: Context, b: Context) { return a === b || !(['editor', 'graph'].includes(a) && ['editor', 'graph'].includes(b)); }
export function conflicts(overrides: Overrides, os: Platform): [Command, Command][] {
  const pairs: [Command, Command][] = [];
  catalog.forEach((a, i) => { const key = signature(bindingFor(a, overrides, os)); if (key) for (const b of catalog.slice(i + 1)) if (overlaps(a.context, b.context) && signature(bindingFor(b, overrides, os)) === key) pairs.push([a, b]); });
  return pairs;
}
export function validate(overrides: Overrides, os: Platform): string[] {
  return [...catalog.flatMap(c => { const b = bindingFor(c, overrides, os); const error = b && invalid(b, os); return error ? [`${c.title}: ${error}`] : []; }), ...conflicts(overrides, os).map(([a, b]) => `Конфликт: ${a.title} ↔ ${b.title}`)];
}

/** Новое умолчание уступает сохранённому назначению; два пользовательских конфликта отклоняют карту. */
export function runtimeOverrides(overrides: Overrides, os: Platform): { active: Overrides; errors: string[] } {
  const active = { ...overrides }, errors = validate(overrides, os);
  for (const [a,b] of conflicts(overrides,os)) {
    const ownA=Object.hasOwn(overrides,a.id), ownB=Object.hasOwn(overrides,b.id);
    if (ownA === ownB) return { active: {}, errors };
    active[ownA ? b.id : a.id] = null;
  }
  return { active: validate(active,os).length ? {} : active, errors };
}
