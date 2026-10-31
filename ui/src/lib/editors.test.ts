import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AUTOSAVE_MS, EditorBuffer, READ_ONLY_MESSAGES } from "./editors.svelte";
import { documentRead, documentRevision, documentWrite, type DocumentRead, type DocumentWrite } from "./api";
import { editors } from "./editors.svelte";

vi.mock("./api", () => ({ documentRead: vi.fn(), documentRevision: vi.fn(), documentWrite: vi.fn(), inTauri: () => true }));

const read = (patch: Partial<DocumentRead> = {}): DocumentRead => ({
  status: "ok", text: "# а\n", revision: "r1", eol: "lf", bom: false, size: 5, read_only: null, ...patch,
});
const saved = (revision: string): DocumentWrite => ({ status: "saved", revision });

async function loaded(patch: Partial<DocumentRead> = {}): Promise<EditorBuffer> {
  vi.mocked(documentRead).mockResolvedValue(read(patch));
  const buffer = new EditorBuffer("C:/p", "docs/a.md", "");
  await buffer.load();
  return buffer;
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.mocked(documentRead).mockReset();
  vi.mocked(documentWrite).mockReset();
  vi.mocked(documentRevision).mockReset();
});
afterEach(() => vi.useRealTimers());

it("перемещение сохраняет буфер и записывает по новому пути без захвата соседнего префикса", async () => {
  vi.mocked(documentRead).mockResolvedValue(read());
  const buffer = editors.attach("move-project", "docs/a/x.md", "");
  const neighbor = editors.attach("move-project", "docs/ab/x.md", "");
  await buffer.load(); await neighbor.load();
  editors.movePaths("move-project", "docs/a", "docs/b");
  expect(editors.get("move-project", "docs/a/x.md")).toBeUndefined();
  expect(editors.get("move-project", "docs/b/x.md")).toBe(buffer);
  expect(neighbor.path).toBe("docs/ab/x.md");
  buffer.edit("Новая правка");
  vi.mocked(documentWrite).mockResolvedValue(saved("r2"));
  await buffer.save();
  expect(vi.mocked(documentWrite).mock.calls.at(-1)?.slice(0,2)).toEqual(["move-project","docs/b/x.md"]);
  editors.discard(buffer); editors.discard(neighbor);
});

it("чтение берёт текст, ревизию, перевод строк и BOM с диска", async () => {
  const buffer = await loaded({ eol: "crlf", bom: true });
  expect([buffer.text, buffer.revision, buffer.eol, buffer.bom, buffer.status]).toEqual(["# а\n", "r1", "crlf", true, "clean"]);
});

it("документ только для чтения не принимает правки и объясняет причину", async () => {
  const buffer = await loaded({ read_only: "not_utf8" });
  buffer.edit("другое");
  expect(buffer.status).toBe("readonly");
  expect(buffer.text).toBe("# а\n");
  expect(buffer.message).toBe(READ_ONLY_MESSAGES.not_utf8);
});

it("запись начинается через секунду после последней правки, а не после первой", async () => {
  const buffer = await loaded();
  vi.mocked(documentWrite).mockResolvedValue(saved("r2"));
  buffer.edit("# а\n1");
  await vi.advanceTimersByTimeAsync(AUTOSAVE_MS - 100);
  buffer.edit("# а\n12");
  await vi.advanceTimersByTimeAsync(AUTOSAVE_MS - 100);
  expect(documentWrite).not.toHaveBeenCalled();
  expect(buffer.status).toBe("dirty");
  await vi.advanceTimersByTimeAsync(200);
  expect(documentWrite).toHaveBeenCalledTimes(1);
  expect(documentWrite).toHaveBeenCalledWith("C:/p", "docs/a.md", "# а\n12", "r1", "lf", false);
  expect(buffer.status).toBe("saved");
  expect(buffer.revision).toBe("r2");
});

it("правка во время записи не создаёт вторую запись одновременно и пишется следующей с новой ревизией", async () => {
  const buffer = await loaded();
  let finish!: (value: DocumentWrite) => void;
  vi.mocked(documentWrite).mockImplementationOnce(() => new Promise((done) => { finish = done; }));
  vi.mocked(documentWrite).mockResolvedValueOnce(saved("r3"));
  buffer.edit("первая");
  await vi.advanceTimersByTimeAsync(AUTOSAVE_MS);
  expect(buffer.status).toBe("saving");
  buffer.edit("вторая");
  await vi.advanceTimersByTimeAsync(AUTOSAVE_MS * 2);
  expect(documentWrite).toHaveBeenCalledTimes(1);
  finish(saved("r2"));
  await vi.advanceTimersByTimeAsync(0);
  expect(documentWrite).toHaveBeenCalledTimes(2);
  expect(vi.mocked(documentWrite).mock.calls[1].slice(2, 4)).toEqual(["вторая", "r2"]);
  expect(buffer.status).toBe("saved");
  expect(buffer.revision).toBe("r3");
});

