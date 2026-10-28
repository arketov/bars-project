// Этап 3.4.2 (Т-40): отдельные окна документа, согласованность правок между окнами, `Ctrl+O`.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import Workspace from "./Workspace.svelte";
import { app } from "./app.svelte";
import { call, documentRead, documentRevision, documentWrite, openInMainWindow, type DocumentRead } from "./api";
import { Editors, type Bus, type SavedMessage } from "./editors.svelte";
import { documentMenu } from "./menu-model";
import { documentTargets } from "./link-picker";
import { keybindings } from "./keybindings.svelte";
import { buildTree } from "./tree";

vi.mock("./api", async (original) => ({
  ...(await original<typeof import("./api")>()),
  call: vi.fn(), documentRead: vi.fn(), documentRevision: vi.fn(), documentWrite: vi.fn(), openInMainWindow: vi.fn(), inTauri: () => true,
}));
vi.mock("./graph-renderer", () => ({ GraphRenderer: class { set() {} destroy() {} } }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}), emit: vi.fn(async () => {}) }));

const read = (text: string, revision: string): DocumentRead => ({ status: "ok", text, revision, eol: "lf", bom: false, size: 5, read_only: null });

/** Общий канал двух окон: сообщение получают все подписчики, как события Tauri. */
function sharedBus(): () => Bus {
  const handlers: ((message: SavedMessage) => void)[] = [];
  return () => ({ emit: (message) => handlers.forEach((handler) => handler(message)), listen: (handler) => void handlers.push(handler) });
}

const settle = async () => { await new Promise((resolve) => setTimeout(resolve, 0)); flushSync(); };

beforeEach(() => {
  vi.mocked(documentRead).mockReset(); vi.mocked(documentWrite).mockReset(); vi.mocked(documentRevision).mockReset();
});

it("запись в одном окне молча обновляет чистый буфер того же документа в другом окне", async () => {
  const bus = sharedBus();
  const main = new Editors(bus()), second = new Editors(bus());
  vi.mocked(documentRead).mockResolvedValue(read("# а\n", "r1"));
  const a = main.attach("C:/p", "docs/a.md", ""), b = second.attach("C:/p", "docs/a.md", "");
  await settle();
  vi.mocked(documentWrite).mockResolvedValue({ status: "saved", revision: "r2" });
  vi.mocked(documentRevision).mockResolvedValue({ status: "ok", revision: "r2" });
  vi.mocked(documentRead).mockResolvedValue(read("# а\nновая строка\n", "r2"));
  a.edit("# а\nновая строка\n");
  await a.save();
  await settle();
  expect(b.text).toBe("# а\nновая строка\n");
  expect(b.revision).toBe("r2");
  expect(b.status).toBe("clean");
  expect(b.notice).toBe("Обновлено с диска");
});

it("запись в другом окне при несохранённых правках здесь даёт конфликт, а не перезапись", async () => {
  const bus = sharedBus();
  const main = new Editors(bus()), second = new Editors(bus());
  vi.mocked(documentRead).mockResolvedValue(read("# а\n", "r1"));
  const a = main.attach("C:/p", "docs/a.md", ""), b = second.attach("C:/p", "docs/a.md", "");
  await settle();
  b.edit("# а\nмоя правка\n");
  vi.mocked(documentWrite).mockResolvedValue({ status: "saved", revision: "r2" });
  vi.mocked(documentRevision).mockResolvedValue({ status: "ok", revision: "r2" });
  a.edit("# а\nчужая правка\n");
  await a.save();
  await settle();
  expect(b.status).toBe("conflict");
  expect(b.text).toBe("# а\nмоя правка\n");
  expect(b.diskRevision).toBe("r2");
});

it("своя запись не заставляет окно сверять диск заново", async () => {
  const bus = sharedBus();
  const main = new Editors(bus());
  vi.mocked(documentRead).mockResolvedValue(read("# а\n", "r1"));
  const a = main.attach("C:/p", "docs/a.md", "");
  await settle();
  vi.mocked(documentWrite).mockResolvedValue({ status: "saved", revision: "r2" });
  a.edit("# б\n");
  await a.save();
  await settle();
  expect(documentRevision).not.toHaveBeenCalled();
});

it("запись другого документа или проекта не трогает буфер", async () => {
  const bus = sharedBus();
  const main = new Editors(bus()), second = new Editors(bus());
  vi.mocked(documentRead).mockResolvedValue(read("# а\n", "r1"));
  const a = main.attach("C:/p", "docs/a.md", ""), other = second.attach("C:/p", "docs/b.md", ""), foreign = second.attach("C:/q", "docs/a.md", "");
  await settle();
  vi.mocked(documentWrite).mockResolvedValue({ status: "saved", revision: "r2" });
  a.edit("# б\n");
  await a.save();
  await settle();
  expect(documentRevision).not.toHaveBeenCalled();
  expect([other.status, foreign.status]).toEqual(["clean", "clean"]);
});

