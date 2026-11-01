// Шаг 3.3.5: меню документа на карточке, вкладке, в шапке и в редакторе по общей модели.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import { EditorView } from "@codemirror/view";
import { app } from "./app.svelte";
import ContextMenu from "./ContextMenu.svelte";
import Editor from "./Editor.svelte";
import { entries } from "./entries.svelte";
import { buildTree } from "./tree";
import { registerTabHost, type TabHost } from "./workspace-tabs";

const api = vi.hoisted(() => ({ call: vi.fn(), documentAction: vi.fn(), documentEditors: vi.fn(), copyText: vi.fn() }));
vi.mock("./api", () => ({ ...api, joinPath: (a: string, b: string) => `${a}/${b}`, openPath: vi.fn(), pickFolder: vi.fn(), inTauri: () => false }));

const components: ReturnType<typeof mount>[] = [];
const project = { path: "C:/проект", name: "Проект", docs_root: "docs", state: "ready" as const, modules: [] };
const settle = async () => { await new Promise(resolve => setTimeout(resolve, 0)); flushSync(); };
const clipboard = { writeText: vi.fn(async () => {}), readText: vi.fn(async () => "вставка") };
let unregister: (() => void) | undefined;

function host() { const element = document.createElement("div"); document.body.append(element); return element; }
async function rightClick(element: Element) {
  element.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 100, clientY: 100 })); await settle();
}
const items = () => Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'));
const name = (item: Element) => item.querySelector("span")!.childNodes[0].textContent;
const button = (text: string) => items().find(item => name(item)?.includes(text))!;
const exact = (text: string) => items().find(item => name(item) === text)!;
const labels = () => items().map(name);
const tabHost = (state = { others: 1, right: 0, canMove: true }) => {
  const run = vi.fn(async () => {});
  const stub: TabHost = { panelAt: () => "document:docs/a.md", state: () => state, run };
  unregister = registerTabHost(stub);
  return run;
};

beforeEach(() => {
  Object.defineProperty(navigator, "clipboard", { value: clipboard, configurable: true });
  app.projects = [project]; app.current = project.path;
  app.tree = buildTree([{ path: "docs/a.md", module: ".", title: "А", chunks: 1 }], "docs");
  api.documentEditors.mockResolvedValue([{ id: "code", label: "VS Code" }]);
});
afterEach(async () => {
  for (const component of components.splice(0)) await unmount(component);
  unregister?.(); unregister = undefined; entries.cancel();
  app.closeViewer(); app.current = null; app.projects = []; app.tree = [];
  document.body.innerHTML = ""; vi.clearAllMocks();
});