it("конфликт не перезаписывает файл, останавливает запись и запоминает диск", async () => {
  const buffer = await loaded();
  vi.mocked(documentWrite).mockResolvedValue({ status: "conflict", revision: "внешняя" });
  buffer.edit("моя версия");
  await vi.advanceTimersByTimeAsync(AUTOSAVE_MS);
  expect([buffer.status, buffer.diskRevision, buffer.text]).toEqual(["conflict", "внешняя", "моя версия"]);
  buffer.edit("ещё правка");
  await vi.advanceTimersByTimeAsync(AUTOSAVE_MS * 3);
  expect(documentWrite).toHaveBeenCalledTimes(1);
  expect(buffer.status).toBe("conflict");
  expect(buffer.text).toBe("ещё правка");
});

it("запись, которой файл не нашла, переводит буфер в «файл удалён»", async () => {
  const buffer = await loaded();
  vi.mocked(documentWrite).mockResolvedValue({ status: "conflict", revision: null });
  buffer.edit("x");
  await vi.advanceTimersByTimeAsync(AUTOSAVE_MS);
  expect([buffer.status, buffer.message, buffer.text]).toEqual(["deleted", "Файл удалён с диска.", "x"]);
});

it("ошибка записи оставляет черновик и повторяется кнопкой, а не по таймеру", async () => {
  const buffer = await loaded();
  vi.mocked(documentWrite).mockResolvedValueOnce({ status: "error", kind: "locked", message: "Файл занят другой программой" });
  buffer.edit("черновик");
  await vi.advanceTimersByTimeAsync(AUTOSAVE_MS);
  expect([buffer.status, buffer.message, buffer.text]).toEqual(["error", "Файл занят другой программой", "черновик"]);
  await vi.advanceTimersByTimeAsync(AUTOSAVE_MS * 5);
  expect(documentWrite).toHaveBeenCalledTimes(1);
  vi.mocked(documentWrite).mockResolvedValueOnce(saved("r2"));
  await buffer.retry();
  expect([buffer.status, buffer.message]).toEqual(["saved", ""]);
  expect(buffer.dirty).toBe(false);
});

it("save() записывает сразу и без правок ничего не делает", async () => {
  const buffer = await loaded();
  await buffer.save();
  expect(documentWrite).not.toHaveBeenCalled();
  vi.mocked(documentWrite).mockResolvedValue(saved("r2"));
  buffer.edit("сразу");
  await buffer.save();
  expect(documentWrite).toHaveBeenCalledTimes(1);
});

it("возврат к записанному тексту снимает отметку изменения без записи", async () => {
  const buffer = await loaded();
  buffer.edit("лишнее");
  buffer.edit("# а\n");
  expect(buffer.status).toBe("clean");
  await vi.advanceTimersByTimeAsync(AUTOSAVE_MS * 2);
  expect(documentWrite).not.toHaveBeenCalled();
});

const disk = (revision: string | null) => vi.mocked(documentRevision).mockResolvedValue({ status: "ok", revision });

it("проверка диска: тот же файл ничего не меняет", async () => {
  const buffer = await loaded();
  disk("r1");
  await buffer.checkDisk();
  expect([buffer.status, buffer.notice]).toEqual(["clean", ""]);
  expect(documentRead).toHaveBeenCalledTimes(1);
});

it("проверка диска: чистый буфер молча перечитывается, курсорный текст заменяется", async () => {
  const buffer = await loaded();
  disk("r2");
  vi.mocked(documentRead).mockResolvedValue(read({ text: "внешний текст", revision: "r2" }));
  await buffer.checkDisk();
  expect([buffer.text, buffer.revision, buffer.status, buffer.notice]).toEqual(["внешний текст", "r2", "clean", "Обновлено с диска"]);
});

it("проверка диска: буфер с правками уходит в конфликт, ничего не записывается и не теряется", async () => {
  const buffer = await loaded();
  buffer.edit("моя правка");
  disk("r2");
  await buffer.checkDisk();
  expect([buffer.status, buffer.diskRevision, buffer.text]).toEqual(["conflict", "r2", "моя правка"]);
  await vi.advanceTimersByTimeAsync(AUTOSAVE_MS * 2);
  expect(documentWrite).not.toHaveBeenCalled();
});

