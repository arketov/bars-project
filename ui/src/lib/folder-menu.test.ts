import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import ContextMenu from "./ContextMenu.svelte";
import TreeRow from "./TreeRow.svelte";
import Sidebar from "./Sidebar.svelte";
import { app } from "./app.svelte";
import { buildTree } from "./tree";
import { folderItems } from "./contextmenu";

const api = vi.hoisted(() => ({ call: vi.fn(), folderTarget: vi.fn(), folderAction: vi.fn(), copyText: vi.fn(), documentAction: vi.fn(), documentEditors: vi.fn() }));
vi.mock("./api", () => ({ ...api, joinPath: (a: string, b: string) => `${a}/${b}`, openPath: vi.fn(), pickFolder: vi.fn() }));
const mounted: ReturnType<typeof mount>[] = [];
const project = { path: "C:/Проект", name: "Проект", docs_root: "manual/docs", state: "ready" as const, modules: [] };
function host() { const el = document.createElement("div"); document.body.append(el); return el; }
const settle = async () => { await new Promise(r => setTimeout(r, 0)); flushSync(); };
const button = (label: string) => [...document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].find(b => b.textContent?.startsWith(label))!;
async function open(el: Element) { el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 9999, clientY: 9999 })); await settle(); }
beforeEach(() => {
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
});

describe("Меню папок этапа 31", () => {
  it.each([".nm", "svg", "input", ".ct", ".row"])("правый клик %s закрепляет модуль без выбора/раскрытия", async selector => {
    const el = host(); mounted.push(mount(TreeRow, { target: el, props: { node: app.tree[0] } })); flushSync();
    await open(el.querySelector(selector)!);
    expect(api.folderTarget).toHaveBeenCalledWith(project.path, project.docs_root, "модуль");
    expect(app.selected.size).toBe(0); expect(app.current).toBe(project.path);
    expect(el.querySelector(".kids")).toBeNull();
    expect(button("Проверить индекс проекта").textContent).toContain("Проверяется весь проект");
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
    // Последний пункт — выбор цвета и иконки подпапки второго уровня (Т-38).
    expect(document.activeElement).toBe(button("Цвет и иконка…"));
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
    await open(el.firstElementChild!); button("Проверить индекс проекта").click(); await settle();
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
