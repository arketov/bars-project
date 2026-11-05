import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import { app } from "./app.svelte";
import { call, documentRead, documentWrite } from "./api";
import { editors, type EditorBuffer } from "./editors.svelte";
import { unsavedPrompt } from "./unsaved.svelte";
import UnsavedDialog from "./UnsavedDialog.svelte";
import SearchPane from "./SearchPane.svelte";

vi.mock("./api", () => ({ call: vi.fn(), inTauri: () => false, treeRead: vi.fn(async () => ({entries:[], problems:[]})), documentRead: vi.fn(), documentWrite: vi.fn(), documentRevision: vi.fn(), joinPath: vi.fn(), openPath: vi.fn(), pickFolder: vi.fn() }));
const buffers: EditorBuffer[] = [];
let component: ReturnType<typeof mount> | undefined;
const project = (path: string) => ({path, name:path, docs_root:"docs", state:"ready" as const, modules:[]});
const settle = async () => { await new Promise(r => setTimeout(r, 0)); flushSync(); };
function dirty(project = "C:/a") {
  const buffer = editors.attach(project, "docs/a.md", "исходный");
  buffers.push(buffer); buffer.edit("черновик"); return buffer;
}
beforeEach(() => {
  vi.restoreAllMocks(); vi.mocked(call).mockReset();
  vi.mocked(call).mockResolvedValue({});
  vi.mocked(documentWrite).mockResolvedValue({status:"saved", revision:"r2"});
  app.projects = [project("C:/a"), project("C:/b")]; app.current = "C:/a"; app.viewer = null; app.status = null;
});
afterEach(async () => {
  unsavedPrompt.pending?.resolve("cancel");
  if (component) await unmount(component); component = undefined;
  buffers.splice(0).forEach(b => editors.discard(b)); document.body.replaceChildren();
});
it("смена проекта ждёт запись; буфер другого проекта не записывает", async () => {
  const a = dirty(), b = dirty("C:/b"); b.status = "conflict";
  let finish!: () => void;
  vi.mocked(documentWrite).mockImplementation(() => new Promise(resolve => { finish = () => resolve({status:"saved", revision:"r2"}); }));
  const switching = app.select("C:/b");
  expect(app.current).toBe("C:/a"); finish(); await switching;
  expect(app.current).toBe("C:/b"); expect(a.dirty).toBe(false); expect(b.dirty).toBe(true);
  expect(documentWrite).toHaveBeenCalledTimes(1);
});
it("конфликт удерживает проект и вкладку; отмена сохраняет черновик; отказ отпускает переход", async () => {
  const a = dirty(); a.status = "conflict"; a.diskRevision = "чужая";
  app.viewer = {hit:{path:a.path,title:"А",first_line:0,last_line:0},title:"А",text:a.text,missing:false};
  const viewer = app.viewer;
  let switching = app.select("C:/b"); await settle();
  expect(app.current).toBe("C:/a"); expect(app.viewer).toBe(viewer);
  expect(unsavedPrompt.pending?.allowBeside).toBe(true);
  unsavedPrompt.pending!.resolve("cancel"); await switching;
  expect(a.text).toBe("черновик"); expect(app.current).toBe("C:/a");
  switching = app.select("C:/b"); await settle(); unsavedPrompt.pending!.resolve("discard"); await switching;
  expect(app.current).toBe("C:/b"); expect(editors.get(a.project,a.path)).toBeUndefined();
});
it("ошибка копии оставляет диалог; успешная копия сохраняет черновик рядом перед сменой", async () => {
  component = mount(UnsavedDialog, {target:document.body});
  const a = dirty(); a.status = "conflict"; a.diskRevision = "чужая";
  const switching = app.select("C:/b"); await settle();
  vi.mocked(documentWrite).mockResolvedValueOnce({status:"error",kind:"io",message:"нет места"});
  await unsavedPrompt.saveBeside(); await settle();
  expect(app.current).toBe("C:/a"); expect(document.body.textContent).toContain("нет места"); expect(a.dirty).toBe(true);
  vi.mocked(documentRead).mockResolvedValue({status:"ok",text:"чужая",revision:"r3",eol:"lf",bom:false,size:5,read_only:null});
  await unsavedPrompt.saveBeside(); await switching;
  expect(app.current).toBe("C:/b"); expect(documentWrite).toHaveBeenLastCalledWith("C:/a",expect.stringContaining("(конфликт"),"черновик",null,"lf",false);
});
it("поиск проходит при конфликте своего проекта и чужом буфере; предупреждение в выдаче", async () => {
  const a = dirty(); a.status = "conflict"; a.message = "Файл изменён на диске";
  const b = dirty("C:/b"); b.status = "conflict";
  app.query = "правки";
  vi.mocked(call).mockRejectedValue(Error("модель недоступна"));
  await app.search();
  expect(vi.mocked(call).mock.calls.map(c=>c[0])).toEqual(["search"]);
  expect(app.searchUnsaved).toEqual([{path:a.path,reason:a.message}]);
  component = mount(SearchPane,{target:document.body}); flushSync();
  const alert = document.querySelector('[role="alert"]')!;
  expect(alert.textContent).toContain("Результаты по записанной версии");
  expect(alert.closest('.wrap')).not.toBeNull();
  alert.querySelector<HTMLButtonElement>('button')!.click();
  expect(app.viewer?.hit.path).toBe(a.path); expect(app.viewer?.text).toBe("черновик");
});
it("ошибка записи перед удалением или исключением проекта требует решения до команды", async () => {
  const a = dirty(); a.status = "conflict";
  const removing = app.removeProject("C:/a"); await settle();
  expect(call).not.toHaveBeenCalled(); expect(unsavedPrompt.pending?.allowBeside).toBe(false);
  unsavedPrompt.pending!.resolve("cancel"); await removing;
  app.requestDeleteProject("C:/a"); const deleting = app.confirmDeletion(); await settle();
  expect(call).not.toHaveBeenCalled(); unsavedPrompt.pending!.resolve("cancel"); await deleting;
  expect(app.deleteBusy).toBe(false); expect(a.dirty).toBe(true);
});

it("после явного отказа от записи отцепление вкладки не запускает повторную запись", async () => {
  const a = dirty();
  vi.mocked(documentWrite).mockResolvedValue({status:"error",kind:"read_only",message:"только чтение"});
  const switching = app.select("C:/b"); await settle();
  expect(unsavedPrompt.pending?.allowBeside).toBe(false);
  unsavedPrompt.pending!.resolve("discard"); await switching;
  vi.mocked(documentWrite).mockClear(); editors.detach(a);
  expect(documentWrite).not.toHaveBeenCalled();
});
