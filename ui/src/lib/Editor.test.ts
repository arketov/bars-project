import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import { EditorView } from "@codemirror/view";
import { redo, redoDepth, undo, undoDepth } from "@codemirror/commands";
import Editor from "./Editor.svelte";
import { EditorBuffer } from "./editors.svelte";
import { documentRead, documentRevision, documentWrite, type DocumentRead } from "./api";

vi.mock("./api", () => ({ documentRead: vi.fn(), documentRevision: vi.fn(), documentWrite: vi.fn(), inTauri: () => false }));

const disk = (text: string, revision = "r1"): DocumentRead => ({
  status: "ok", text, revision, eol: "lf", bom: false, size: text.length, read_only: null,
});
let buffer: EditorBuffer;
let components: ReturnType<typeof mount>[];

function panel(): EditorView {
  const host = document.createElement("div");
  document.body.append(host);
  components.push(mount(Editor, { target: host, props: {
    get text() { return buffer.text; },
    get historyVersion() { return buffer.historyVersion; },
    oninput: (text: string) => buffer.edit(text),
  } }));
  flushSync();
  return EditorView.findFromDOM(host.querySelector(".cm-editor")!)!;
}

function type(view: EditorView, text: string) {
  view.dispatch({ changes: { from: view.state.doc.length, insert: text }, userEvent: "input.type" });
  flushSync();
}

beforeEach(async () => {
  vi.mocked(documentRead).mockReset();
  vi.mocked(documentRevision).mockReset();
  vi.mocked(documentWrite).mockReset();
  components = [];
  buffer = new EditorBuffer("C:/p", "docs/a.md", "начало");
  vi.mocked(documentRead).mockResolvedValue(disk("начало"));
  await buffer.load();
});
afterEach(async () => {
  for (const component of components) await unmount(component);
  buffer.dispose();
  document.body.innerHTML = "";
});

it("перечитывание чистого буфера сбрасывает отмену и повтор, сохраняет курсор и не пишет старый текст", async () => {
  const view = panel();
  type(view, " моя правка");
  vi.mocked(documentWrite).mockResolvedValue({ status: "saved", revision: "r2" });
  await buffer.save();
  expect(undoDepth(view.state)).toBe(1);
  view.dispatch({ selection: { anchor: 4, head: 2 } });
  vi.mocked(documentRevision).mockResolvedValue({ status: "ok", revision: "r3" });
  vi.mocked(documentRead).mockResolvedValue(disk("диск", "r3"));
  await buffer.checkDisk(); flushSync();
  expect(view.state.selection.main).toMatchObject({ anchor: 4, head: 2 });
  expect([undoDepth(view.state), redoDepth(view.state)]).toEqual([0, 0]);
  expect(undo(view)).toBe(false);
  expect(redo(view)).toBe(false);
  await buffer.save();
  expect(buffer.text).toBe("диск");
  expect(documentWrite).toHaveBeenCalledTimes(1);
  type(view, "!");
  expect(undo(view)).toBe(true); flushSync();
  expect(buffer.text).toBe("диск");
});

it.each(["takeDisk", "saveBeside"] as const)("%s исключает замену из отмены и сбрасывает прежний повтор", async (choice) => {
  const view = panel();
  type(view, " первая");
  expect(undo(view)).toBe(true); flushSync();
  expect(redoDepth(view.state)).toBe(1);
  // Конфликт с черновиком и ранее накопленным повтором.
  buffer.edit("черновик"); flushSync();
  type(view, "!");
  expect(undo(view)).toBe(true); flushSync();
  buffer.status = "conflict";
  vi.mocked(documentRead).mockResolvedValue(disk("чужая версия", "r3"));
  vi.mocked(documentWrite).mockResolvedValue({ status: "saved", revision: "copy" });
  await buffer[choice](); flushSync();
  expect([undo(view), redo(view)]).toEqual([false, false]);
  expect(buffer.text).toBe("чужая версия");
  expect(buffer.dirty).toBe(false);
  if (choice === "saveBeside") expect(vi.mocked(documentWrite).mock.calls[0][2]).toBe("черновик");
});

it("новая версия с тем же текстом сбрасывает историю; ошибка чтения сохраняет историю", async () => {
  const view = panel();
  type(view, "!");
  vi.mocked(documentRead).mockResolvedValue({ status: "error", kind: "io", message: "Ошибка чтения" });
  await buffer.load(); flushSync();
  expect(undoDepth(view.state)).toBe(1);
  vi.mocked(documentRead).mockResolvedValue(disk(buffer.text, "r2"));
  await buffer.load(); flushSync();
  expect(undo(view)).toBe(false);
  expect(buffer.dirty).toBe(false);
});

it("первое чтение заменяет seed без отмены и ограничивает курсор длиной текста", async () => {
  buffer = new EditorBuffer("C:/p", "docs/a.md", "длинный seed");
  const view = panel();
  view.dispatch({ selection: { anchor: 12 } });
  vi.mocked(documentRead).mockResolvedValue(disk("я"));
  await buffer.load(); flushSync();
  expect(view.state.selection.main.anchor).toBe(1);
  expect(undo(view)).toBe(false);
  expect(view.state.doc.toString()).toBe("я");
});

it("автосохранение и эхо своих правок сохраняют отмену; соседняя панель не отменяет чужую правку", async () => {
  const first = panel(), second = panel();
  type(first, "!");
  expect(second.state.doc.toString()).toBe(buffer.text);
  expect(undo(second)).toBe(false);
  vi.mocked(documentWrite).mockResolvedValue({ status: "saved", revision: "r2" });
  await buffer.save(); flushSync();
  expect(undo(first)).toBe(true); flushSync();
  expect(buffer.text).toBe("начало");
  expect(redo(first)).toBe(true); flushSync();
  type(second, " сосед");
  expect(undo(first)).toBe(false);
  expect(undo(second)).toBe(true); flushSync();
  expect(buffer.text).toBe("начало!");
});