it("проверка диска: удалённый файл при правках — «файл удалён», правка цела", async () => {
  const buffer = await loaded();
  buffer.edit("моя правка");
  disk(null);
  await buffer.checkDisk();
  expect([buffer.status, buffer.message, buffer.text, buffer.dirty]).toEqual(["deleted", "Файл удалён с диска.", "моя правка", true]);
});

it("проверка диска: удалённый файл при чистом буфере не стирает текст (M-05)", async () => {
  const buffer = await loaded();
  disk(null);
  await buffer.checkDisk();
  expect([buffer.status, buffer.text, buffer.dirty, buffer.notice]).toEqual(["deleted", "# а\n", false, ""]);
  // Новое чтение отсутствующего файла тоже не опустошает документ.
  vi.mocked(documentRead).mockResolvedValue(read({ text: "", revision: null }));
  await buffer.load();
  expect([buffer.status, buffer.text]).toEqual(["deleted", "# а\n"]);
});

it("в состоянии «файл удалён» правка не запускает автосохранение, файл не воскресает молча", async () => {
  const buffer = await loaded();
  disk(null);
  await buffer.checkDisk();
  buffer.edit("набранный символ");
  await vi.advanceTimersByTimeAsync(AUTOSAVE_MS * 3);
  expect(documentWrite).not.toHaveBeenCalled();
  expect([buffer.status, buffer.text]).toEqual(["deleted", "набранный символ"]);
  expect(await editors.flush()).toEqual([]);
});

it("«Записать заново» создаёт файл с base_revision = null и делает буфер чистым, даже без правок", async () => {
  const buffer = await loaded();
  disk(null);
  await buffer.checkDisk();
  vi.mocked(documentWrite).mockResolvedValueOnce(saved("r7"));
  await buffer.restore();
  expect(documentWrite).toHaveBeenCalledWith("C:/p", "docs/a.md", "# а\n", null, "lf", false);
  expect([buffer.status, buffer.revision, buffer.dirty]).toEqual(["saved", "r7", false]);
});

it("«Записать заново»: ошибка записи оставляет повтор рабочим для чистого буфера", async () => {
  const buffer = await loaded();
  disk(null);
  await buffer.checkDisk();
  vi.mocked(documentWrite).mockResolvedValueOnce({ status: "error", kind: "parent_missing", message: "Папки документа нет" });
  await buffer.restore();
  expect([buffer.status, buffer.message]).toEqual(["error", "Папки документа нет"]);
  vi.mocked(documentWrite).mockResolvedValueOnce(saved("r8"));
  await buffer.retry();
  expect([buffer.status, buffer.revision]).toEqual(["saved", "r8"]);
});

it("файл вернулся на место: чистый буфер перечитывается, буфер с правками уходит в конфликт", async () => {
  const buffer = await loaded();
  disk(null);
  await buffer.checkDisk();
  vi.mocked(documentRead).mockResolvedValue(read({ text: "вернулся", revision: "r1" }));
  disk("r1");
  await buffer.checkDisk();
  expect([buffer.status, buffer.text]).toEqual(["clean", "вернулся"]);
  const edited = await loaded();
  edited.edit("моя");
  disk(null);
  await edited.checkDisk();
  disk("r5");
  await edited.checkDisk();
  expect([edited.status, edited.diskRevision]).toEqual(["conflict", "r5"]);
});

it("удаление в корзину bars: чистые буферы возвращаются на закрытие вкладок, изменённые получают «файл удалён»", async () => {
  vi.mocked(documentRead).mockResolvedValue(read());
  const clean = editors.attach("trash-p", "docs/t/clean.md", "");
  const dirty = editors.attach("trash-p", "docs/t/dirty.md", "");
  const other = editors.attach("trash-p", "docs/tt/other.md", "");
  await vi.advanceTimersByTimeAsync(0);
  dirty.edit("правка");
  expect(editors.trashed("trash-p", "docs/t").sort()).toEqual(["docs/t/clean.md"]);
  expect([clean.status, dirty.status, dirty.text, other.status]).toEqual(["deleted", "deleted", "правка", "clean"]);
  [clean, dirty, other].forEach((buffer) => editors.discard(buffer));
});

