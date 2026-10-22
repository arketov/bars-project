import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import Workspace from "./Workspace.svelte";
import WorkspaceDocument from "./WorkspaceDocument.svelte";
import { app } from "./app.svelte";
import { editors } from "./editors.svelte";
import { EditorView } from "@codemirror/view";
import { documentId, layoutKey } from "./workspace-layout";
import { call } from "./api";
import { mockCall } from "./mock";
import { settings } from "./settings.svelte";
import { BarsError, type SettingsOutput } from "./types";
import { keybindings } from './keybindings.svelte';
vi.mock("./api", async original => ({ ...await original<typeof import("./api")>(), call: vi.fn() }));
vi.mock("./graph-renderer", () => ({ GraphRenderer: class { set() {} destroy() {} } }));
let component: ReturnType<typeof mount> | undefined;
const project = { path: "C:/А", name: "А", docs_root: "docs", state: "ready" as const, modules: [{ name: "manual", documents: 1 }] };
const hit = (path: string) => ({ path, title: path, first_line: 3, last_line: 4 });
const settle = async () => { await new Promise(resolve => setTimeout(resolve, 0)); flushSync(); };
beforeEach(() => {
  keybindings.active = {}; keybindings.recording = false;
  localStorage.clear(); vi.mocked(call).mockReset();
  vi.mocked(call).mockImplementation(() => new Promise(() => {}));
  app.projects = [project]; app.current = project.path; app.view = "main"; app.settingsTab = "models"; app.navigationVersion = 0; app.topicsVersion = 0;
  app.workerDown = null; app.viewer = null; app.topicsOpen = false; app.tree = []; app.activity = null;
  app.results = null; app.searchError = null; app.searching = false; app.consentOpen = false; app.deletingProject = null;
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
});

it('горячая клавиша RU открывает общий поиск; повтор фокусирует запрос, IME не выполняется', async () => {
  component=mount(Workspace,{target:document.body});flushSync();await settle();
  app.openSettings();flushSync();await settle();
  const send=(options:KeyboardEventInit={})=>window.dispatchEvent(new KeyboardEvent('keydown',{bubbles:true,cancelable:true,code:'KeyF',key:'А',ctrlKey:true,shiftKey:true,...options}));
  send({isComposing:true});flushSync();
  expect(document.querySelector<HTMLDialogElement>('dialog[aria-label="Глобальный поиск"]')!.open).toBe(false);
  send();flushSync();await settle();
  const modal=document.querySelector<HTMLDialogElement>('dialog[aria-label="Глобальный поиск"]')!;
  expect(modal.open).toBe(true);
  modal.querySelector<HTMLElement>('button')!.focus();send();flushSync();
  expect(document.activeElement).toBe(modal.querySelector('textarea'));
});

it('список команд сохраняет цель вкладки и доступен кнопкой после снятия сочетания', async () => {
  component=mount(Workspace,{target:document.body});flushSync();await settle();
  app.openSettings();flushSync();await settle();
  window.dispatchEvent(new KeyboardEvent('keydown',{code:'KeyP',key:'З',ctrlKey:true,shiftKey:true,bubbles:true,cancelable:true}));flushSync();await settle();
  const modal=document.querySelector<HTMLDialogElement>('dialog[aria-label="Список действий"]')!;
  expect(modal.open).toBe(true);
  const query=modal.querySelector<HTMLInputElement>('input')!;query.value='Закрыть вкладку';query.dispatchEvent(new Event('input',{bubbles:true}));flushSync();
  query.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true,cancelable:true}));flushSync();await settle();
  expect(document.querySelector('[data-panel-kind="settings"]')).toBeNull();
  keybindings.active={'commands.open':null};
  document.querySelector<HTMLButtonElement>('[aria-label="Список действий"]')!.click();flushSync();expect(modal.open).toBe(true);
});

it('команда настроек клавиатуры не сбрасывается обычным открытием панели', async()=>{
  component=mount(Workspace,{target:document.body});flushSync();await settle();
  document.querySelector<HTMLButtonElement>('[aria-label="Список действий"]')!.click();flushSync();
  const query=document.querySelector<HTMLInputElement>('dialog[aria-label="Список действий"] input')!;
  query.value='Сочетания клавиш';query.dispatchEvent(new Event('input',{bubbles:true}));flushSync();
  query.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true,cancelable:true}));flushSync();await settle();
  expect(app.view).toBe('settings');
  expect(app.settingsTab).toBe('keybindings');
});