describe("Меню документа на карточке, вкладке и в шапке", () => {
  it("карточка: группы по порядку, «Открыть» есть, вкладки нет, удаление последним и красным", async () => {
    const card = host(); card.dataset.documentPath = "docs/a.md"; card.dataset.menuKind = "file"; card.dataset.menuPlace = "card";
    components.push(mount(ContextMenu, { target: host() })); flushSync();
    await rightClick(card);
    expect(labels()).toEqual(["Открыть", "Открыть в новом окне", "Открыть в …", "Переименовать…", "Переместить…", "Показать в Проводнике", "Скопировать путь", "Проверить документ", "Удалить…"]);
    expect(document.querySelectorAll('[role="separator"]')).toHaveLength(4);
    expect(items().at(-1)!.classList.contains("danger")).toBe(true);
    expect(document.querySelector('[aria-haspopup="menu"]')?.textContent).toContain("Открыть в …");
  });

  it("«Переименовать», «Переместить», «Удалить» с карточки открывают тот же диалог, что и в дереве", async () => {
    const card = host(); card.dataset.documentPath = "docs/a.md";
    components.push(mount(ContextMenu, { target: host() })); flushSync();
    for (const [text, action] of [["Переименовать", "rename"], ["Переместить", "move"], ["Удалить", "trash"]] as const) {
      await rightClick(card); button(text).click(); await settle();
      expect(entries.request).toMatchObject({ action, node: { documentPath: "docs/a.md" } });
      entries.cancel();
    }
  });

  it("документ вне дерева: изменение серое с причиной, клик ничего не открывает", async () => {
    const card = host(); card.dataset.documentPath = "docs/нет.md";
    components.push(mount(ContextMenu, { target: host() })); flushSync();
    await rightClick(card);
    expect(button("Переименовать").getAttribute("aria-disabled")).toBe("true");
    button("Переименовать").focus(); await settle();
    expect(document.querySelector(".reason")?.textContent).toContain("дереве");
    button("Переименовать").click(); await settle();
    expect(entries.request).toBeNull();
  });

  it("шапка: группа вкладки сверху, «Закрыть справа» серое, «Закрыть» идёт в хозяин вкладок", async () => {
    const run = tabHost();
    const header = host(); header.dataset.documentPath = "docs/a.md"; header.dataset.menuKind = "file"; header.dataset.menuPlace = "header";
    components.push(mount(ContextMenu, { target: host() })); flushSync();
    await rightClick(header);
    expect(labels().slice(0, 4)).toEqual(["Закрыть", "Закрыть другие", "Закрыть справа", "Переместить вкладку…"]);
    expect(labels()).not.toContain("Открыть");
    expect(button("Закрыть справа").getAttribute("aria-disabled")).toBe("true");
    button("Закрыть справа").click(); await settle();
    expect(run).not.toHaveBeenCalled();
    await rightClick(header); button("Закрыть другие").click(); await settle();
    expect(run).toHaveBeenLastCalledWith("tab.closeOthers", "document:docs/a.md");
    await rightClick(header); exact("Закрыть").click(); await settle();
    expect(run).toHaveBeenLastCalledWith("tab.close", "document:docs/a.md");
  });

  it("без вкладки (отдельное окно) шапка не показывает группу вкладки", async () => {
    const header = host(); header.dataset.documentPath = "docs/a.md"; header.dataset.menuKind = "file"; header.dataset.menuPlace = "header";
    components.push(mount(ContextMenu, { target: host() })); flushSync();
    await rightClick(header);
    expect(labels()[0]).toBe("Открыть в новом окне");
  });

  it("вложенный объект со своим видом не наследует меню документа вкладки", async () => {
    tabHost();
    const header = host(); header.dataset.documentPath = "docs/a.md"; header.dataset.menuKind = "file"; header.dataset.menuPlace = "header";
    header.innerHTML = '<a data-menu-kind="link"><span>ссылка</span></a>';
    components.push(mount(ContextMenu, { target: host() })); flushSync();
    await rightClick(header.querySelector("span")!);
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });

  it("ошибка команды вкладки показывается уведомлением, а не успехом", async () => {
    const run = tabHost(); run.mockRejectedValueOnce(new Error("Вкладка уже закрыта"));
    const header = host(); header.dataset.documentPath = "docs/a.md"; header.dataset.menuKind = "file"; header.dataset.menuPlace = "tab";
    components.push(mount(ContextMenu, { target: host() })); flushSync();
    await rightClick(header); exact("Закрыть").click(); await settle();
    expect(app.toast).toEqual({ text: "Вкладка уже закрыта", bad: true });
  });

  it("смена проекта закрывает меню документа", async () => {
    const card = host(); card.dataset.documentPath = "docs/a.md";
    components.push(mount(ContextMenu, { target: host() })); flushSync();
    await rightClick(card); app.current = "другой"; await settle();
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });
});

