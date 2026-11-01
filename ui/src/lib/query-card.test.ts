import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import SearchPane from "./SearchPane.svelte";
import { app } from "./app.svelte";
import type { TextOutput } from "./text-search.svelte";

let component: ReturnType<typeof mount>;
const empty = (extra: Partial<TextOutput> = {}): TextOutput => ({ documents: [], docs_total: 0, hits_total: 0, skipped: [], truncated: null, elapsed_ms: 1, ...extra });
const summary = () => document.querySelector(".query-parameters summary")!;
const button = (name: string) => [...document.querySelectorAll("button")].find(b => b.textContent?.trim() === name)!;
beforeEach(() => {
  app.projects = [{ path: "C:/p", name: "Проект", docs_root: "docs", state: "ready", modules: [] }];
  app.current = "C:/p"; app.tree = []; app.selected = new Set(); app.query = "запрос";
  app.results = null; app.searchedQuery = ""; app.searching = false; app.searchError = null; app.searchUnsaved = [];
  app.retrievalMode = "hybrid"; app.includeRelated = true; app.count = null;
  app.textSearch.reset(); app.textSearch.caseSensitive = false; app.textSearch.regex = false; app.textSearch.pathFilter = "";
  component = mount(SearchPane, { target: document.body }); flushSync();
});
afterEach(async () => { await unmount(component); document.body.replaceChildren(); vi.restoreAllMocks(); });

it("параметры свёрнуты; скрытые отличия видны, настройки не запускают поиск и сохраняют выдачу", () => {
  const search = vi.spyOn(app, "search").mockResolvedValue(undefined);
  expect(document.querySelector<HTMLDetailsElement>(".query-parameters")!.open).toBe(false);
  expect(summary().textContent).toBe("Параметры");
  app.textSearch.output = empty();
  app.retrievalMode = "mix"; app.includeRelated = false; app.count = 2;
  document.querySelector<HTMLButtonElement>('[aria-label="Учитывать регистр"]')!.click();
  document.querySelector<HTMLButtonElement>('[aria-label="Регулярное выражение"]')!.click();
  const path = document.querySelector<HTMLInputElement>('[aria-label="Фильтр пути текстового поиска"]')!;
  path.value = "features/очень-длинный-путь/*.md"; path.dispatchEvent(new Event("input", { bubbles: true })); flushSync();
  expect(summary().textContent).toBe("Параметры · Со связями · без связанных источников · 2 рез. · регистр · выражение · путь");
  expect(app.textSearch.output).not.toBeNull(); expect(search).not.toHaveBeenCalled();
  button("по умолчанию").click(); flushSync();
  expect(app.count).toBeNull(); expect(app.query).toBe("запрос"); expect(app.textSearch.pathFilter).toContain("features/");
  expect(summary().textContent).not.toContain("2 рез.");
});

it("Enter запускает поиск из запроса и пути, IME и изменение параметров не запускают", () => {
  const search = vi.spyOn(app, "search").mockResolvedValue(undefined);
  for (const input of document.querySelectorAll("textarea, .text-options input")) {
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, isComposing: true }));
    expect(search).not.toHaveBeenCalled();
  }
  for (const input of document.querySelectorAll("textarea, .text-options input")) input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  expect(search).toHaveBeenCalledTimes(2);
});

it("полный пустой текстовый итог под смысловым, включая ошибку модели; новый запуск убирает старый итог", () => {
  expect(document.querySelector(".text-empty")).toBeNull();
  app.searchedQuery = "запрос"; app.textSearch.output = empty(); flushSync();
  const note = document.querySelector(".text-empty")!;
  expect(note.textContent).toBe("В тексте: вхождений не найдено");
  expect(document.querySelector(".text-results")).toBeNull();
  expect(document.querySelector(".semantic-title")!.compareDocumentPosition(note) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  app.searchError = { kind: "model", message: "Модель недоступна" } as never; flushSync();
  expect(document.querySelector(".text-empty")).not.toBeNull();
  app.textSearch.reset(); app.textSearch.loading = true; flushSync();
  expect(document.querySelector(".text-empty")).toBeNull();
  expect(document.querySelector(".text-results")!.textContent).toContain("Ищем в файлах");
});

it.each(["skipped", "truncated", "error", "loading"])("не прячет отдельный раздел текста при %s", kind => {
  app.textSearch.output = empty(kind === "skipped" ? { skipped: [{ path: "a.md", reason: "нет доступа" }] } : kind === "truncated" ? { truncated: "time" } : {});
  if (kind === "error") app.textSearch.error = "Ошибка выражения";
  if (kind === "loading") app.textSearch.loading = true;
  flushSync();
  expect(document.querySelector(".text-empty")).toBeNull();
  expect(document.querySelector(".text-results")).not.toBeNull();
  expect(document.querySelector(".text-results")!.textContent).not.toContain("Вхождений в файлах не найдено");
});

it("переиндексация не заменяет подпись области", () => {
  app.projects[0].state = "reindex_required"; flushSync();
  expect(document.querySelector(".scope")!.textContent).toContain("всём проекте");
  expect(document.querySelector<HTMLSelectElement>('[aria-label="Режим поиска"]')!.disabled).toBe(true);
  expect(button("Найти").disabled).toBe(false);
});
