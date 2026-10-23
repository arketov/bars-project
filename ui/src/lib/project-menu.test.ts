import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import Sidebar from "./Sidebar.svelte";
import ContextMenu from "./ContextMenu.svelte";
import { app } from "./app.svelte";

const api = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("./api", () => ({ ...api, joinPath: vi.fn(), openPath: vi.fn(), pickFolder: vi.fn(), copyText: vi.fn(), documentAction: vi.fn(), documentEditors: vi.fn(), inTauri: () => false }));
const first = { name: "Одинаковое имя", path: "C:/первый проект", docs_root: "docs", state: "ready" as const, modules: [] };
const second = { ...first, path: "D:/другой проект" };
const components: ReturnType<typeof mount>[] = [];
const settle = async () => { await new Promise(resolve => setTimeout(resolve, 0)); flushSync(); };
function host() { const el = document.createElement("div"); document.body.append(el); return el; }
function item(label: string) { return Array.from(document.querySelectorAll<HTMLButtonElement>(".cmenu button")).find(b => b.textContent?.startsWith(label))!; }
async function open(element: Element) {
  const event = new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 900, clientY: 700 });
  element.dispatchEvent(event); await settle(); expect(event.defaultPrevented).toBe(true);
}
beforeEach(() => {
  app.projects = [first, second]; app.current = first.path; app.tree = []; app.activity = null;
  app.filter = ""; app.selected = new Set(["сохранить"]); app.query = "исходный запрос"; app.toast = null;
  api.call.mockResolvedValue({ accepted: true });
  components.push(mount(Sidebar, { target: host() }));
  components.push(mount(ContextMenu, { target: host() })); flushSync();
});
afterEach(async () => {
  for (const component of components.splice(0)) await unmount(component);
  document.body.innerHTML = ""; app.current = null; app.projects = []; app.selected = new Set();
  vi.clearAllMocks();
});

it("меню выбранного проекта: подтверждение удаления, проверка без смены области", async () => {
  await open(document.querySelector(".proj b")!);
  const remove = item("Удалить проект");
  expect(remove.disabled).toBe(false);
  remove.click(); expect(api.call).not.toHaveBeenCalled();
  expect(app.deletingProject?.path).toBe(first.path);
  app.cancelDeletion();
  await open(document.querySelector(".proj b")!);
  item("Проверить индекс").click(); await settle();
  expect(api.call).toHaveBeenCalledExactlyOnceWith("reindex", { cwd: first.path, project: first.path });
  expect(app.current).toBe(first.path); expect(app.query).toBe("исходный запрос");
  expect([...app.selected]).toEqual(["сохранить"]);
  expect(app.toast).toEqual({ text: "Проверка проекта поставлена в очередь", bad: false });
});

it("одинаковые имена в переключателе: проверяется нажатый проект без выбора", async () => {
  document.querySelector<HTMLButtonElement>(".proj")!.click(); flushSync();
  await open(document.querySelectorAll(".pitem b")[1]);
  expect(app.current).toBe(first.path);
  item("Проверить индекс").click(); await settle();
  expect(api.call).toHaveBeenCalledExactlyOnceWith("reindex", { cwd: second.path, project: second.path });
  expect(app.current).toBe(first.path);
});

it("меню пропавшего проекта в списке; ошибка без ложного успеха", async () => {
  app.projects = [first, { ...second, state: "unavailable" }]; app.current = second.path; flushSync();
  await open(document.querySelectorAll(".litem b")[1]);
  api.call.mockRejectedValueOnce(new Error("Папка проекта не найдена"));
  item("Проверить индекс").click(); await settle();
  expect(app.toast).toEqual({ text: "Папка проекта не найдена", bad: true });
  expect(api.call).toHaveBeenCalledExactlyOnceWith("reindex", { cwd: second.path, project: second.path });
});

it("Shift+F10 и стрелки обходят пункты; Escape/Tab закрывают меню", async () => {
  const project = document.querySelector<HTMLButtonElement>(".proj")!;
  for (const key of ["Escape", "Tab"]) {
    project.focus(); project.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "F10", shiftKey: true })); await settle();
    for (const [arrow, label] of [["ArrowUp", "Проверить индекс"], ["ArrowDown", "Удалить проект"], ["Home", "Удалить проект"], ["End", "Проверить индекс"]]) {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: arrow }));
      expect(document.activeElement).toBe(item(label));
    }
    window.dispatchEvent(new KeyboardEvent("keydown", { key })); flushSync();
    expect(document.querySelector(".cmenu")).toBeNull();
  }
});

it("меню закрывается по внешнему клику, прокрутке, blur, resize и смене проекта", async () => {
  for (const event of [new MouseEvent("mousedown", { bubbles: true }), new Event("scroll"), new Event("blur"), new Event("resize")]) {
    await open(document.querySelector(".proj")!); window.dispatchEvent(event); flushSync();
    expect(document.querySelector(".cmenu")).toBeNull();
  }
  await open(document.querySelector(".proj")!); app.current = second.path; await settle();
  expect(document.querySelector(".cmenu")).toBeNull();
});

it("исчезновение целевого проекта закрывает меню", async () => {
  document.querySelector<HTMLButtonElement>(".proj")!.click(); flushSync();
  await open(document.querySelectorAll(".pitem")[1]);
  app.projects = [first]; await settle();
  expect(document.querySelector(".cmenu")).toBeNull(); expect(api.call).not.toHaveBeenCalled();
});