it("глобальный поиск из настроек переносит те же узлы и не меняет раскладку", async () => {
  component = mount(Workspace, { target: document.body }); flushSync(); await settle();
  const query = document.querySelector<HTMLTextAreaElement>('textarea')!;
  const tree = document.querySelector('aside');
  app.query = "сохранить"; app.selected = new Set(["manual"]); app.filter = "папка";
  app.openSettings("logs"); flushSync(); await settle();
  const layout = localStorage.getItem(layoutKey(project.path));
  const launcher = document.querySelector<HTMLButtonElement>('[aria-label="Глобальный поиск"]')!;
  launcher.focus(); launcher.click(); flushSync(); await settle();
  const dialog = document.querySelector<HTMLDialogElement>('dialog[aria-label="Глобальный поиск"]')!;
  expect(dialog.open).toBe(true); expect(dialog.querySelector('textarea')).toBe(query);
  expect(dialog.querySelector('aside')).toBe(tree); expect(document.activeElement).toBe(query);
  dialog.dispatchEvent(new Event('cancel', { cancelable: true })); flushSync(); await settle();
  expect(dialog.open).toBe(false); expect(document.activeElement).toBe(launcher);
  expect(document.querySelector('[data-panel-kind="search"] textarea')).toBe(query);
  expect(localStorage.getItem(layoutKey(project.path))).toBe(layout);
  expect(app.query).toBe("сохранить");
  expect([...app.selected]).toEqual(["manual"]); expect(app.filter).toBe("папка");
});

it("глобальный поиск доступен с закрытыми панелями без их восстановления", async () => {
  component = mount(Workspace, { target: document.body }); flushSync(); await settle();
  const query = document.querySelector('textarea');
  document.querySelector('[data-workspace-panel="search"]')!.closest('[role="tab"]')!.querySelector<HTMLElement>('.dv-default-tab-action')!.click();
  flushSync(); await settle();
  const layout = localStorage.getItem(layoutKey(project.path));
  document.querySelector<HTMLButtonElement>('[aria-label="Глобальный поиск"]')!.click(); flushSync(); await settle();
  expect(document.querySelector('dialog[aria-label="Глобальный поиск"] textarea')).toBe(query);
  document.querySelector<HTMLButtonElement>('[aria-label="Закрыть поиск"]')!.click(); flushSync(); await settle();
  expect(document.querySelector('[data-panel-kind="search"]')).toBeNull();
  expect(document.querySelector('.column aside')).not.toBeNull();
  expect(localStorage.getItem(layoutKey(project.path))).toBe(layout);
});

it("смена проекта закрывает слой; тот же поиск возвращается в новые панели", async () => {
  component = mount(Workspace, { target: document.body }); flushSync(); await settle();
  const surface = document.querySelector('[data-panel-kind="search"] .shared-surface');
  document.querySelector<HTMLButtonElement>('[aria-label="Глобальный поиск"]')!.click(); flushSync();
  const next = { ...project, path: "C:/Б", name: "Б" }; app.projects = [project, next];
  await app.select(null); app.current = next.path; flushSync(); await settle();
  expect(document.querySelector<HTMLDialogElement>('dialog[aria-label="Глобальный поиск"]')!.open).toBe(false);
  expect(document.querySelector('[data-panel-kind="search"] .shared-surface')).toBe(surface);
});

