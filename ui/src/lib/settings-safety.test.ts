import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import SettingsPage from "./SettingsPage.svelte";
import { app } from "./app.svelte";
import { call } from "./api";
import { mockCall } from "./mock";
import { clone } from "./settings-logic";
import { settings } from "./settings.svelte";
import type { SettingsOutput, SettingsSaveOutput } from "./types";
vi.mock("./api", async original => ({ ...await original<typeof import("./api")>(), call: vi.fn() }));
let component: ReturnType<typeof mount> | undefined;
let disk: SettingsOutput;
const settle = async () => { for (let i = 0; i < 12; i++) { await Promise.resolve(); flushSync(); } };
beforeEach(async () => {
  disk = await mockCall("settings_get", {}) as SettingsOutput;
  disk.settings.search.results = 5; disk.settings.search.candidates = 30; disk.revision = "base";
  settings.saved = settings.draft = null; settings.conflict = null; settings.conflicts = [];
  settings.loadError = null; settings.saving = settings.loading = settings.leaveOpen = false;
  app.settingsLeave = null; app.view = "settings"; app.settingsTab = "search";
  vi.mocked(call).mockImplementation(async (method, params) => {
    if (method === "settings_get") return clone(disk) as never;
    return await mockCall(method, params ?? {}) as never;
  });
});
afterEach(async () => {
  if (component) await unmount(component); component = undefined;
  settings.saved = settings.draft = null; settings.conflict = null; settings.conflicts = [];
  settings.loading = settings.saving = settings.leaveOpen = false;
  app.settingsLeave = null; app.view = "main";
  document.body.replaceChildren(); vi.mocked(call).mockReset();
});
async function render() { component = mount(SettingsPage, { target: document.body }); await settle(); }
function button(label: string) { return [...document.querySelectorAll("button")].find(node => node.textContent.trim() === label)!; }
function reply(saved: boolean): SettingsSaveOutput { return { ...clone(disk), status: saved ? "saved" : "conflict", conflicts: saved ? [] : [{ key: "search.results", base: 5, disk: disk.settings.search.results, mine: 9 }] }; }

it("пауза в окне: умолчание 15 секунд и явно сохранённые 30", async () => {
  app.settingsTab = "indexing";
  await render();
  expect((document.querySelector("#edit-pause") as HTMLInputElement).value).toBe("15");
  expect(settings.dirty).toBe(false);
  await unmount(component!); component = undefined;
  disk.settings.indexing.edit_pause_seconds = 30;
  await render();
  expect((document.querySelector("#edit-pause") as HTMLInputElement).value).toBe("30");
  expect(settings.dirty).toBe(false);
});

it("повторное открытие перечитывает настройки; фокус с грязным черновиком сохраняет независимые правки", async () => {
  await render(); expect(settings.draft!.search.results).toBe(5);
  await unmount(component!); component = undefined;
  disk.settings.search.results = 7; disk.revision = "outside";
  await render(); expect(settings.draft!.search.results).toBe(7);
  settings.draft!.search.candidates = 42;
  disk.settings.search.results = 8; disk.revision = "new";
  window.dispatchEvent(new Event("focus")); await settle();
  expect(settings.draft!.search.results).toBe(8); expect(settings.draft!.search.candidates).toBe(42);
  expect(settings.saved!.search.candidates).toBe(30); expect(settings.revision).toBe("new");
});

it("сохранение отправляет только изменённые поля и принимает слитый ответ", async () => {
  await render(); settings.draft!.search.results = 9;
  disk.settings.search.results = 9; disk.settings.search.candidates = 40; disk.revision = "merged";
  vi.mocked(call).mockResolvedValueOnce(reply(true));
  expect(await settings.save()).toBe(true);
  expect(call).toHaveBeenCalledWith("settings_save", expect.objectContaining({ expected_revision: "base", changes: { "search.results": 9 }, base: expect.objectContaining({ search: expect.objectContaining({ results: 5 }) }) }));
  expect(settings.saved!.search.candidates).toBe(40); expect(settings.dirty).toBe(false);
});