it("контекстное меню документа предлагает «Открыть в новом окне» сразу после «Открыть»", () => {
  const items = documentMenu({ path: "docs/a.md", place: "card", state: "ready", inTree: true });
  expect(items.map((item) => item.action).slice(0, 2)).toEqual(["open", "newWindow"]);
  expect(items[1].label).toBe("Открыть в новом окне");
});

it("выбор по Ctrl+O включает index.md папок, в отличие от выбора ссылки", () => {
  const tree = buildTree([
    { path: "docs/a/index.md", module: "a", title: "Папка A", chunks: 1 },
    { path: "docs/a/x.md", module: "a", title: "Икс", chunks: 1 },
  ], "docs");
  expect(documentTargets(tree, "docs", true).map((t) => t.docs)).toEqual(["a/index.md", "a/x.md"]);
  expect(documentTargets(tree, "docs").map((t) => t.docs)).toEqual(["a/x.md"]);
});

it("в отдельном окне документа ссылка открывается в основном окне, а не здесь", async () => {
  vi.mocked(openInMainWindow).mockResolvedValue(undefined);
  app.documentWindow = true; app.current = "C:/p";
  await app.openDocument({ path: "docs/b.md", title: "b", first_line: 3, last_line: 4 });
  expect(openInMainWindow).toHaveBeenCalledWith("C:/p", "docs/b.md", 3, 4);
  expect(app.viewer).toBeNull();
  app.documentWindow = false;
});

let component: ReturnType<typeof mount> | undefined;
afterEach(() => { if (component) unmount(component); component = undefined; document.body.innerHTML = ""; });

const project = { path: "C:/А", name: "А", docs_root: "docs", state: "ready" as const, modules: [{ name: "manual", documents: 2 }] };
const ctrlO = (options: KeyboardEventInit = {}) =>
  window.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, code: "KeyO", key: "щ", ctrlKey: true, ...options }));

async function openPicker() {
  keybindings.active = {}; keybindings.recording = false;
  localStorage.clear(); vi.mocked(call).mockReset();
  vi.mocked(call).mockImplementation(() => new Promise(() => {}));
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute("open", ""); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute("open"); };
  app.projects = [project]; app.current = project.path; app.view = "main"; app.viewer = null; app.topicsOpen = false; app.activity = null;
  app.tree = buildTree([
    { path: "docs/manual/a.md", module: "manual", title: "Альфа", chunks: 1 },
    { path: "docs/manual/b.md", module: "manual", title: "Бета", chunks: 1 },
  ], "docs");
  component = mount(Workspace, { target: document.body }); flushSync(); await settle();
  ctrlO(); flushSync(); await settle();
  return document.querySelector<HTMLDialogElement>('dialog[aria-label="Открыть документ"]')!;
}

it("Ctrl+O (в русской раскладке тоже) открывает выбор документа по названию; Enter открывает вкладкой", async () => {
  const open = vi.spyOn(app, "openDocument").mockResolvedValue();
  const modal = await openPicker();
  expect(modal.open).toBe(true);
  const rows = [...modal.querySelectorAll('[role="option"]')].map((row) => row.textContent);
  expect(rows).toHaveLength(2);
  expect(rows[0]).toContain("Альфа");
  const query = modal.querySelector<HTMLInputElement>("input")!;
  query.value = "бет"; query.dispatchEvent(new Event("input", { bubbles: true })); flushSync();
  query.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true })); flushSync(); await settle();
  expect(open).toHaveBeenCalledWith({ path: "docs/manual/b.md", title: "Бета", first_line: 0, last_line: 0 });
  open.mockRestore();
});

it("Alt+Enter в выборе документа открывает его в новом окне", async () => {
  const open = vi.spyOn(app, "openDocument").mockResolvedValue();
  const inWindow = vi.spyOn(app, "openInNewWindow").mockResolvedValue();
  const modal = await openPicker();
  modal.querySelector<HTMLInputElement>("input")!.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", altKey: true, bubbles: true, cancelable: true })); flushSync(); await settle();
  expect(inWindow).toHaveBeenCalledWith("docs/manual/a.md");
  expect(open).not.toHaveBeenCalled();
  open.mockRestore(); inWindow.mockRestore();
});

it("после трёх знаков ниже названий добавляются документы из поиска по смыслу, без повторов", async () => {
  const modal = await openPicker();
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  try {
    vi.mocked(call).mockImplementation(async (method: string) => method === "search"
      ? ({ results: [{ path: "docs/manual/a.md" }, { path: "docs/manual/b.md" }, { path: "docs/outside.md" }] } as never)
      : new Promise(() => {}));
    const query = modal.querySelector<HTMLInputElement>("input")!;
    query.value = "бет"; query.dispatchEvent(new Event("input", { bubbles: true })); flushSync();
    await vi.advanceTimersByTimeAsync(300); flushSync();
    // «Бета» найдена по названию; «Альфа» добавлена по смыслу; документа вне дерева нет.
    const rows = [...modal.querySelectorAll('[role="option"]')].map((row) => row.textContent ?? "");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toContain("Бета");
    expect(rows[1]).toContain("Альфа");
    expect(rows[1]).toContain("по смыслу");
  } finally { vi.useRealTimers(); }
});
