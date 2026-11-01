import { afterEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import { SvelteMap } from "svelte/reactivity";
import { EditorView } from "@codemirror/view";
import { undo, undoDepth } from "@codemirror/commands";
import Editor from "./Editor.svelte";
import WorkspaceDocument from "./WorkspaceDocument.svelte";
import { editors } from "./editors.svelte";
import { documentRead, documentWrite } from "./api";
import type { DocumentReference } from "./app.svelte";

vi.mock("./api", async original => ({
  ...(await original<typeof import("./api")>()), inTauri: () => false,
  call: vi.fn(() => new Promise(() => {})), documentRead: vi.fn(), documentWrite: vi.fn(),
}));
let component: ReturnType<typeof mount>;
const text = "# Документ\nПервая строка\nВторая строка\nКонец";
const hits = new SvelteMap<string, { first: number; last: number; column?: number; length?: number } | null>();
const view = () => EditorView.findFromDOM(document.querySelector(".cm-editor")!)!;
const label = () => document.querySelector(".cm-found-label")?.textContent;
const settle = async () => { await new Promise(resolve => setTimeout(resolve, 0)); flushSync(); };
afterEach(async () => {
  if (component) await unmount(component);
  const buffer = editors.get("C:/fragment", "docs/a.md"); if (buffer) editors.discard(buffer);
  document.body.replaceChildren(); hits.clear();
});

it("диапазон и повторный переход меняют декорацию, текст и история остаются чистыми", () => {
  const input = vi.fn(); hits.set("hit", { first: 2, last: 3 });
  component = mount(Editor, { target: document.body, props: { text, get hit() { return hits.get("hit"); }, oninput: input } }); flushSync();
  expect(label()).toBe("строки 2–3 · найдено");
  expect(document.querySelectorAll(".cm-found-line")).toHaveLength(2);
  expect(view().state.doc.toString()).toBe(text); expect(undoDepth(view().state)).toBe(0); expect(input).not.toHaveBeenCalled();
  hits.set("hit", { first: 4, last: 4 }); flushSync();
  expect(label()).toBe("строка 4 · найдено"); expect(document.querySelectorAll(".cm-found-line")).toHaveLength(1);
  hits.set("hit", null); flushSync(); expect(label()).toBeUndefined();
});

it.each([0, 4])("точное вхождение длиной %i сохраняет подпись, символы и снимается первой правкой", length => {
  hits.set("hit", { first: 2, last: 2, column: 2, length });
  component = mount(Editor, { target: document.body, props: { text, get hit() { return hits.get("hit"); }, oninput: vi.fn() } }); flushSync();
  expect(label()).toBe("строка 2 · найдено");
  expect(document.querySelector(".cm-hit")?.textContent).toBe(length ? "ерва" : "Первая строка");
  view().dispatch({ changes: { from: view().state.doc.length, insert: "!" }, userEvent: "input.type" }); flushSync();
  expect(label()).toBeUndefined(); expect(document.querySelector(".cm-hit")).toBeNull();
  expect(undo(view())).toBe(true); flushSync(); expect(label()).toBeUndefined();
});

it.each([false, true])("смена режима после правки/отмены не возвращает подпись, замена тем же текстом: %s", async sameText => {
  const reference: DocumentReference = { path: "docs/a.md", title: "Документ", first_line: 2, last_line: 3, fromSearch: true };
  vi.mocked(documentRead).mockResolvedValue({ status: "ok", text, revision: "r1", eol: "lf", bom: false, size: text.length, read_only: null });
  vi.mocked(documentWrite).mockResolvedValue({ status: "saved", revision: "r2" });
  component = mount(WorkspaceDocument, { target: document.body, props: { project: "C:/fragment", hit: reference, initial: { hit: reference, title: "Документ", text, missing: false } } });
  flushSync(); await settle(); await settle();
  const toggle = () => document.querySelector<HTMLButtonElement>('[aria-label="Исходник / рендер"]')!.click();
  expect(label()).toBe("строки 2–3 · найдено");
  toggle(); await settle(); expect(document.querySelector("#hit")).not.toBeNull();
  toggle(); await settle(); expect(label()).toBe("строки 2–3 · найдено");
  view().dispatch({ changes: sameText ? { from: 0, to: 1, insert: "#" } : { from: view().state.doc.length, insert: "!" }, userEvent: "input.type" }); flushSync();
  expect(label()).toBeUndefined(); undo(view()); flushSync();
  toggle(); await settle(); expect(document.querySelector("#hit")).toBeNull();
  toggle(); await settle(); expect(label()).toBeUndefined();
});

it("внешний текст снимает подпись, новый переход назначает новый диапазон", async () => {
  const references = new SvelteMap<string, DocumentReference>();
  const reference: DocumentReference = { path: "docs/a.md", title: "Документ", first_line: 2, last_line: 3 };
  references.set("hit", reference);
  vi.mocked(documentRead).mockResolvedValue({ status: "ok", text, revision: "r1", eol: "lf", bom: false, size: text.length, read_only: null });
  component = mount(WorkspaceDocument, { target: document.body, props: { project: "C:/fragment", get hit() { return references.get("hit")!; }, initial: { hit: reference, title: "Документ", text, missing: false } } });
  flushSync(); await settle(); await settle(); expect(label()).toBe("строки 2–3 · найдено");
  vi.mocked(documentRead).mockResolvedValue({ status: "ok", text: text + " с диска", revision: "r2", eol: "lf", bom: false, size: text.length + 8, read_only: null });
  await editors.get("C:/fragment", "docs/a.md")!.load(); flushSync(); expect(label()).toBeUndefined();
  const toggle = document.querySelector<HTMLButtonElement>('[aria-label="Исходник / рендер"]')!;
  toggle.click(); await settle(); expect(document.querySelector("#hit")).toBeNull();
  toggle.click(); await settle(); expect(label()).toBeUndefined();
  references.set("hit", { ...reference, first_line: 4, last_line: 4 }); flushSync(); await settle();
  expect(label()).toBe("строка 4 · найдено");
});