it("«Оставить мою версию» пишет поверх ревизии, которую увидел конфликт", async () => {
  const buffer = await loaded();
  vi.mocked(documentWrite).mockResolvedValueOnce({ status: "conflict", revision: "внешняя" });
  buffer.edit("моя");
  await vi.advanceTimersByTimeAsync(AUTOSAVE_MS);
  vi.mocked(documentWrite).mockResolvedValueOnce(saved("r9"));
  await buffer.keepMine();
  expect(vi.mocked(documentWrite).mock.calls[1].slice(2, 4)).toEqual(["моя", "внешняя"]);
  expect([buffer.status, buffer.revision, buffer.message]).toEqual(["saved", "r9", ""]);
});

it("«Взять версию с диска» заменяет черновик", async () => {
  const buffer = await loaded();
  vi.mocked(documentWrite).mockResolvedValueOnce({ status: "conflict", revision: "внешняя" });
  buffer.edit("моя");
  await vi.advanceTimersByTimeAsync(AUTOSAVE_MS);
  vi.mocked(documentRead).mockResolvedValue(read({ text: "диск", revision: "внешняя" }));
  await buffer.takeDisk();
  expect([buffer.text, buffer.status, buffer.dirty, buffer.notice]).toEqual(["диск", "clean", false, "Загружена версия с диска"]);
});

it("«Сохранить мою рядом» пишет копию с новым именем в той же папке и берёт диск", async () => {
  const buffer = await loaded();
  vi.mocked(documentWrite).mockResolvedValueOnce({ status: "conflict", revision: "внешняя" });
  buffer.edit("моя");
  await vi.advanceTimersByTimeAsync(AUTOSAVE_MS);
  vi.mocked(documentWrite).mockResolvedValueOnce({ status: "conflict", revision: "занято" }).mockResolvedValueOnce(saved("копия"));
  vi.mocked(documentRead).mockResolvedValue(read({ text: "диск", revision: "внешняя" }));
  await buffer.saveBeside();
  const [first, second] = [vi.mocked(documentWrite).mock.calls[1], vi.mocked(documentWrite).mock.calls[2]];
  expect(first[1]).toMatch(/^docs\/a \(конфликт \d{6}\)\.md$/);
  expect(second[1]).toMatch(/^docs\/a \(конфликт \d{6}-2\)\.md$/);
  expect([first[2], first[3]]).toEqual(["моя", null]);
  expect([buffer.text, buffer.status]).toEqual(["диск", "clean"]);
  expect(buffer.notice).toContain("сохранена рядом");
});

it("ошибка копии рядом оставляет конфликт и текст", async () => {
  const buffer = await loaded();
  vi.mocked(documentWrite).mockResolvedValueOnce({ status: "conflict", revision: "внешняя" });
  buffer.edit("моя");
  await vi.advanceTimersByTimeAsync(AUTOSAVE_MS);
  vi.mocked(documentWrite).mockResolvedValueOnce({ status: "error", kind: "locked", message: "занят" });
  await buffer.saveBeside();
  expect([buffer.status, buffer.text]).toEqual(["conflict", "моя"]);
  expect(buffer.message).toContain("занят");
});

it("flush записывает правки всех буферов и возвращает те, что записать не удалось", async () => {
  vi.mocked(documentRead).mockResolvedValue(read());
  const good = editors.attach("C:/p", "docs/good.md", "");
  const bad = editors.attach("C:/p", "docs/bad.md", "");
  await vi.advanceTimersByTimeAsync(0);
  vi.mocked(documentWrite).mockImplementation(async (_project, path) => (path === "docs/bad.md" ? { status: "conflict", revision: "x" } : saved("ok")));
  good.edit("а");
  bad.edit("б");
  const rest = await editors.flush();
  expect(rest.map((buffer) => buffer.path)).toEqual(["docs/bad.md"]);
  expect(good.status).toBe("saved");
  editors.discard(good);
  editors.discard(bad);
});

it("закрытие последнего вида с правками дописывает их в фоне и не теряет", async () => {
  vi.mocked(documentRead).mockResolvedValue(read());
  const buffer = editors.attach("C:/p", "docs/close.md", "");
  await vi.advanceTimersByTimeAsync(0);
  vi.mocked(documentWrite).mockResolvedValue(saved("r2"));
  buffer.edit("правка");
  editors.detach(buffer);
  await vi.advanceTimersByTimeAsync(0);
  expect(documentWrite).toHaveBeenCalledTimes(1);
  expect(editors.get("C:/p", "docs/close.md")).toBe(buffer);
  editors.discard(buffer);
});
