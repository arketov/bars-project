import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import ContextMenu from "./ContextMenu.svelte";
import TreeRow from "./TreeRow.svelte";
import Sidebar from "./Sidebar.svelte";
import { app } from "./app.svelte";
import { buildTree } from "./tree";
import { folderItems } from "./contextmenu";
import { keybindings } from './keybindings.svelte';
import { entries } from './entries.svelte';
import { adapterFor } from './command-adapters';

const api = vi.hoisted(() => ({ call: vi.fn(), folderTarget: vi.fn(), folderAction: vi.fn(), copyText: vi.fn(), documentAction: vi.fn(), documentEditors: vi.fn() }));
vi.mock("./api", () => ({ ...api, joinPath: (a: string, b: string) => `${a}/${b}`, openPath: vi.fn(), pickFolder: vi.fn(), inTauri: () => false }));
const mounted: ReturnType<typeof mount>[] = [];
const project = { path: "C:/Проект", name: "Проект", docs_root: "manual/docs", state: "ready" as const, modules: [] };
function host() { const el = document.createElement("div"); document.body.append(el); return el; }
const settle = async () => { await new Promise(r => setTimeout(r, 0)); flushSync(); };
const button = (label: string) => [...document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].find(b => b.textContent?.startsWith(label))!;
async function open(el: Element) { el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 9999, clientY: 9999 })); await settle(); }
beforeEach(() => {
  app.workerDown = null; keybindings.active = {};
  app.current = project.path; app.projects = [project]; app.docsRoot = project.docs_root;
  app.tree = buildTree([{ path: "manual/docs/модуль/вложенная/Документ.md", module: "модуль", title: "Документ", chunks: 1 }], project.docs_root);
  app.selected = new Set();
  api.folderTarget.mockResolvedValue({ id: "42", absolute: "C:/Проект/manual/docs/модуль", missing: false });
  api.folderAction.mockResolvedValue({ absolute: "C:/Проект/manual/docs/модуль", missing: false, operation_id: "7" });
  mounted.push(mount(ContextMenu, { target: host() }));
  flushSync();
});
afterEach(async () => {
  for (const c of mounted.splice(0)) await unmount(c);
  app.current = null; app.projects = []; app.tree = []; app.toast = null; app.selected = new Set();
  document.body.innerHTML = ""; vi.clearAllMocks();
  entries.cancel(); keybindings.active = {}; app.workerDown = null;
});

