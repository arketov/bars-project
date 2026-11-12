import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import Sidebar from "./Sidebar.svelte";
import { app } from "./app.svelte";
import { matchProject } from "./projects";

const api = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("./api", () => ({ ...api, joinPath: vi.fn(), openPath: vi.fn(), pickFolder: vi.fn(), copyText: vi.fn(), documentAction: vi.fn(), documentEditors: vi.fn(), inTauri: () => false }));

const make = (name: string, path = `C:/work/${name}`) => ({ name, path, docs_root: "docs", state: "ready" as const, modules: [] });
const components: ReturnType<typeof mount>[] = [];
const settle = async () => { await new Promise(resolve => setTimeout(resolve, 0)); flushSync(); };
const scrolled: Element[] = [];
const menu = () => document.querySelector<HTMLElement>(".projmenu");
const rows = () => Array.from(document.querySelectorAll<HTMLElement>(".projmenu .pitem"));
const field = () => document.querySelector<HTMLInputElement>(".projmenu .pfilter input")!;
async function openMenu() { document.querySelector<HTMLButtonElement>("button.proj")!.click(); await settle(); }
function key(target: Element, name: string) { target.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true })); flushSync(); }
async function type(value: string) { const input = field(); input.value = value; input.dispatchEvent(new Event("input", { bubbles: true })); await settle(); }

beforeEach(() => {
  Element.prototype.scrollIntoView = function (this: Element) { scrolled.push(this); };
  app.projects = Array.from({ length: 40 }, (_, i) => make(`project-${String(i + 1).padStart(2, "0")}`));
  app.projects.push(make("web-portal", "C:\\Work\\Веб-Портал"));
  app.current = app.projects[40].path; app.tree = []; app.activity = null; app.filter = "";
  api.call.mockResolvedValue({ accepted: true });
  components.push(mount(Sidebar, { target: document.body })); flushSync();
});
afterEach(async () => {
  for (const component of components.splice(0)) await unmount(component);
  document.body.innerHTML = ""; app.current = null; app.projects = []; scrolled.length = 0;
  vi.clearAllMocks();
});

it("matchProject: имя, составное имя, путь, регистр, слеши, кириллица", () => {
  const p = { name: "Api", path: "C:\\Work\\Бэкенд\\api", qualified_name: "tmc-opovechenie/backend/api" };
  expect(matchProject(p, "")).toBe(true);
  expect(matchProject(p, "   ")).toBe(true);
  expect(matchProject(p, "API")).toBe(true);
  expect(matchProject(p, "opovechenie/BACKEND")).toBe(true);
  expect(matchProject(p, "work/бэкенд")).toBe(true);
  expect(matchProject(p, "work\\БЭКЕНД")).toBe(true);
  expect(matchProject({ name: "x", path: "C:/x" }, "backend")).toBe(false);
  expect(matchProject({ name: "x", path: "C:/x", qualified_name: null }, "x")).toBe(true);
});

it("40 проектов: у меню предел высоты от --projmenu-top, список прокручивается, «Добавить проект…» вне списка, выбранный прокручен", async () => {
  await openMenu();
  const style = menu()!.getAttribute("style") ?? "";
  expect(style).toContain("--projmenu-top");
  expect(style).toContain("max-height");
  expect(style).toContain("var(--projmenu-top)");
  const scroll = menu()!.querySelector(".pscroll")!;
  expect(rows()).toHaveLength(41);
  expect(scroll.contains(rows()[40])).toBe(true);
  const add = menu()!.querySelector(".padd")!;
  expect(add.textContent).toContain("Добавить проект");
  expect(scroll.contains(add)).toBe(false);
  expect(scroll.contains(menu()!.querySelector(".pfilter"))).toBe(false);
  expect(scrolled).toContain(rows()[40]);
});

it("resize пересчитывает --projmenu-top", async () => {
  await openMenu();
  const button = document.querySelector<HTMLButtonElement>("button.proj")!;
  button.getBoundingClientRect = () => ({ bottom: 100 } as DOMRect);
  window.dispatchEvent(new Event("resize")); await settle();
  expect(menu()!.getAttribute("style")).toContain("106px");
});

it("поле фильтра: фокус при открытии, совпадение по имени и пути, «Ничего не найдено», сброс при закрытии", async () => {
  await openMenu();
  expect(document.activeElement).toBe(field());
  await type("веб");
  expect(rows().map(r => r.dataset.projectPath)).toEqual(["C:\\Work\\Веб-Портал"]);
  await type("c:/work/project-0");
  expect(rows()).toHaveLength(9);
  await type("нет такого");
  expect(rows()).toHaveLength(0);
  expect(menu()!.textContent).toContain("Ничего не найдено");
  expect(menu()!.querySelector(".padd")).not.toBeNull();
  expect(app.filter).toBe("");
  key(field(), "Escape"); await settle();
  expect(field().value).toBe("");
  expect(rows()).toHaveLength(41);
  key(field(), "Escape"); await settle();
  expect(menu()).toBeNull();
  await openMenu();
  expect(field().value).toBe("");
});

it("Enter в поле выбирает первую строку списка", async () => {
  app.select = vi.fn(async (path: string) => { app.current = path; }) as typeof app.select;
  await openMenu();
  await type("project-03");
  key(field(), "Enter"); await settle();
  expect(app.select).toHaveBeenCalledWith("C:/work/project-03");
  expect(menu()).toBeNull();
});

it("стрелки, Home/End, Enter и пробел; Esc возвращает фокус на кнопку проекта", async () => {
  app.select = vi.fn(async (path: string) => { app.current = path; }) as typeof app.select;
  await openMenu();
  key(field(), "ArrowDown");
  expect(document.activeElement).toBe(rows()[0]);
  key(rows()[0], "ArrowDown");
  expect(document.activeElement).toBe(rows()[1]);
  key(rows()[1], "ArrowUp");
  expect(document.activeElement).toBe(rows()[0]);
  key(rows()[0], "End");
  expect(document.activeElement).toBe(menu()!.querySelector(".padd"));
  key(document.activeElement!, "Home");
  expect(document.activeElement).toBe(rows()[0]);
  key(rows()[0], " "); await settle();
  expect(app.select).toHaveBeenCalledWith("C:/work/project-01");
  await openMenu();
  key(field(), "ArrowDown"); key(document.activeElement!, "ArrowDown"); key(document.activeElement!, "Enter"); await settle();
  expect(app.select).toHaveBeenLastCalledWith("C:/work/project-02");
  await openMenu();
  key(field(), "ArrowDown");
  key(document.activeElement!, "Escape"); await settle();
  expect(menu()).toBeNull();
  expect(document.activeElement).toBe(document.querySelector("button.proj"));
});