describe("Меню редактора: правка текста и подменю «Документ»", () => {
  let text = "";
  function editorPage(source = "один два") {
    text = source;
    const body = host(); body.dataset.documentPath = "docs/a.md"; body.dataset.menuKind = "editor"; body.dataset.menuPlace = "editor";
    components.push(mount(Editor, { target: body, props: { text: source, oninput: (value: string) => { text = value; } } }));
    components.push(mount(ContextMenu, { target: host() })); flushSync();
    return { body, view: EditorView.findFromDOM(body.querySelector(".cm-editor")!)! };
  }

  it("сначала шесть команд правки, ниже «Документ ▸»; без выделения вырезать и копировать серые", async () => {
    tabHost();
    const { body } = editorPage();
    await rightClick(body.querySelector(".cm-content")!);
    expect(labels()).toEqual(["Отменить", "Повторить", "Вырезать", "Копировать", "Вставить", "Выделить всё", "Документ"]);
    for (const name of ["Отменить", "Повторить", "Вырезать", "Копировать"]) expect(button(name).getAttribute("aria-disabled")).toBe("true");
    expect(button("Вставить").getAttribute("aria-disabled")).toBeNull();
    expect(button("Вырезать").textContent).toContain("Ctrl+X");
    expect(button("Документ").getAttribute("aria-haspopup")).toBe("menu");
  });

  it("«Выделить всё», затем «Копировать» и «Вырезать» работают над текстом редактора", async () => {
    const { body, view } = editorPage();
    await rightClick(body.querySelector(".cm-content")!); button("Выделить всё").click(); await settle();
    expect(view.state.selection.main.to).toBe(view.state.doc.length);
    await rightClick(body.querySelector(".cm-content")!); button("Копировать").click(); await settle();
    expect(clipboard.writeText).toHaveBeenLastCalledWith("один два");
    await rightClick(body.querySelector(".cm-content")!); button("Вырезать").click(); await settle();
    expect(clipboard.writeText).toHaveBeenLastCalledWith("один два");
    expect(view.state.doc.toString()).toBe(""); expect(text).toBe("");
    await rightClick(body.querySelector(".cm-content")!);
    expect(button("Отменить").getAttribute("aria-disabled")).toBeNull();
    button("Отменить").click(); await settle();
    expect(view.state.doc.toString()).toBe("один два");
  });

  it("«Вставить» кладёт текст буфера на место выделения; отказ буфера — уведомление", async () => {
    const { body, view } = editorPage("abc");
    view.dispatch({ selection: { anchor: 0, head: 3 } });
    await rightClick(body.querySelector(".cm-content")!); button("Вставить").click(); await settle();
    expect(view.state.doc.toString()).toBe("вставка");
    clipboard.readText.mockRejectedValueOnce(new Error("нет доступа"));
    await rightClick(body.querySelector(".cm-content")!); button("Вставить").click(); await settle();
    expect(app.toast?.bad).toBe(true);
    expect(view.state.doc.toString()).toBe("вставка");
  });

  it("«Документ ▸» открывается стрелкой вправо, несёт группы вкладки и редакторы плоско, Escape возвращает на родителя", async () => {
    const run = tabHost();
    const { body } = editorPage();
    await rightClick(body.querySelector(".cm-content")!);
    button("Документ").focus();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight" })); await settle();
    const sub = document.querySelector(".sub")!;
    const names = Array.from(sub.querySelectorAll('[role="menuitem"]')).map(name);
    expect(names).toEqual(["Закрыть", "Закрыть другие", "Закрыть справа", "Переместить вкладку…", "Открыть в новом окне",
      "Открыть в программе по умолчанию", "Открыть в VS Code", "Переименовать…", "Переместить…", "Показать в Проводнике", "Скопировать путь", "Проверить документ", "Удалить…"]);
    expect(sub.querySelector('[aria-haspopup]')).toBeNull();
    expect(document.activeElement).toBe(sub.querySelector("button"));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })); await settle();
    expect(document.querySelector(".sub")).toBeNull();
    expect(document.activeElement).toBe(button("Документ"));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight" })); await settle();
    (Array.from(document.querySelectorAll<HTMLButtonElement>(".sub [role=\"menuitem\"]")).find(item => name(item) === "Закрыть"))!.click(); await settle();
    expect(run).toHaveBeenLastCalledWith("tab.close", "document:docs/a.md");
  });

  it("программа из «Документ ▸» открывает именно этот документ", async () => {
    const { body } = editorPage();
    await rightClick(body.querySelector(".cm-content")!);
    button("Документ").click(); await settle();
    button("Открыть в VS Code").click(); await settle();
    expect(api.documentAction).toHaveBeenLastCalledWith(project.path, "docs/a.md", "open", "code");
  });

  it("документ только для чтения: изменяющая правка серая, копирование доступно после выделения", async () => {
    const body = host(); body.dataset.documentPath = "docs/a.md"; body.dataset.menuKind = "editor"; body.dataset.menuPlace = "editor";
    components.push(mount(Editor, { target: body, props: { text: "abc", readonly: true, oninput: () => {} } }));
    components.push(mount(ContextMenu, { target: host() })); flushSync();
    const view = EditorView.findFromDOM(body.querySelector(".cm-editor")!)!;
    view.dispatch({ selection: { anchor: 0, head: 3 } });
    await rightClick(body.querySelector(".cm-content")!);
    for (const name of ["Отменить", "Повторить", "Вырезать", "Вставить"]) expect(button(name).getAttribute("aria-disabled")).toBe("true");
    expect(button("Копировать").getAttribute("aria-disabled")).toBeNull();
  });
});