describe("Меню папок этапа 31", () => {
  it('группы с разделителями, удаление последним; меню и команда реестра открывают один диалог', async () => {
    const el = host(); mounted.push(mount(TreeRow, { target: el, props: {node:app.tree[0]} })); flushSync();
    const row = el.querySelector<HTMLElement>('.row')!;
    await open(row);
    const menu = document.querySelector('.primary')!;
    expect([...menu.querySelectorAll('[role="menuitem"]')].map(b => b.textContent?.trim())).toEqual([
      'Новый документ…','Новая папка…','Переименовать…F2','Переместить…','Показать в Проводнике',
      'Скопировать путь','Цвет и иконка…','Проверить проектСверит документы с поиском и обновит изменившиеся','Удалить…Delete',
    ]);
    expect(menu.querySelectorAll('[role="separator"]')).toHaveLength(5);
    expect(menu.firstElementChild?.getAttribute('role')).toBe('menuitem');
    expect(menu.lastElementChild?.classList.contains('danger')).toBe(true);
    button('Переименовать…').click(); await settle();
    expect(entries.request?.node.path).toBe('модуль'); expect(entries.request?.action).toBe('rename');
    entries.cancel();
    await adapterFor('target.rename',row)!.run();
    expect(entries.request?.node.path).toBe('модуль'); expect(entries.request?.action).toBe('rename');
  });
  it('недоступная проверка в обходе стрелками; Enter/клик ничего не запускает, причина читаема', async () => {
    app.projects = [{...project,state:'unavailable'}];
    const el = host(); mounted.push(mount(TreeRow, {target:el,props:{node:app.tree[0]}})); flushSync();
    await open(el.querySelector('.row')!);
    const check = button('Проверить проект');
    expect(check.getAttribute('aria-disabled')).toBe('true'); expect(check.title).toContain('недоступен');
    expect(document.getElementById(check.getAttribute('aria-describedby')!)?.textContent).toContain('недоступен');
    window.dispatchEvent(new KeyboardEvent('keydown',{key:'End',cancelable:true}));
    window.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowUp',cancelable:true})); flushSync();
    expect(document.activeElement).toBe(check); expect(document.querySelector('.reason')?.textContent).toContain('недоступен');
    check.click(); await settle(); expect(api.folderAction).not.toHaveBeenCalled();
    expect(document.querySelector('.cmenu')).not.toBeNull();
  });
  it('при потере воркера меню папки показывает причины без IPC; назначения перечитываются при открытии', async () => {
    const el = host(); mounted.push(mount(TreeRow, {target:el,props:{node:app.tree[0]}})); flushSync();
    const row = el.querySelector<HTMLElement>('.row')!;
    app.workerDown = 'Нет соединения'; keybindings.active = {'target.rename':{code:'F3',modifiers:[]}};
    await open(row);
    expect(api.folderTarget).not.toHaveBeenCalled();
    expect(button('Переименовать…').getAttribute('aria-disabled')).toBe('true');
    expect(button('Переименовать…').textContent).toContain('F3');
    button('Удалить…').click(); expect(entries.request).toBeNull();
    app.current = 'C:/другой'; flushSync(); expect(document.querySelector('.cmenu')).toBeNull();
    app.current = project.path;
    window.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape'})); await settle();
    app.workerDown = null; keybindings.active = {'target.rename':null}; await open(row);
    expect(button('Переименовать…').querySelector('.shortcut')?.textContent).toBe('');
  });
  it('стрелка вправо и клик на сером выборе программы не вызывают IPC', async () => {
    const el = host(); mounted.push(mount(TreeRow, {target:el,props:{node:app.tree[0],forceOpen:true}})); flushSync();
    app.workerDown = 'Нет соединения';
    await open(el.querySelector('[data-menu-kind="file"]')!);
    const programs = button('Открыть в …'); programs.focus();
    window.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowRight',cancelable:true})); programs.click(); await settle();
    expect(programs.getAttribute('aria-disabled')).toBe('true');
    expect(api.documentEditors).not.toHaveBeenCalled(); expect(document.querySelector('.sub')).toBeNull();
  });
  it.each([".nm", "svg", "input", ".ct", ".row"])("правый клик %s закрепляет модуль без выбора/раскрытия", async selector => {
    const el = host(); mounted.push(mount(TreeRow, { target: el, props: { node: app.tree[0] } })); flushSync();
    await open(el.querySelector(selector)!);
    expect(api.folderTarget).toHaveBeenCalledWith(project.path, project.docs_root, "модуль");
    expect(app.selected.size).toBe(0); expect(app.current).toBe(project.path);
    expect(el.querySelector(".kids")).toBeNull();
    expect(button("Проверить проект").textContent).toContain("Сверит документы с поиском и обновит изменившиеся");
    expect(button("Открыть в …")).toBeUndefined();
    // Т-33: над папкой доступны создание, переименование, перенос и удаление.
    for (const label of ["Новый документ…", "Новая папка…", "Переименовать…", "Переместить…", "Удалить…"]) expect(button(label)).toBeDefined();
    expect(document.querySelector<HTMLElement>(".cmenu")!.style.left).not.toBe("9999px");
  });
  it("вложенная папка, клавиатура, Escape возвращает фокус строки", async () => {
    const el = host(); mounted.push(mount(TreeRow, { target: el, props: { node: app.tree[0], forceOpen: true } })); flushSync();
    const row = el.querySelector<HTMLElement>('[data-folder-path="модуль/вложенная"]')!; row.focus();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "F10", shiftKey: true, cancelable: true })); await settle();
    expect(api.folderTarget).toHaveBeenCalledWith(project.path, project.docs_root, "модуль/вложенная");
    expect(document.activeElement).toBe(button("Новый документ…"));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "End", cancelable: true }));
    // Опасная группа последняя, после оформления и проверки индекса (ISS-57).
    expect(document.activeElement).toBe(button("Удалить…"));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })); flushSync();
    expect(document.activeElement).toBe(row); expect(document.querySelector(".cmenu")).toBeNull();
  });
  it.each(["docs", "manual/docs", "."])("меню корня docs_root %s открывает группа «Корневые документы»; отдельной строки корня нет", async docsRoot => {
    app.docsRoot = docsRoot;
    app.tree = buildTree([{ path: docsRoot === "." ? "Index.md" : `${docsRoot}/Index.md`, module: ".", title: "Index", chunks: 2 }], docsRoot);
    const el = host(); mounted.push(mount(Sidebar, { target: el })); flushSync();
    expect(el.querySelector(".docs-root")).toBeNull();
    const group = el.querySelector<HTMLElement>('[data-folder-path="."]')!;
    expect(group.dataset.folderKind).toBe("Корневые документы"); await open(group);
    expect(api.folderTarget).toHaveBeenCalledWith(project.path, docsRoot, ".");
  });
  it("пустой индекс: в дереве нет ни одной цели меню папки", () => {
    app.tree = [];
    const el = host(); mounted.push(mount(Sidebar, { target: el })); flushSync();
    expect(el.querySelector("[data-folder-path]")).toBeNull();
  });
  it("копирует подтверждённый путь; проверяет весь закреплённый проект с ID очереди", async () => {
    const el = host(); el.innerHTML = '<div data-folder-path="." data-folder-kind="Корневые документы"></div>';
    await open(el.firstElementChild!); button("Скопировать путь").click(); await settle();
    expect(api.folderAction).toHaveBeenCalledWith("42", "path"); expect(api.copyText).toHaveBeenCalledWith("C:/Проект/manual/docs/модуль");
    await open(el.firstElementChild!); button("Проверить проект").click(); await settle();
    expect(api.folderAction).toHaveBeenCalledWith("42", "check"); expect(app.toast?.text).toBe("Проверка проекта в очереди · №7");
  });
  it("ошибка буфера содержит исходную цель и не даёт успеха", async () => {
    api.copyText.mockRejectedValueOnce(new Error("Буфер недоступен"));
    const el = host(); el.innerHTML = '<div data-folder-path="."></div>';
    await open(el.firstElementChild!); button("Скопировать путь").click(); await settle();
    expect(app.toast?.bad).toBe(true); expect(app.toast?.text).toContain("C:/Проект/manual/docs/модуль — Буфер недоступен");
  });
  it.each(["Escape", "Tab", "blur", "outside", "project", "project-roundtrip"])("позднее закрепление после %s не открывает меню", async cancel => {
    let resolve!: (v: unknown) => void;
    api.folderTarget.mockImplementationOnce(() => new Promise(r => { resolve = r; }));
    const el = host(); el.innerHTML = '<div data-folder-path="."></div>';
    await open(el.firstElementChild!);
    if (cancel === "blur") window.dispatchEvent(new Event("blur"));
    else if (cancel === "outside") document.body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    else if (cancel.startsWith("project")) {
      app.current = "C:/другой"; flushSync();
      if (cancel === "project-roundtrip") { app.current = project.path; flushSync(); }
    }
    else window.dispatchEvent(new KeyboardEvent("keydown", { key: cancel }));
    resolve({ id: "late", absolute: "late", missing: false }); await settle();
    expect(document.querySelector(".cmenu")).toBeNull();
  });
  it("смена docs_root закрывает меню; особые состояния блокируют только проверку", async () => {
    const el = host(); el.innerHTML = '<div data-folder-path="."></div>';
    await open(el.firstElementChild!); app.docsRoot = "."; flushSync();
    expect(document.querySelector(".cmenu")).toBeNull();
    expect(folderItems("reindex_required")[2].disabled).toBe(true);
    expect(folderItems("unavailable")[2].hint).toBe("Проект недоступен");
    expect(folderItems("ready", true)[0].disabled).toBe(true);
    expect(folderItems("ready", true)[1].disabled).not.toBe(true);
  });
});