it("взять конфликт с диска сохраняет остальные мои поля без автоматической записи", async () => {
  await render(); settings.draft!.search.results = 9; settings.draft!.search.candidates = 42;
  disk.settings.search.results = 8; disk.revision = "outside";
  window.dispatchEvent(new Event("focus")); await settle();
  expect(document.body.textContent).toContain("settings.toml изменён вне окна");
  vi.mocked(call).mockClear(); button("Взять с диска").click(); await settle();
  expect(settings.draft!.search.results).toBe(8); expect(settings.draft!.search.candidates).toBe(42);
  expect(settings.saved!.search.candidates).toBe(30); expect(settings.revision).toBe("outside");
  expect(call).not.toHaveBeenCalled();
});

it("оставить моё повторяет сохранение с новой ревизией; новый конфликт остаётся видимым", async () => {
  await render(); settings.draft!.search.results = 9;
  disk.settings.search.results = 8; disk.revision = "outside";
  vi.mocked(call).mockResolvedValueOnce(reply(false));
  expect(await settings.save()).toBe(false); await settle();
  disk.settings.search.results = 7; disk.revision = "again";
  vi.mocked(call).mockResolvedValueOnce(reply(false));
  button("Оставить моё").click(); await settle();
  expect(call).toHaveBeenLastCalledWith("settings_save", expect.objectContaining({ expected_revision: "outside", changes: { "search.results": 9 } }));
  expect(settings.conflict!.revision).toBe("again"); expect(settings.draft!.search.results).toBe(9);
});

it("уход: остаться, ошибка сохранения, конфликт, не сохранять", async () => {
  await render(); settings.draft!.search.results = 9;
  app.openIndexing(); await settle(); expect(app.view).toBe("settings");
  button("Остаться").click(); await settle(); expect(app.view).toBe("settings"); expect(settings.dirty).toBe(true);
  app.openSearch(); await settle();
  vi.mocked(call).mockRejectedValueOnce(new Error("только чтение"));
  await settings.finishLeave("save"); await settle(); expect(app.view).toBe("settings"); expect(settings.draft!.search.results).toBe(9);
  app.openSearch(); await settle(); disk.settings.search.results = 8; disk.revision = "outside";
  vi.mocked(call).mockResolvedValueOnce(reply(false));
  await settings.finishLeave("save"); await settle(); expect(app.view).toBe("settings"); expect(settings.conflict).not.toBeNull();
  app.openSearch(); await settle(); button("Не сохранять").click(); await settle();
  expect(app.view).toBe("main"); expect(settings.dirty).toBe(false);
});

it("успешное сохранение при уходе открывает назначенный раздел", async () => {
  await render(); settings.draft!.search.results = 9;
  app.openIndexing(); await settle(); disk.settings.search.results = 9;
  vi.mocked(call).mockResolvedValueOnce(reply(true));
  await settings.finishLeave("save"); await settle(); expect(app.view).toBe("indexing"); expect(settings.dirty).toBe(false);
});

it("ошибка чтения оставляет черновик и запрещает запись устаревших значений", async () => {
  await render(); settings.draft!.search.results = 9;
  vi.mocked(call).mockRejectedValueOnce(new Error("битый TOML"));
  await settings.load(); await settle();
  expect(settings.draft!.search.results).toBe(9); expect(settings.loadError).toBe("битый TOML");
  expect(await settings.save()).toBe(false); expect(button("Сохранить").disabled).toBe(true);
});

it("пустое числовое поле переживает проверку фокуса и остаётся доступным для исправления", async () => {
  await render(); settings.draft!.search.results = undefined as unknown as number;
  window.dispatchEvent(new Event("focus")); await settle();
  expect(settings.loadError).toBeNull(); expect(settings.draft!.search.results).toBeUndefined();
  expect(document.querySelector("fieldset")!.disabled).toBe(false);
  expect(button("Сохранить").disabled).toBe(true);
});

it("возврат прежней модели использует ревизию и сохраняет несвязанные правки черновика", async () => {
  await render(); settings.draft!.search.results = 9;
  vi.spyOn(app, "recheck").mockResolvedValueOnce(undefined);
  const output = reply(true); output.settings.embed.model = "BAAI/bge-m3:fp16";
  vi.mocked(call).mockResolvedValueOnce(output);
  await settings.revertModel("builtin:BAAI/bge-m3:fp16");
  expect(call).toHaveBeenCalledWith("settings_save", expect.objectContaining({ expected_revision: "base", changes: { "embed.model": "BAAI/bge-m3:fp16" } }));
  expect(settings.draft!.search.results).toBe(9); expect(settings.saved!.search.results).toBe(5);
  expect(settings.saving).toBe(false); vi.mocked(app.recheck).mockRestore();
});
