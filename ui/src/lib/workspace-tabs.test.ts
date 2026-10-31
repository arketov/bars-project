// Шаг 3.3.5: меню вкладки документа в настоящем Dockview — закрытие других и вкладок справа.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import Workspace from "./Workspace.svelte";
import ContextMenu from "./ContextMenu.svelte";
import { app } from "./app.svelte";
import { call } from "./api";
import { keybindings } from "./keybindings.svelte";
import { tabHost } from "./workspace-tabs";

vi.mock("./api", async original => ({ ...await original<typeof import("./api")>(), call: vi.fn() }));
vi.mock("./graph-renderer", () => ({ GraphRenderer: class { set() {} destroy() {} } }));
const project = { path: "C:/А", name: "А", docs_root: "docs", state: "ready" as const, modules: [{ name: "manual", documents: 1 }] };
const hit = (path: string) => ({ path, title: path, first_line: 3, last_line: 4 });
const settle = async () => { await new Promise(resolve => setTimeout(resolve, 0)); flushSync(); };
const components: ReturnType<typeof mount>[] = [];
const documents = () => [...document.querySelectorAll<HTMLElement>('[role="tab"] [data-workspace-panel^="document:"]')].map(tab => tab.dataset.documentPath);
const items = () => [...document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')];
const name = (item: Element) => item.querySelector("span")!.childNodes[0].textContent;

beforeEach(async () => {
  keybindings.active = {}; keybindings.recording = false;
  localStorage.clear(); vi.mocked(call).mockReset();
  vi.mocked(call).mockImplementation(() => new Promise(() => {}));
  app.projects = [project]; app.current = project.path; app.view = "main"; app.viewer = null; app.tree = [];
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute("open", ""); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute("open"); };
  components.push(mount(Workspace, { target: document.body }));
  const menu = document.createElement("div"); document.body.append(menu);
  components.push(mount(ContextMenu, { target: menu })); flushSync(); await settle();
  for (const path of ["docs/a.md", "docs/b.md", "docs/c.md"]) { app.viewer = { hit: hit(path), title: path, text: "# " + path, missing: false }; flushSync(); await settle(); }
});
afterEach(async () => {
  for (const component of components.splice(0)) await unmount(component);
  app.viewer = null; app.current = null; app.projects = []; document.body.innerHTML = "";
});

async function menuOf(path: string) {
  const tab = document.querySelector<HTMLElement>(`[data-workspace-panel^="document:"][data-document-path="${path}"]`)!;
  tab.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 50, clientY: 50 })); await settle();
}

it("меню вкладки: группа вкладки сверху, состояние берётся из группы Dockview", async () => {
  expect(documents()).toEqual(["docs/a.md", "docs/b.md", "docs/c.md"]);
  await menuOf("docs/b.md");
  expect(items().map(name).slice(0, 4)).toEqual(["Закрыть", "Закрыть другие", "Закрыть справа", "Переместить вкладку…"]);
  expect(items().find(item => name(item) === "Закрыть справа")!.getAttribute("aria-disabled")).toBeNull();
  expect(items().map(name)).not.toContain("Открыть");
  expect(tabHost()!.state(tabHost()!.panelAt(document.querySelector<HTMLElement>('[data-document-path="docs/c.md"]')!)!)).toMatchObject({ others: 2, right: 0 });
});

it("«Закрыть справа» закрывает только вкладки справа, «Закрыть другие» — все остальные в группе", async () => {
  await menuOf("docs/b.md");
  items().find(item => name(item) === "Закрыть справа")!.click(); await settle(); await settle();
  expect(documents()).toEqual(["docs/a.md", "docs/b.md"]);
  await menuOf("docs/a.md");
  items().find(item => name(item) === "Закрыть другие")!.click(); await settle(); await settle();
  expect(documents()).toEqual(["docs/a.md"]);
  await menuOf("docs/a.md");
  for (const text of ["Закрыть другие", "Закрыть справа"]) expect(items().find(item => name(item) === text)!.getAttribute("aria-disabled")).toBe("true");
});

it("«Закрыть» на неактивной вкладке закрывает именно её", async () => {
  await menuOf("docs/a.md");
  items().find(item => name(item) === "Закрыть")!.click(); await settle(); await settle();
  expect(documents()).toEqual(["docs/b.md", "docs/c.md"]);
});

it("шапка и тело документа определяют свою вкладку, а не активную", async () => {
  const host = tabHost()!;
  for (const path of ["docs/a.md", "docs/b.md", "docs/c.md"]) {
    const section = document.querySelector<HTMLElement>(`section.document[data-document-path="${path}"]`)!;
    expect(host.panelAt(section.querySelector("header")!)).toBe(`document:${path}`);
    expect(host.panelAt(section.querySelector(".body")!)).toBe(`document:${path}`);
  }
});

it("меню шапки неактивного документа закрывает его вкладку", async () => {
  const header = document.querySelector<HTMLElement>('section.document[data-document-path="docs/a.md"] header')!;
  header.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 50, clientY: 50 })); await settle();
  expect(items().map(name).slice(0, 4)).toEqual(["Закрыть", "Закрыть другие", "Закрыть справа", "Переместить вкладку…"]);
  items().find(item => name(item) === "Закрыть")!.click(); await settle(); await settle();
  expect(documents()).toEqual(["docs/b.md", "docs/c.md"]);
});
