import { afterEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import UnsavedDialog from "./UnsavedDialog.svelte";
import { confirmClose, unsavedPrompt } from "./unsaved.svelte";
import { editors, type EditorBuffer } from "./editors.svelte";

vi.mock("./api", () => ({
  documentRead: vi.fn(),
  documentRevision: vi.fn(),
  documentWrite: vi.fn(async () => ({ status: "saved", revision: "r2" })),
  inTauri: () => false,
}));

let component: ReturnType<typeof mount> | undefined;
const buffers: EditorBuffer[] = [];
function open(path: string): EditorBuffer {
  const buffer = editors.attach("C:/p", path, "исходный");
  buffers.push(buffer);
  return buffer;
}
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  unsavedPrompt.pending?.resolve("cancel");
  buffers.splice(0).forEach((buffer) => editors.discard(buffer));
  document.body.replaceChildren();
});

it("вкладка без буфера или без правок закрывается без вопроса", async () => {
  expect(await confirmClose("C:/p", "docs/none.md")).toBe(true);
  open("docs/a.md");
  expect(await confirmClose("C:/p", "docs/a.md")).toBe(true);
  expect(unsavedPrompt.pending).toBeNull();
});

it("правка сначала записывается, спрашивать нечего", async () => {
  const buffer = open("docs/a.md");
  buffer.edit("новый");
  expect(await confirmClose("C:/p", "docs/a.md")).toBe(true);
  expect(buffer.status).toBe("saved");
  expect(unsavedPrompt.pending).toBeNull();
});

it("при конфликте вкладку держит вопрос: «Вернуться» оставляет вкладку и правки", async () => {
  const buffer = open("docs/a.md");
  buffer.edit("моя");
  buffer.status = "conflict";
  buffer.message = "Файл изменён на диске.";
  const closing = confirmClose("C:/p", "docs/a.md");
  await vi.waitFor(() => expect(unsavedPrompt.pending).not.toBeNull());
  expect(unsavedPrompt.pending?.items).toEqual([buffer]);
  unsavedPrompt.pending!.resolve("cancel");
  expect(await closing).toBe(false);
  expect(editors.get("C:/p", "docs/a.md")).toBe(buffer);
  expect(buffer.text).toBe("моя");
});

it("«Закрыть без записи» удаляет буфер и разрешает закрытие", async () => {
  const buffer = open("docs/a.md");
  buffer.edit("моя");
  buffer.status = "conflict";
  const closing = confirmClose("C:/p", "docs/a.md");
  await vi.waitFor(() => expect(unsavedPrompt.pending).not.toBeNull());
  unsavedPrompt.pending!.resolve("discard");
  expect(await closing).toBe(true);
  expect(editors.get("C:/p", "docs/a.md")).toBeUndefined();
});

it("другой вид того же документа держит буфер: закрытие этого вида ничего не спрашивает", async () => {
  const first = open("docs/a.md");
  open("docs/a.md");
  first.edit("моя");
  first.status = "conflict";
  expect(await confirmClose("C:/p", "docs/a.md")).toBe(true);
  expect(unsavedPrompt.pending).toBeNull();
});

it("диалог называет документы и причину, Escape и «Вернуться» отменяют, «Закрыть без записи» соглашается", async () => {
  component = mount(UnsavedDialog, { target: document.body });
  const buffer = open("docs/a.md");
  buffer.edit("моя");
  buffer.status = "conflict";
  buffer.message = "Файл изменён на диске.";
  let decision = unsavedPrompt.ask([buffer]); flushSync();
  const dialog = document.querySelector('[role="alertdialog"]')!;
  expect(dialog.textContent).toContain("docs/a.md");
  expect(dialog.textContent).toContain("Файл изменён на диске.");
  dialog.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); flushSync();
  expect(await decision).toBe("cancel");
  expect(document.querySelector('[role="alertdialog"]')).toBeNull();

  decision = unsavedPrompt.ask([buffer]); flushSync();
  [...document.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Закрыть без записи")!.click();
  expect(await decision).toBe("discard");
  decision = unsavedPrompt.ask([buffer]); flushSync();
  [...document.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Вернуться")!.click();
  expect(await decision).toBe("cancel");
});

it("новый вопрос отменяет прежний", async () => {
  const buffer = open("docs/a.md");
  const first = unsavedPrompt.ask([buffer]);
  const second = unsavedPrompt.ask([buffer]);
  expect(await first).toBe("cancel");
  unsavedPrompt.pending!.resolve("discard");
  expect(await second).toBe("discard");
});