it("чтение из слоя завершает переход документом с фокусом, включая удалённый файл", async () => {
  component = mount(Workspace, { target: document.body }); flushSync(); await settle();
  app.openSettings(); flushSync(); await settle();
  document.querySelector<HTMLButtonElement>('[aria-label="Глобальный поиск"]')!.click(); flushSync();
  let resolve!: (value: unknown) => void;
  vi.mocked(call).mockImplementation(() => new Promise(done => resolve = done));
  const reading = app.openDocument(hit('docs/a.md'));
  expect(document.querySelector<HTMLDialogElement>('dialog[aria-label="Глобальный поиск"]')!.open).toBe(true);
  resolve({ title: 'А', text: '# А\n\nТекст' }); await reading; await settle();
  expect(document.querySelector<HTMLDialogElement>('dialog[aria-label="Глобальный поиск"]')!.open).toBe(false);
  expect(document.activeElement).toBe(document.querySelector('.document header'));
  document.querySelector<HTMLButtonElement>('[aria-label="Глобальный поиск"]')!.click(); flushSync();
  app.viewer = { hit: hit('docs/deleted.md'), title: 'Удалён', text: '', missing: true }; flushSync(); await settle();
  expect(document.querySelector<HTMLDialogElement>('dialog[aria-label="Глобальный поиск"]')!.open).toBe(false);
  expect(document.activeElement?.closest('.document')?.textContent).toContain('Файл не найден');
});

