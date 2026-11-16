import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import SearchPane from "./SearchPane.svelte";
import { app } from "./app.svelte";
import type { SearchHit, SearchOutput } from "./types";

let component: ReturnType<typeof mount>;
const button = (name: string) => [...document.querySelectorAll("button")].find(b => b.textContent?.trim() === name)!;
const hit = (path: string, first: number, chain: string): SearchHit => ({
  path, module: "m", title: "Документ", heading_chain: chain, first_line: first, last_line: first + 4,
  text: "Возврат без чека возможен", score: 0.8, score_kind: "rerank",
});
const output = (results: SearchHit[], over: Partial<SearchOutput> = {}): SearchOutput => ({
  results, applied: { results: 5, candidates: 30, rerank_candidates: 15, min_score: 0.1, score_kind: "rerank", clamped: false },
  indexing: false, pending: 0, ...over,
});

beforeEach(() => {
  app.projects = [{ path: "C:/p", name: "Проект", docs_root: "docs", state: "ready", modules: [] }];
  app.current = "C:/p"; app.tree = []; app.selected = new Set(); app.query = "возврат";
  app.results = null; app.searchedQuery = ""; app.searching = false; app.searchError = null; app.searchUnsaved = [];
  app.waitingModel = false; app.count = null; app.showScores = false;
  component = mount(SearchPane, { target: document.body }); flushSync();
});
afterEach(async () => { await unmount(component); document.body.replaceChildren(); vi.restoreAllMocks(); });

it("в поиске нет режимов, «В тексте», регистра, выражения и фильтра пути", () => {
  expect(document.querySelector('[aria-label="Режим поиска"]')).toBeNull();
  expect(document.querySelector('[aria-label="Учитывать регистр"]')).toBeNull();
  expect(document.querySelector('[aria-label="Регулярное выражение"]')).toBeNull();
  expect(document.querySelector('[aria-label="Фильтр пути текстового поиска"]')).toBeNull();
  expect(document.body.textContent).not.toContain("В тексте");
});

it("операторы: «Понято как» с чипами, «×» убирает оператор и ищет заново, «?» открывает таблицу", () => {
  const search = vi.spyOn(app, "search").mockResolvedValue(undefined);
  app.query = '"возврат без чека" -скидка папка:инструкции'; flushSync();
  const meta = document.querySelector(".qmeta")!;
  expect(meta.textContent).toContain("Понято как");
  expect([...meta.querySelectorAll(".chip")].map(c => c.className.replace("chip ", "").replace(/\s*svelte-\w+/g, ""))).toEqual(["phrase", "exclude", "path"]);
  meta.querySelectorAll<HTMLButtonElement>(".chip button")[1].click(); flushSync();
  expect(app.query).toBe('"возврат без чека" папка:инструкции'); expect(search).toHaveBeenCalled();
  app.query = "возврат"; flushSync();
  expect(document.querySelector(".qmeta .chip")).toBeNull();
  document.querySelector<HTMLButtonElement>('[aria-label="Как уточнить запрос"]')!.click(); flushSync();
  expect(document.querySelector(".pop")!.textContent).toContain("папка:инструкции");
});

it("выдача — карточки документов, итог «документа, разделов», баллы скрыты до переключателя и запоминаются", () => {
  app.results = output([hit("a.md", 1, "Документ/Сроки"), hit("a.md", 20, "Документ/Документы"), hit("b.md", 3, "Документ/Другое")]);
  app.searchedQuery = "возврат"; flushSync();
  expect(document.querySelectorAll("article.doc")).toHaveLength(2);
  expect(document.querySelector(".summary b")!.textContent).toBe("2 документа, 3 раздела");
  expect(document.querySelector(".score")).toBeNull();
  expect(document.querySelector(".tech")).toBeNull();
  const toggle = document.querySelector<HTMLInputElement>(".toggle input")!;
  toggle.click(); flushSync();
  expect(app.showScores).toBe(true);
  expect(document.querySelector(".score")).not.toBeNull();
  expect(document.querySelector(".tech")!.textContent).toContain("порог 0,10");
  expect(localStorage.getItem("bars-search-scores")).toBe("1");
});

it("score_kind none: оценка не рисуется, предупреждение «только по словам» с повтором", () => {
  const search = vi.spyOn(app, "search").mockResolvedValue(undefined);
  app.showScores = true;
  app.results = output([{ ...hit("a.md", 1, "Документ/Сроки"), score_kind: "none", score: 0 }], {
    applied: { results: 5, candidates: 30, rerank_candidates: 15, min_score: 0, score_kind: "none", clamped: false },
    semantic_error: "модель загружается",
  });
  app.searchedQuery = "возврат"; flushSync();
  expect(document.body.textContent).toContain("Результаты только по словам запроса");
  expect(document.querySelector(".score")!.textContent).toBe("—");
  button("Повторить с моделью").click(); expect(search).toHaveBeenCalled();
});

it("ожидание модели: баннер с кнопкой «Искать по словам сейчас»", () => {
  const words = vi.spyOn(app, "searchWords").mockResolvedValue(undefined);
  app.searching = true; app.waitingModel = true; flushSync();
  expect(document.body.textContent).toContain("Загружается модель поиска");
  button("Искать по словам сейчас").click(); expect(words).toHaveBeenCalled();
});

it("пусто: кнопки только тех операторов, что есть в запросе", () => {
  const as = vi.spyOn(app, "searchAs").mockResolvedValue(undefined);
  app.results = output([]); app.searchedQuery = '"возврат без чека" -скидка'; flushSync();
  expect(document.querySelector(".empty h1")!.textContent).toBe("Ничего не нашлось");
  button("Без кавычек").click();
  expect(as).toHaveBeenCalledWith("возврат без чека -скидка");
  button("Без исключения").click();
  expect(as).toHaveBeenLastCalledWith('"возврат без чека"');
  expect([...document.querySelectorAll(".empty .acts button")].map(b => b.textContent)).toEqual(["Без кавычек", "Без исключения"]);
});

it("Enter запускает поиск, IME не запускает", () => {
  const search = vi.spyOn(app, "search").mockResolvedValue(undefined);
  const field = document.querySelector("textarea")!;
  field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, isComposing: true }));
  expect(search).not.toHaveBeenCalled();
  field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  expect(search).toHaveBeenCalledTimes(1);
});

it("переиндексация не заменяет подпись области", () => {
  app.projects[0].state = "reindex_required"; flushSync();
  expect(document.querySelector(".scope")!.textContent).toContain("всём проекте");
  expect(button("Найти").disabled).toBe(false);
});
