import { afterEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import { app } from "./app.svelte";
import TreeRow from "./TreeRow.svelte";
import ContextMenu from "./ContextMenu.svelte";
import { buildTree } from "./tree";
import { documentAt } from "./contextmenu";

const api = vi.hoisted(() => ({ call: vi.fn(), documentAction: vi.fn(), documentEditors: vi.fn(), copyText: vi.fn() }));
vi.mock("./api", () => ({ ...api, joinPath: (a: string, b: string) => `${a}/${b}`, openPath: vi.fn(), pickFolder: vi.fn(), inTauri: () => false }));

const components: ReturnType<typeof mount>[] = [];
const project = { path: "C:/проект с пробелами", name: "Проект", docs_root: "docs", state: "ready" as const, modules: [] };
const reference = (path: string) => ({ path, title: path, first_line: 0, last_line: 0 });
const settle = async () => { await new Promise(resolve => setTimeout(resolve, 0)); flushSync(); };
function host() { const element = document.createElement("div"); document.body.append(element); return element; }
function setup() { app.projects = [project]; app.current = project.path; api.call.mockResolvedValue({ title: "Документ", text: "# Документ" }); }

afterEach(async () => {
  for (const component of components.splice(0)) await unmount(component);
  app.closeViewer(); app.current = null; app.projects = []; app.selected = new Set();
  document.body.innerHTML = ""; vi.clearAllMocks();
});

describe("Документы этапа 18", () => {
  it.each(["docs", "guide/manual", "."])("сохраняет исходный путь дерева при docs_root=%s", root => {
    const path = root === "." ? "Index.md" : `${root}/Index.md`;
    const tree = buildTree([{ path, module: ".", title: "Обзор", chunks: 0 }], root);
    expect(tree[0].children[0].documentPath).toBe(path);
  });

  it("открывает файл по имени, не меняя галочки; галочка не открывает файл", async () => {
    setup();
    const tree = buildTree([{ path: "docs/Index.md", module: ".", title: "Обзор", chunks: 1 }], "docs");
    app.tree = tree;
    const element = host();
    components.push(mount(TreeRow, { target: element, props: { node: tree[0].children[0] } })); flushSync();
    element.querySelector<HTMLButtonElement>(".filename")!.click(); await settle();
    expect(api.call).toHaveBeenCalledWith("read", expect.objectContaining({ path: "./docs/Index.md" }));
    expect(app.selected.size).toBe(0);
    expect(app.viewer?.hit.first_line).toBe(0);
    api.call.mockClear();
    element.querySelector<HTMLInputElement>("input")!.click(); await settle();
    expect(app.selected.size).toBe(1);
    expect(api.call).not.toHaveBeenCalled();
  });

  it("игнорирует поздние ответы после нового открытия, закрытия и смены проекта", async () => {
    setup();
    let resolve!: (value: unknown) => void;
    api.call.mockImplementationOnce(() => new Promise(done => { resolve = done; }));
    const old = app.openDocument(reference("docs/old.md"));
    await app.openDocument(reference("docs/new.md"));
    resolve({ title: "Старый", text: "old" }); await old;
    expect(app.viewer?.hit.path).toBe("docs/new.md");
    for (const cancel of [() => app.closeViewer(), () => { app.current = "другой"; }]) {
      app.current = project.path;
      api.call.mockImplementationOnce(() => new Promise(done => { resolve = done; }));
      const reading = app.openDocument(reference("docs/late.md")); cancel();
      resolve({ title: "Поздний", text: "late" }); await reading;
      expect(app.viewer?.title).not.toBe("Поздний");
    }
  });

  it("открывает меню вложенной карточки, подменю редакторов и копирует абсолютный путь", async () => {
    setup(); api.documentEditors.mockResolvedValue([{ id: "code", label: "VS Code" }]);
    api.documentAction.mockResolvedValue("C:/проект с пробелами/docs/Тема.md");
    const element = host(); element.innerHTML = '<div data-document-path="docs/Тема.md"><span>Документ</span></div>';
    components.push(mount(ContextMenu, { target: host() })); flushSync();
    const open = async () => { element.querySelector("span")!.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 100, clientY: 100 })); await settle(); };
    expect(documentAt(element.querySelector("span"))).toEqual({ path: "docs/Тема.md", line: 0 });
    await open();
    const button = (text: string) => Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')).find(button => button.textContent?.includes(text))!;
    button("Открыть в …").click(); await settle();
    expect(button("Программа по умолчанию")).toBeTruthy();
    button("VS Code").click(); await settle();
    expect(api.documentAction).toHaveBeenLastCalledWith(project.path, "docs/Тема.md", "open", "code");
    await open(); button("Скопировать путь").click(); await settle();
    expect(api.copyText).toHaveBeenCalledWith("C:/проект с пробелами/docs/Тема.md");
    await open(); button("Проверить индекс").click(); await settle();
    expect(api.documentAction).toHaveBeenLastCalledWith(project.path, "docs/Тема.md", "check", undefined);
    await open(); window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })); await settle();
    expect(document.querySelector('[role="menu"]')).toBeNull();
    await open(); app.current = "другой"; await settle();
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });

  it("ошибка буфера не подтверждает копирование", async () => {
    setup(); api.documentAction.mockResolvedValue("C:/p/docs/a.md"); api.copyText.mockRejectedValueOnce(new Error("Буфер недоступен"));
    const element = host(); element.dataset.documentPath = "docs/a.md";
    components.push(mount(ContextMenu, { target: host() })); flushSync();
    element.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true })); await settle();
    Array.from(document.querySelectorAll<HTMLButtonElement>("button")).find(button => button.textContent === "Скопировать путь")!.click(); await settle();
    expect(app.toast).toEqual({ text: "Буфер недоступен", bad: true });
  });
});
