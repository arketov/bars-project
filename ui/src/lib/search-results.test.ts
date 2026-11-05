import { describe, expect, it } from "vitest";
import { dropToken, groupHits, highlight, markerFor, parseQuery, sectionChain, withoutOperators } from "./search-results";
import type { SearchHit } from "./types";

const hit = (path: string, first: number, chain = "Док/Раздел"): SearchHit => ({
  path, module: "m", title: "Док", heading_chain: chain, first_line: first, last_line: first + 3, text: "Текст", score: 0.5, score_kind: "rerank",
});

describe("карточка документа", () => {
  it("собирается группировкой по path, порядок карточек и разделов — как в выдаче", () => {
    const cards = groupHits([hit("a/x.md", 1), hit("b/y.md", 5), hit("a/x.md", 20, "Док/Другой")]);
    expect(cards.map(c => c.path)).toEqual(["a/x.md", "b/y.md"]);
    expect(cards[0].sections.map(s => s.first_line)).toEqual([1, 20]);
    expect(cards[0].folder).toBe("a");
  });
  it("цепочка заголовков не повторяет название документа", () => {
    expect(sectionChain(hit("a.md", 1, "Док/Раздел/Подраздел"))).toEqual(["Раздел", "Подраздел"]);
    expect(sectionChain(hit("a.md", 1, "Док"))).toEqual(["Док"]);
  });
});

describe("операторы запроса", () => {
  it("фраза, исключение и папка распознаются, обычные символы — нет", () => {
    const kinds = (q: string) => parseQuery(q).map(t => t.kind);
    expect(kinds('"возврат без чека" -скидка папка:инструкции')).toEqual(["phrase", "exclude", "path"]);
    expect(kinds('-"старая цена" path:"features/search"')).toEqual(["exclude", "path"]);
    expect(kinds("по-русски 274-ФЗ -5 кот-пёс")).toEqual(["word", "word", "word", "word"]);
    expect(kinds('"--" path:')).toEqual(["word", "word"]);
  });
  it("чип «×» убирает один оператор, «Без кавычек» оставляет слова фразы", () => {
    const q = '"возврат без чека" -скидка';
    const exclude = parseQuery(q).find(t => t.kind === "exclude")!;
    expect(dropToken(q, exclude)).toBe('"возврат без чека"');
    expect(withoutOperators(q, ["phrase"])).toBe("возврат без чека -скидка");
    expect(withoutOperators(q, ["exclude"])).toBe('"возврат без чека"');
  });
});

describe("подсветка слов", () => {
  const marked = (text: string, query: string, understood?: Parameters<typeof markerFor>[1]) =>
    highlight(text, markerFor(query, understood)).filter(p => p.mark).map(p => p.text);
  it("слова подсвечиваются по основе, регистр не важен", () => {
    expect(marked("Деньги ВОЗВРАЩАЮТСЯ после возврата заказа", "возврат заказа")).toEqual(["возврата", "заказа"]);
  });
  it("фраза — дословно, исключённое слово не подсвечивается", () => {
    expect(marked("возврат без чека возможен, скидка не действует", '"возврат без чека" -скидка')).toEqual(["возврат без чека"]);
  });
  it("ответ воркера важнее клиентского разбора", () => {
    expect(marked("белый кот и пёс", "белый кот -пёс", { text: "белый кот", phrases: [], excluded_words: ["пёс"] })).toEqual(["белый", "кот"]);
  });
});