it("слой сохраняет общие режимы, показывает ошибку и передаёт согласие на переиндексацию", async () => {
  component = mount(Workspace, { target: document.body }); flushSync(); await settle();
  document.querySelector<HTMLButtonElement>('[aria-label="Глобальный поиск"]')!.click(); flushSync();
  const modal = document.querySelector<HTMLDialogElement>('dialog[aria-label="Глобальный поиск"]')!;
  const mode = modal.querySelector<HTMLSelectElement>('[aria-label="Режим поиска"]')!;
  mode.value = 'mix'; mode.dispatchEvent(new Event('change', { bubbles: true })); flushSync();
  modal.querySelector<HTMLInputElement>('.related-option input')!.click(); flushSync();
  const includeRelated = app.includeRelated;
  modal.querySelector<HTMLButtonElement>('[aria-label="Закрыть поиск"]')!.click(); flushSync();
  expect(app.retrievalMode).toBe('mix'); expect(app.includeRelated).toBe(includeRelated);
  app.searchError = new BarsError('Провайдер недоступен', 'provider_error'); flushSync();
  document.querySelector<HTMLButtonElement>('[aria-label="Глобальный поиск"]')!.click(); flushSync();
  expect(modal.textContent).toContain('Embed-провайдер не ответил');
  expect(modal.textContent).toContain('provider_error');
  app.projects = [{ ...project, state: 'reindex_required' }]; flushSync();
  expect(modal.querySelector<HTMLTextAreaElement>('textarea')!.disabled).toBe(true);
  expect(vi.mocked(call).mock.calls.some(([method]) => method === 'reindex')).toBe(false);
  [...modal.querySelectorAll<HTMLButtonElement>('button')].find(b => b.textContent === 'Переиндексировать…')!.click(); flushSync();
  expect(app.consentOpen).toBe(true); expect(modal.open).toBe(false);
  app.retrievalMode = 'hybrid'; app.includeRelated = true;
});
afterEach(async () => { if (component) await unmount(component); component = undefined; document.body.replaceChildren(); app.viewer = null; app.current = null; settings.draft = null; settings.saved = null; vi.restoreAllMocks(); });
it("несколько документов справа, подсветка и режим вкладки сохраняются при переключении", async () => {
  component = mount(Workspace, { target: document.body }); flushSync();
  app.query = "сохранить запрос";
  const searchHit = { ...hit("docs/a.md"), text: "Секретный фрагмент выдачи" };
  app.viewer = { hit: searchHit, title: "А", text: "# А\n\nНайдено\nтут", missing: false }; flushSync(); await settle();
  app.viewer = { hit: hit("docs/b.md"), title: "Б", text: "# Б\n\nВторой", missing: false }; flushSync(); await settle();
  expect(document.querySelectorAll('[data-panel-kind="document"]')).toHaveLength(2);
  const a = document.querySelector('section[data-document-path="docs/a.md"]')!;
  expect(document.querySelector('[role="tab"] [data-document-path="docs/a.md"]')).not.toBeNull();
  expect(a.querySelector('.cm-editor')).not.toBeNull();
  expect(a.querySelector('.cm-line.cm-hit')?.textContent).toContain("Найдено");
  expect(app.query).toBe("сохранить запрос");
  await settle();
  const saved = JSON.parse(localStorage.getItem(layoutKey(project.path))!);
  expect(Object.keys(saved.layout.panels)).toContain(documentId("docs/a.md"));
  expect(JSON.stringify(saved)).not.toContain("Найдено");
  expect(JSON.stringify(saved)).not.toContain("Секретный фрагмент выдачи");
});
it("документ открывается в редакторе; правка меняет рендер и состояние, глаз переключает вид", async () => {
  component = mount(Workspace, { target: document.body }); flushSync();
  app.viewer = { hit: hit("docs/a.md"), title: "А", text: "# А\n\nТекст", missing: false }; flushSync(); await settle();
  const section = document.querySelector<HTMLElement>('section[data-document-path="docs/a.md"]')!;
  expect(section.querySelector('.cm-content')?.getAttribute("contenteditable")).toBe("true");
  expect(section.querySelector('.md')).toBeNull();
  const buffer = editors.get(project.path, "docs/a.md")!;
  buffer.edit("# А\n\nПравленный текст"); flushSync();
  expect(section.querySelector('[role="status"]')?.textContent).toBe("Изменено");
  section.querySelector<HTMLButtonElement>('[aria-label="Исходник / рендер"]')!.click(); flushSync(); await settle();
  expect(section.querySelector('.md')?.textContent).toContain("Правленный текст");
  expect(section.querySelector('.cm-editor')).toBeNull();
  editors.discard(buffer);
});
it("Ctrl+Ж по физической клавише B делает выделение жирным в редакторе, а вне редактора не срабатывает", async () => {
  component = mount(Workspace, { target: document.body }); flushSync();
  app.viewer = { hit: { ...hit("docs/a.md"), first_line: 0, last_line: 0 }, title: "А", text: "# А\n\nТекст", missing: false }; flushSync(); await settle();
  const cm = EditorView.findFromDOM(document.querySelector<HTMLElement>('section[data-document-path="docs/a.md"] .cm-editor')!)!;
  const buffer = editors.get(project.path, "docs/a.md")!;
  cm.dispatch({ selection: { anchor: 5, head: 10 } }); cm.focus();
  const press = (target: HTMLElement) => target.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, code: "KeyB", key: "и", ctrlKey: true }));
  press(cm.contentDOM); flushSync(); await settle();
  expect(buffer.text).toBe("# А\n\n**Текст**");
  expect(buffer.status).toBe("dirty");
  (document.activeElement as HTMLElement | null)?.blur();
  cm.dispatch({ selection: { anchor: 7, head: 11 } });
  press(document.body); flushSync(); await settle();
  expect(buffer.text).toBe("# А\n\n**Текст**");
  editors.discard(buffer);
});
it("вкладка с конфликтом версий не закрывается молча: «Вернуться» оставляет её, «Закрыть без записи» закрывает", async () => {
  component = mount(Workspace, { target: document.body }); flushSync();
  app.viewer = { hit: hit("docs/a.md"), title: "А", text: "# А", missing: false }; flushSync(); await settle();
  const buffer = editors.get(project.path, "docs/a.md")!;
  buffer.edit("# А моя"); buffer.status = "conflict"; buffer.message = "Файл изменён на диске."; buffer.diskRevision = "внешняя"; flushSync();
  const section = document.querySelector('section[data-document-path="docs/a.md"]')!;
  expect([...section.querySelectorAll("button")].map(b => b.textContent?.trim())).toEqual(expect.arrayContaining(["Оставить мою версию", "Взять версию с диска", "Сохранить мою рядом"]));
  const tab = () => document.querySelector('[role="tab"] [data-document-path="docs/a.md"], [role="tab"][data-document-path="docs/a.md"]');
  const closeTab = () => (tab()!.closest('[role="tab"]')!.querySelector('.dv-default-tab-action') as HTMLElement).click();
  closeTab(); flushSync(); await settle();
  expect(document.querySelector('[role="alertdialog"]')?.textContent).toContain("docs/a.md");
  [...document.querySelectorAll<HTMLButtonElement>('[role="alertdialog"] button')].find(b => b.textContent === "Вернуться")!.click(); flushSync(); await settle();
  expect(tab()).not.toBeNull();
  expect(buffer.text).toBe("# А моя");
  closeTab(); flushSync(); await settle();
  [...document.querySelectorAll<HTMLButtonElement>('[role="alertdialog"] button')].find(b => b.textContent === "Закрыть без записи")!.click(); flushSync(); await settle();
  expect(tab()).toBeNull();
  expect(editors.get(project.path, "docs/a.md")).toBeUndefined();
});
it("повторное действие открывает закрытую панель; испорченная раскладка не мешает запуску", async () => {
  localStorage.setItem(layoutKey(project.path), "{broken");
  component = mount(Workspace, { target: document.body }); flushSync();
  expect(app.toast?.text).toContain("Раскладка повреждена");
  app.openIndexing(); flushSync(); await settle();
  expect(document.querySelector('[data-panel-kind="indexing"]')).not.toBeNull();
  // Закрытие штатной кнопкой Dockview, затем тот же вызов из трея/уведомления.
  const tab = [...document.querySelectorAll('[role="tab"]')].find(t => t.textContent?.includes("Индексация"))!;
  tab.querySelector<HTMLElement>('.dv-default-tab-action')!.click(); flushSync(); await settle();
  expect(document.querySelector('[data-panel-kind="indexing"]')).toBeNull();
  app.openIndexing(); flushSync(); await settle();
  expect(document.querySelector('[data-panel-kind="indexing"]')).not.toBeNull();
});
it("смена проекта убирает документы предыдущего проекта", async () => {
  component = mount(Workspace, { target: document.body }); flushSync();
  app.viewer = { hit: hit("docs/a.md"), title: "А", text: "Первый проект", missing: false }; flushSync(); await settle();
  const next = { ...project, path: "C:/Б", name: "Б" }; app.projects = [project, next];
  app.viewer = null; app.current = next.path; flushSync(); await settle();
  expect(document.querySelector('[data-panel-kind="document"]')).toBeNull();
  expect(document.body.textContent).not.toContain("Первый проект");
});
it("поздний read закрытого документа не возвращает его содержимое", async () => {
  let resolve!: (value: unknown) => void;
  vi.mocked(call).mockImplementation(() => new Promise(done => resolve = done));
  component = mount(WorkspaceDocument, { target: document.body, props: { project: project.path, hit: hit("docs/a.md") } }); flushSync();
  await unmount(component); component = undefined;
  resolve({ title: "А", text: "Поздний текст" }); await settle();
  expect(document.body.textContent).not.toContain("Поздний текст");
});
const openFromMenu = async (title: string) => {
  [...document.querySelectorAll<HTMLButtonElement>('button')].find(b => b.textContent === "Панели ▾")!.click(); flushSync();
  [...document.querySelectorAll<HTMLButtonElement>('.menu button')].find(b => b.textContent === title)!.click(); flushSync(); await settle();
};
it("настройки — отдельный экран, а не вкладка; вкладка «Логи» настроек остаётся в настройках, панель логов — из меню", async () => {
  component = mount(Workspace, { target: document.body }); flushSync();
  app.openTopics(); flushSync(); await settle();
  app.openSettings(); flushSync(); await settle();
  expect(document.querySelector('[data-panel-kind="settings"]')).toBeNull();
  expect(document.querySelector('[data-panel-kind="topics"]')).not.toBeNull();
  app.openSettings("logs"); flushSync(); await settle();
  expect(app.view).toBe("settings");
  expect(document.querySelector('[data-panel-kind="logs"]')).toBeNull();
  app.openSearch(); flushSync(); await settle();
  await openFromMenu("Логи");
  expect(document.querySelector('.workspace')?.getAttribute('data-active-panel')).toBe("logs");
});
it("стрелки и End переключают вкладки без сброса запроса и области", async () => {
  component = mount(Workspace, { target: document.body }); flushSync();
  app.query = "контракт"; app.selected = new Set(["manual"]);
  await openFromMenu("Логи");
  const search = [...document.querySelectorAll<HTMLElement>('[role="tab"]')].find(t => t.textContent?.includes("Поиск"))!;
  const logs = [...document.querySelectorAll<HTMLElement>('[role="tab"]')].find(t => t.textContent?.includes("Логи"))!;
  search.focus(); search.dispatchEvent(new KeyboardEvent("keydown", { key: "End", bubbles: true })); flushSync();
  expect(document.activeElement).toBe(logs);
  expect(document.querySelector('.workspace')?.getAttribute('data-active-panel')).toBe("logs");
  logs.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true })); flushSync();
  expect(document.activeElement).toBe(search);
  expect(document.querySelector('.workspace')?.getAttribute('data-active-panel')).toBe("search");
  expect(app.query).toBe("контракт"); expect([...app.selected]).toEqual(["manual"]);
});
it("перенос документной вкладки сохраняет тот же DOM, исходник и скрытую структуру", async () => {
  component = mount(Workspace, { target: document.body }); flushSync();
  app.viewer = { hit: hit("docs/a.md"), title: "А", text: "# А\n\nТекст", missing: false }; flushSync(); await settle();
  document.querySelector<HTMLButtonElement>('[aria-label="Скрыть структуру"]')!.click(); flushSync();
  const source = document.querySelector('.document .cm-editor');
  [...document.querySelectorAll<HTMLButtonElement>('button')].find(b => b.textContent === "Панели ▾")!.click(); flushSync();
  [...document.querySelectorAll<HTMLButtonElement>('button')].find(b => b.textContent === "Активную панель влево")!.click(); flushSync(); await settle();
  expect(document.querySelector('.document .cm-editor')).toBe(source);
  expect(document.querySelector('[title="Показать структуру"]')).not.toBeNull();
});
it("навигация и мини-граф — постоянная колонка, «Граф» открывает полный граф вкладкой", async () => {
  component = mount(Workspace, { target: document.body }); flushSync(); await settle();
  expect([...document.querySelectorAll('[role="tab"]')].some(t => t.textContent?.includes("Навигация"))).toBe(false);
  expect(document.querySelector('.dock aside')).toBeNull();
  expect(document.querySelector('.column aside')).not.toBeNull();
  expect(document.querySelector('.column .mini-graph')).not.toBeNull();
  document.querySelector<HTMLElement>('.column aside')!.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0 })); flushSync();
  [...document.querySelectorAll<HTMLButtonElement>('nav button')].find(b => b.textContent === "Граф")!.click(); flushSync(); await settle();
  expect(document.querySelector('.workspace')?.getAttribute('data-active-panel')).toBe("graph");
  expect(document.querySelector('.dock [data-panel-kind="graph"]')).not.toBeNull();
  expect(document.querySelector('.column .mini-graph')).not.toBeNull();
});
it("правая зона есть сразу; документ открывается в ней, после закрытия зона остаётся", async () => {
  component = mount(Workspace, { target: document.body }); flushSync(); await settle();
  const saved = () => JSON.parse(localStorage.getItem(layoutKey(project.path))!);
  app.viewer = { hit: hit("docs/a.md"), title: "А", text: "# А", missing: false }; flushSync(); await settle();
  const layout = saved().layout;
  const groupOf = (id: string) => (function find(node: any): string | undefined { return node.type === "leaf" ? (node.data.views.includes(id) ? node.data.id : undefined) : node.data.map(find).find(Boolean); })(layout.grid.root);
  expect(groupOf(documentId("docs/a.md"))).toBeDefined();
  expect(groupOf(documentId("docs/a.md"))).not.toBe(groupOf("search"));
  const tab = document.querySelector('[data-workspace-panel="document:docs/a.md"]')!.closest('[role="tab"]')!;
  tab.querySelector<HTMLElement>('.dv-default-tab-action')!.click(); flushSync(); await settle();
  expect(saved().layout.grid.root.data).toHaveLength(2);
});
it("иконка сброса очищает область и сохраняет фильтр документов", async () => {
  app.selected = new Set(["manual"]); app.filter = "руководство";
  component = mount(Workspace, { target: document.body }); flushSync(); await settle();
  document.querySelector<HTMLButtonElement>('[aria-label="Сбросить область поиска"]')!.click(); flushSync();
  expect(app.selected.size).toBe(0);
  expect(app.filter).toBe("руководство");
  expect(document.querySelector('.dock .statusbar')).toBeNull();
  expect(document.querySelector('.column .statusbar')).not.toBeNull();
});
