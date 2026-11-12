import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import Sidebar from "./Sidebar.svelte";
import ContextMenu from "./ContextMenu.svelte";
import { app } from "./app.svelte";
import { buildSwitcher } from "./projects";
import type { ProjectInfo } from "./types";

const api = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("./api", () => ({ ...api, joinPath: vi.fn(), openPath: vi.fn(), pickFolder: vi.fn(), copyText: vi.fn(), documentAction: vi.fn(), documentEditors: vi.fn(), inTauri: () => false }));

const base = { docs_root: "docs", modules: [] };
const main: ProjectInfo = { ...base, name: "tmc", path: "C:\w\tmc", state: "ready" };
const worker: ProjectInfo = { ...base, name: "worker", path: "C:\w\tmc\backend\worker", state: "lazy", parent: main.path, qualified_name: "tmc/backend/worker", discovered: true };
const api1: ProjectInfo = { ...base, name: "api", path: "C:\w\tmc\backend\api", state: "lazy", parent: main.path, qualified_name: "tmc/backend/api", discovered: true };
const vendor: ProjectInfo = { ...base, name: "x", path: "C:\w\tmc\backend\api\vendor\x", state: "lazy", parent: api1.path, qualified_name: "tmc/backend/api/vendor/x", discovered: true };
const angular: ProjectInfo = { ...base, name: "web-angular", path: "C:\w\tmc\frontend\web-angular", state: "ready", parent: main.path, qualified_name: "tmc/frontend/web-angular", discovered: false };
const other: ProjectInfo = { ...base, name: "bars", path: "C:\w\bars", state: "ready" };
// В реестре подпроекты идут не по порядку пути: сортировка под предком — по пути.
const registry = [main, worker, other, angular, vendor, api1];

const components: ReturnType<typeof mount>[] = [];
const settle = async () => { await new Promise(resolve => setTimeout(resolve, 0)); flushSync(); };
const rows = () => Array.from(document.querySelectorAll<HTMLElement>(".projmenu .pitem"));
const labels = () => rows().map(r => r.querySelector("b")!.textContent);
const chev = (row: HTMLElement) => row.querySelector<HTMLButtonElement>("button.chev");
const field = () => document.querySelector<HTMLInputElement>(".projmenu .pfilter input")!;
async function openMenu() { document.querySelector<HTMLButtonElement>("button.proj")!.click(); await settle(); }
function key(target: Element, name: string) { target.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true })); flushSync(); }
async function type(value: string) { const input = field(); input.value = value; input.dispatchEvent(new Event("input", { bubbles: true })); await settle(); }

beforeEach(() => {
  Element.prototype.scrollIntoView = function () {};
  app.projects = registry; app.current = other.path; app.tree = []; app.activity = null; app.filter = "";
  api.call.mockResolvedValue({ accepted: true });
  components.push(mount(Sidebar, { target: document.body })); flushSync();
});
afterEach(async () => {
  for (const component of components.splice(0)) await unmount(component);
  document.body.innerHTML = ""; app.current = null; app.projects = [];
  vi.clearAllMocks();
});

const open = (...paths: string[]) => new Map(paths.map(p => [p.toLowerCase().replace(/\/g, "/"), true]));

it("buildSwitcher: верхний уровень без найденных, самостоятельный подпроект дважды, сортировка по пути, подпись от родителя", () => {
  const all = buildSwitcher(registry, { overrides: open(main.path, api1.path) });
  expect(all.map(r => [r.label, r.depth])).toEqual([
    ["tmc", 0], ["backend/api", 1], ["vendor/x", 2], ["backend/worker", 1], ["frontend/web-angular", 1],
    ["bars", 0], ["web-angular", 0],
  ]);
  expect(new Set(all.map(r => r.key)).size).toBe(all.length);
  expect(all.filter(r => r.project === angular)).toHaveLength(2);
  expect(all.filter(r => r.project.discovered && r.depth === 0)).toHaveLength(0);
  expect(buildSwitcher(registry).map(r => r.label)).toEqual(["tmc", "bars", "web-angular"]);
});

it("buildSwitcher: глубина растёт по предкам, предки текущего раскрыты, ручное сворачивание сильнее", () => {
  const deep = buildSwitcher(registry, { current: vendor.path });
  expect(deep.map(r => [r.label, r.depth])).toContainEqual(["vendor/x", 2]);
  expect(deep.find(r => r.project === main)?.expanded).toBe(true);
  const collapsed = buildSwitcher(registry, { current: vendor.path, overrides: new Map([["c:/w/tmc", false]]) });
  expect(collapsed.map(r => r.label)).toEqual(["tmc", "bars", "web-angular"]);
});

it("buildSwitcher: фильтр — совпавший подпроект под раскрытым предком, предок без совпадения — контекстная строка", () => {
  const found = buildSwitcher(registry, { filter: "worker" });
  expect(found.map(r => [r.label, r.context, r.expanded])).toEqual([["tmc", true, true], ["backend/worker", false, false]]);
  expect(buildSwitcher(registry, { filter: "нет такого" })).toEqual([]);
});

