// Этап 3.4.2 (Т-40, НФТ-5): закрытие отдельного окна документа записывает правки; при отказе спрашивает.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import DocumentWindow from "./DocumentWindow.svelte";
import { app } from "./app.svelte";
import { call, documentRead, documentRevision, documentWindowClose, documentWrite } from "./api";
import { editors } from "./editors.svelte";
import { unsavedPrompt } from "./unsaved.svelte";
import { moved } from "./moved.svelte";
import type { Change } from "./window-updates";

const listeners = new Map<string, (event?: {payload:Change}) => void | Promise<void>>();
let snapshotRevision = 0;
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async (command: string, args: Record<string,unknown>) => {
  if (command === "window_observe") return args.project ? {project:args.project,subscription:args.project,epoch:1} : null;
  if (command === "window_snapshot") return {project:"C:/p",subscription:args.subscription,epoch:1,revisions:[++snapshotRevision,snapshotRevision,0],parts:{index:{revision:snapshotRevision,state:"ready",value:await call("tree",{project:"C:/p",cwd:"C:/p"})}}};
  return null;
}) }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, handler: () => Promise<void>) => { listeners.set(name, handler); return () => listeners.delete(name); }),
  emit: vi.fn(async () => {}),
}));
vi.mock("./api", async (original) => ({
  ...(await original<typeof import("./api")>()),
  call: vi.fn(), documentRead: vi.fn(), documentRevision: vi.fn(), documentWrite: vi.fn(), documentWindowClose: vi.fn(), inTauri: () => true,
}));

const settle = async () => { await new Promise((resolve) => setTimeout(resolve, 0)); flushSync(); };
const read = (text: string, revision: string) => ({ status: "ok" as const, text, revision, eol: "lf" as const, bom: false, size: 5, read_only: null });
let component: ReturnType<typeof mount> | undefined;

beforeEach(async () => {
  listeners.clear();
  snapshotRevision = 0;
  vi.mocked(call).mockReset();
  vi.mocked(call).mockImplementation(async (method: string) => method === "read" ? ({ title: "А", text: "# а\n" } as never) : method === "tree" ? ({docs_root:"docs",documents:[]} as never) : method === "project_observe" ? ({saved:true} as never) : new Promise(() => {}));
  vi.mocked(documentRead).mockResolvedValue(read("# а\n", "r1"));
  vi.mocked(documentWrite).mockReset(); vi.mocked(documentRevision).mockReset(); vi.mocked(documentWindowClose).mockReset();
  vi.mocked(documentWindowClose).mockResolvedValue(undefined);
  component = mount(DocumentWindow, { target: document.body, props: { project: "C:/p", path: "docs/a.md" } });
  flushSync(); await settle(); await settle();
});
afterEach(() => {
  app.stop();
  if (component) unmount(component); component = undefined; document.body.innerHTML = "";
  unsavedPrompt.pending?.resolve("cancel");
  const buffer = editors.get("C:/p", "docs/a.md"); if (buffer) editors.discard(buffer);
});

it("окно показывает один документ в редакторе и подписано его именем; ссылки идут в основное окно", () => {
  expect(document.querySelector('section[data-document-path="docs/a.md"] .cm-editor')).not.toBeNull();
  expect(document.title).toBe("a.md — bars");
  expect(app.documentWindow).toBe(true);
  expect(document.querySelector(".workspace, .dv-dockview")).toBeNull();
  expect(listeners.has("bars://document-close-request")).toBe(true);
});

it("закрытие окна с несохранённой правкой сначала записывает её, потом закрывает окно", async () => {
  const buffer = editors.get("C:/p", "docs/a.md")!;
  vi.mocked(documentWrite).mockResolvedValue({ status: "saved", revision: "r2" });
  buffer.edit("# а\nправка\n");
  await listeners.get("bars://document-close-request")!();
  expect(documentWrite).toHaveBeenCalledWith("C:/p", "docs/a.md", "# а\nправка\n", "r1", "lf", false);
  expect(documentWindowClose).toHaveBeenCalledOnce();
});

it("если запись не удалась, окно не закрывается, пока пользователь не решит: «Вернуться» оставляет окно", async () => {
  const buffer = editors.get("C:/p", "docs/a.md")!;
  vi.mocked(documentWrite).mockResolvedValue({ status: "conflict", revision: "r9" });
  buffer.edit("# а\nправка\n");
  const closing = listeners.get("bars://document-close-request")!();
  await settle();
  expect(unsavedPrompt.pending?.items).toEqual([buffer]);
  unsavedPrompt.pending!.resolve("cancel");
  await closing;
  expect(documentWindowClose).not.toHaveBeenCalled();
  expect(buffer.text).toBe("# а\nправка\n");
});

it("«Закрыть без записи» закрывает окно и отбрасывает правки", async () => {
  const buffer = editors.get("C:/p", "docs/a.md")!;
  vi.mocked(documentWrite).mockResolvedValue({ status: "conflict", revision: "r9" });
  buffer.edit("# а\nправка\n");
  const closing = listeners.get("bars://document-close-request")!();
  await settle();
  unsavedPrompt.pending!.resolve("discard");
  await closing;
  expect(documentWindowClose).toHaveBeenCalledOnce();
  expect(editors.get("C:/p", "docs/a.md")).toBeUndefined();
});

it("дерево для `[[` обновляется по ревизии воркера, без локального debounce или опроса (3.5.7)", async () => {
  vi.mocked(call).mockImplementation(async (method: string) => method === "tree" ? ({ docs_root: "docs", documents: [] } as never) : method === "project_observe" ? ({saved:true} as never) : new Promise(() => {}));
  vi.mocked(documentRevision).mockResolvedValue({ status: "ok", revision: "r1" } as never);
  const trees = () => vi.mocked(call).mock.calls.filter(([method]) => method === "tree").length;
  const before = trees();
  vi.useFakeTimers();
  try {
    await vi.advanceTimersByTimeAsync(20_000);
    expect(trees()).toBe(before);
    await moved.accept({ project: "C:/p", from: "docs/x.md", to: "docs/y.md" });
    await moved.accept({ project: "C:/p", from: "docs/y.md", to: "docs/z.md" });
    await vi.advanceTimersByTimeAsync(2000);
    expect(trees()).toBe(before);
    await listeners.get("bars://project-changed")!({payload:{project:"C:/p",subscription:"C:/p",epoch:1,revisions:[2,2,0]}});
    await vi.advanceTimersByTimeAsync(10);
    expect(trees()).toBe(before + 1);
    vi.mocked(documentWrite).mockResolvedValue({ status: "saved", revision: "r2" });
    editors.get("C:/p", "docs/a.md")!.edit("# а\nправка\n");
    await editors.flush();
    await vi.advanceTimersByTimeAsync(2000);
    expect(trees()).toBe(before + 1);
    await listeners.get("bars://project-changed")!({payload:{project:"C:/p",subscription:"C:/p",epoch:1,revisions:[3,3,0]}});
    await vi.advanceTimersByTimeAsync(10);
    expect(trees()).toBe(before + 2);
  } finally { vi.useRealTimers(); }
});
