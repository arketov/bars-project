// Этап 3.4.2 (Т-40, НФТ-5): закрытие отдельного окна документа записывает правки; при отказе спрашивает.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import DocumentWindow from "./DocumentWindow.svelte";
import { app } from "./app.svelte";
import { call, documentRead, documentRevision, documentWindowClose, documentWrite } from "./api";
import { editors } from "./editors.svelte";
import { unsavedPrompt } from "./unsaved.svelte";

const listeners = new Map<string, () => Promise<void>>();
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
  vi.mocked(call).mockReset();
  vi.mocked(call).mockImplementation(async (method: string) => method === "read" ? ({ title: "А", text: "# а\n" } as never) : new Promise(() => {}));
  vi.mocked(documentRead).mockResolvedValue(read("# а\n", "r1"));
  vi.mocked(documentWrite).mockReset(); vi.mocked(documentRevision).mockReset(); vi.mocked(documentWindowClose).mockReset();
  vi.mocked(documentWindowClose).mockResolvedValue(undefined);
  component = mount(DocumentWindow, { target: document.body, props: { project: "C:/p", path: "docs/a.md" } });
  flushSync(); await settle(); await settle();
});
afterEach(() => {
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