it("шеврон и aria-expanded: клик раскрывает и сворачивает, проект не выбирается; отступы по уровням", async () => {
  app.select = vi.fn(async (path: string) => { app.current = path; }) as typeof app.select;
  await openMenu();
  expect(labels()).toEqual(["tmc", "bars", "web-angular"]);
  const head = rows()[0];
  expect(chev(head)!.getAttribute("aria-expanded")).toBe("false");
  expect(chev(rows()[1])).toBeNull();
  chev(head)!.click(); await settle();
  expect(app.select).not.toHaveBeenCalled();
  expect(document.querySelector(".projmenu")).not.toBeNull();
  expect(chev(rows()[0])!.getAttribute("aria-expanded")).toBe("true");
  expect(labels()).toEqual(["tmc", "backend/api", "backend/worker", "frontend/web-angular", "bars", "web-angular"]);
  expect(rows().map(r => r.dataset.depth)).toEqual(["0", "1", "1", "1", "0", "0"]);
  expect(rows()[1].style.paddingLeft).toBe("26px");
  expect(rows()[1].title).toBe("tmc/backend/api");
  chev(rows()[1])!.click(); await settle();
  expect(labels()).toContain("vendor/x");
  expect(rows().find(r => r.textContent?.includes("vendor/x"))!.style.paddingLeft).toBe("44px");
  chev(rows()[0])!.click(); await settle();
  expect(labels()).toEqual(["tmc", "bars", "web-angular"]);
  rows()[0].click(); await settle();
  expect(app.select).toHaveBeenCalledWith(main.path);
});

it("стрелка вправо раскрывает, повторная — к первому подпроекту; влево сворачивает, затем — к родителю", async () => {
  await openMenu();
  const head = () => rows()[0];
  key(field(), "ArrowDown");
  expect(document.activeElement).toBe(head());
  key(head(), "ArrowRight");
  expect(labels()).toContain("backend/api");
  expect(chev(head())!.getAttribute("aria-expanded")).toBe("true");
  key(head(), "ArrowRight");
  expect(document.activeElement).toBe(rows()[1]);
  key(rows()[1], "ArrowLeft");
  expect(document.activeElement).toBe(head());
  key(head(), "ArrowLeft");
  expect(labels()).toEqual(["tmc", "bars", "web-angular"]);
  key(head(), "ArrowLeft");
  expect(document.activeElement).toBe(head());
  key(field(), "ArrowRight"); // в поле стрелки остаются полю
});

it("самостоятельный подпроект виден дважды, обе строки — текущий проект; его предки раскрыты", async () => {
  app.current = angular.path; flushSync();
  await openMenu();
  const mine = rows().filter(r => r.dataset.projectPath === angular.path);
  expect(mine).toHaveLength(2);
  expect(mine.every(r => r.classList.contains("on"))).toBe(true);
  expect(mine.map(r => r.querySelector("b")!.textContent)).toEqual(["frontend/web-angular", "web-angular"]);
  expect(new Set(mine.map(r => r.dataset.rowKey)).size).toBe(2);
  expect(document.querySelector(".proj b")!.textContent).toBe("tmc/frontend/web-angular");
});

it("фильтр: подпроект под раскрытым приглушённым предком, предок выбираем; сброс раскрытия вместе с текстом", async () => {
  await openMenu();
  await type("worker");
  expect(labels()).toEqual(["tmc", "backend/worker"]);
  expect(rows()[0].classList.contains("ctx")).toBe(true);
  expect(rows()[0].dataset.projectPath).toBe(main.path);
  chev(rows()[0])!.click(); await settle();
  expect(labels()).toEqual(["tmc"]);
  await type("worke");
  expect(labels()).toEqual(["tmc", "backend/worker"]);
  await type("");
  expect(labels()).toEqual(["tmc", "bars", "web-angular"]);
});

it("точка idle у ленивого подпроекта с подписью «ждёт первого обращения»", async () => {
  await openMenu();
  chev(rows()[0])!.click(); await settle();
  const row = rows().find(r => r.dataset.projectPath === api1.path)!;
  expect(row.querySelector(".dot")!.classList.contains("idle")).toBe(true);
  expect(row.querySelector("small")!.textContent).toBe("ждёт первого обращения");
});

it("автовыбор при пустом current — первый проект верхнего уровня, не найденный подпроект", async () => {
  app.select = vi.fn(async (path: string | null) => { app.current = path; }) as typeof app.select;
  app.current = null;
  api.call.mockResolvedValueOnce({ projects: [worker, main, other] });
  await app.loadProjects();
  expect(app.select).toHaveBeenCalledWith(main.path);
  api.call.mockResolvedValueOnce({ projects: [worker, api1] });
  app.current = null;
  await app.loadProjects();
  expect(app.select).toHaveBeenCalledTimes(1); // только найденные подпроекты: сами не открываются
});

async function contextOn(element: Element) {
  element.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 10, clientY: 10 })); await settle();
}
const menuLabels = () => Array.from(document.querySelectorAll<HTMLButtonElement>(".cmenu button")).map(b => b.textContent ?? "");

it("контекстное меню найденного подпроекта: без «Переименовать…», «Проверить проект» и «Удалить проект» остаются", async () => {
  components.push(mount(ContextMenu, { target: document.body })); flushSync();
  await openMenu();
  chev(rows()[0])!.click(); await settle();
  await contextOn(rows().find(r => r.dataset.projectPath === api1.path)!.querySelector("b")!);
  expect(menuLabels().some(t => t.startsWith("Переименовать"))).toBe(false);
  expect(menuLabels().some(t => t.startsWith("Удалить проект"))).toBe(true);
  expect(menuLabels().some(t => t.startsWith("Проверить"))).toBe(true);
});

it("контекстное меню зарегистрированного проекта и самостоятельного подпроекта — с «Переименовать…»", async () => {
  components.push(mount(ContextMenu, { target: document.body })); flushSync();
  await openMenu();
  await contextOn(rows().find(r => r.dataset.projectPath === angular.path)!.querySelector("b")!);
  expect(menuLabels().some(t => t.startsWith("Переименовать"))).toBe(true);
});
