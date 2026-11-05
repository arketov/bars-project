import { afterEach, beforeEach, expect, it } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import SearchPane from "./SearchPane.svelte";
import { app } from "./app.svelte";
import type { SearchHit, SearchOutput } from "./types";

// ISS-59 на карточке выдачи (labels.md): пользователь видит слова, идентификаторы протокола — только в подсказках.
let component: ReturnType<typeof mount>;

const hit: SearchHit = {
  path: "docs/instructions/refunds.md", module: "instructions", title: "Возврат средств", heading_chain: "Возврат средств/Сроки",
  first_line: 3, last_line: 9, text: "Возврат без чека возможен в течение двух недель.",
  score: 0.7, score_kind: "cosine", signals: { cosine: 0.62, fulltext: 1, exact: 0.5 },
};
const output = (over: Partial<SearchOutput> = {}): SearchOutput => ({
  results: [hit],
  applied: { results: 5, candidates: 30, rerank_candidates: 15, min_score: 0.1, score_kind: "cosine", clamped: false, signals: ["cosine", "fulltext", "exact"] },
  indexing: false, pending: 0, ...over,
});
/** Служебные слова протокола, которых нет в видимом тексте выдачи. «(rerank)» — пояснение в разборе баллов («технические подробности»). */
const CODES = /include_related|score_kind|semantic_error|rerank_error|chunk_id|heading_chain|\bmix\b|\bhybrid\b|\bcosine\b|\bfulltext\b|\bexact\b|\brerank\b/;
const visible = () => (document.body.textContent ?? "").replace("оценка модели (rerank)", "оценка модели");

beforeEach(() => {
  app.projects = [{ path: "C:/проект", name: "Проект", docs_root: "docs", state: "ready", modules: [] }];
  app.current = "C:/проект"; app.tree = []; app.selected = new Set(); app.query = "возврат без чека";
  app.results = null; app.searchedQuery = "возврат без чека"; app.searching = false; app.searchError = null; app.searchUnsaved = [];
  app.waitingModel = false; app.count = null; app.showScores = false;
  component = mount(SearchPane, { target: document.body }); flushSync();
});
afterEach(async () => {
  await unmount(component); document.body.replaceChildren();
  app.results = null; app.current = null; app.projects = [];
});

it("карточка показывает название, папку и раздел словами, путь остаётся в подсказке", () => {
  app.results = output(); flushSync();
  const card = document.querySelector("article.doc")!;
  expect(card.querySelector("h3")!.textContent).toContain("Возврат средств");
  expect(card.querySelector(".dpath")!.textContent).toBe("docs/instructions");
  expect(card.textContent).toContain("Сроки");
  expect(card.textContent).not.toContain("refunds.md");
  expect(card.querySelector('[title="docs/instructions/refunds.md"]')).not.toBeNull();
  expect(visible()).not.toMatch(CODES);
});

it("разбор баллов называет сигналы словами и не показывает коды протокола", () => {
  app.showScores = true; app.results = output(); flushSync();
  const text = visible();
  expect(text).toContain("близость по смыслу");
  expect(text).toContain("слова запроса");
  expect(text).toContain("точное совпадение");
  expect(text).not.toMatch(CODES);
  expect(document.querySelector('[title="Оценка лучшего раздела"]')).not.toBeNull();
});

it("пометка о недоступной модели — словами, код причины не попадает в текст", () => {
  app.results = output({ semantic_error: "future_fallback=1" }); flushSync();
  expect(visible()).toContain("Результаты только по словам запроса");
  expect(visible()).not.toContain("future_fallback");
  expect(visible()).not.toMatch(CODES);
});

it("без опорного сигнала оценка заменена прочерком с подсказкой, а не названием вида", () => {
  app.showScores = true;
  app.results = output({
    results: [{ ...hit, score: 0, score_kind: "none", signals: { fulltext: 1, exact: 0 } }],
    applied: { results: 5, candidates: 30, rerank_candidates: 15, min_score: 0, score_kind: "none", clamped: false, signals: ["fulltext", "exact"] },
  }); flushSync();
  expect(document.querySelector('.score[title="Оценки нет: нет опорного сигнала"]')!.textContent).toBe("—");
  expect(visible()).toContain("без опоры");
  expect(visible()).not.toMatch(/\bnone\b|score_kind/);
});
