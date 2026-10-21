import { expect, it } from "vitest";
import {
  activeCount,
  ago,
  appliedParams,
  variantCheckParams,
  scoreKindChanged,
  badgeText,
  checkIsStale,
  linesLabel,
  matchesFilter,
  newRequestId,
  observationsLabel,
  outcomeLabel,
  reasonsLabel,
  scopeLabel,
  sourcesLabel,
  summarizeCheck,
  visibleTopics,
  type CheckResult,
  type TopicCheck,
  type TopicSummary,
} from "./new-topics";

const topic = (over: Partial<TopicSummary> & { id: number }): TopicSummary => ({
  revision: 1, title: `Тема ${over.id}`, status: "open", first_seen: 100, last_seen: 100, observations: 1,
  sources: ["window"], reasons: ["empty_result"], variants: 1, ...over,
});
const result = (over: Partial<CheckResult>): CheckResult => ({ variant_id: 1, scope: "project", outcome: "empty", hits: [], ...over });
const check = (results: CheckResult[], over: Partial<TopicCheck> = {}): TopicCheck => ({
  id: 1, topic_revision: 1, at: 1000, expanded: false, model: "m", params: "{}", conclusive: true, results, ...over,
});

it("фильтр «Активные» включает все незакрытые, остальные — точный статус", () => {
  const topics = [topic({ id: 1 }), topic({ id: 2, status: "materials_found" }), topic({ id: 3, status: "resolved" })];
  expect(topics.filter((t) => matchesFilter(t, "active")).map((t) => t.id)).toEqual([1, 2]);
  expect(topics.filter((t) => matchesFilter(t, "materials_found")).map((t) => t.id)).toEqual([2]);
  expect(topics.filter((t) => matchesFilter(t, "resolved")).map((t) => t.id)).toEqual([3]);
  expect(activeCount(topics)).toBe(2);
});

it("список ищет по заголовку без учёта регистра и сортирует по последнему обращению или по числу обращений", () => {
  const topics = [
    topic({ id: 1, title: "Шины бота", last_seen: 10, observations: 9 }),
    topic({ id: 2, title: "Маршрут ШИНЫ", last_seen: 30, observations: 1 }),
    topic({ id: 3, title: "Лимит", last_seen: 20, observations: 5 }),
  ];
  expect(visibleTopics(topics, "active", "шины", "recent").map((t) => t.id)).toEqual([2, 1]);
  expect(visibleTopics(topics, "active", "", "recent").map((t) => t.id)).toEqual([2, 3, 1]);
  expect(visibleTopics(topics, "active", "", "count").map((t) => t.id)).toEqual([1, 3, 2]);
});

it("метки: обращения, источники, основания, область, строки, счётчик", () => {
  expect(observationsLabel(1)).toBe("1 обращение");
  expect(observationsLabel(3)).toBe("3 обращения");
  expect(observationsLabel(11)).toBe("11 обращений");
  expect(sourcesLabel(["window", "mcp"])).toBe("Окно · MCP");
  expect(reasonsLabel(["empty_result", "manual_missing"])).toContain("Пользователь отметил");
  expect(scopeLabel("project")).toBe("Весь проект");
  expect(scopeLabel("m=features,adr;f=")).toBe("модули: features, adr");
  expect(scopeLabel("m=;f=a.md,b.md")).toBe("файлы: a.md, b.md");
  expect(linesLabel({ first_line: 5, last_line: 5 })).toBe("5");
  expect(linesLabel({ first_line: 5, last_line: 9 })).toBe("5–9");
  expect(badgeText(null)).toBe("—");
  expect(badgeText(0)).toBe("0");
});

it("давность: минуты, часы, вчера", () => {
  const now = 1_000_000_000_000;
  const sec = now / 1000;
  expect(ago(sec - 5, now)).toBe("только что");
  expect(ago(sec - 18 * 60, now)).toBe("18 мин назад");
  expect(ago(sec - 3 * 3600, now)).toBe("3 ч назад");
  expect(ago(sec - 30 * 3600, now)).toBe("вчера");
});

it("итог проверки: находка, пусто и неполнота различаются; неполная проверка не даёт вывода «не найдено»", () => {
  expect(summarizeCheck(check([result({ outcome: "found", hits: [{} as never] })])).headline).toBe("found");
  expect(summarizeCheck(check([result({ outcome: "empty" })])).headline).toBe("empty");
  expect(summarizeCheck(check([result({ outcome: "empty" }), result({ outcome: "incomplete", reason: "indexing" })])).headline).toBe("incomplete");
  // Пусто по всем вариантам, но метки индекса разошлись (conclusive=false): вывода нет.
  expect(summarizeCheck(check([result({ outcome: "empty" })], { conclusive: false })).headline).toBe("incomplete");
  // Находка в одном варианте остаётся находкой даже при неполноте другого.
  expect(summarizeCheck(check([result({ outcome: "found", hits: [{} as never] }), result({ outcome: "incomplete" })])).headline).toBe("found");
});

it("причины неполноты названы по-русски, неизвестная причина не теряется", () => {
  expect(outcomeLabel(result({ outcome: "incomplete", reason: "scope_missing" }))).toContain("область больше недоступна");
  expect(outcomeLabel(result({ outcome: "incomplete", reason: "новая" }))).toContain("новая");
  expect(outcomeLabel(result({ outcome: "incomplete" }))).toContain("причина не указана");
  expect(outcomeLabel(result({ outcome: "empty" }))).toContain("не найдены при этой проверке");
});

it("проверка устаревает только когда тема менялась после её начала; параметры читаются из JSON", () => {
  expect(checkIsStale(check([], { topic_revision: 2 }), { revision: 3 })).toBe(true);
  expect(checkIsStale(check([], { topic_revision: 3 }), { revision: 3 })).toBe(false);
  expect(appliedParams(check([], { params: '{"min_score":0.1,"results":5,"candidates":30}' }))).toBe("порог 0,10 · результатов 5 · кандидатов 30");
  expect(appliedParams(check([], { params: "не json" }))).toBe("");
});

it("ID обращения окна уникален и помечен источником", () => {
  const a = newRequestId(), b = newRequestId();
  expect(a).toMatch(/^window:/);
  expect(a).not.toBe(b);
});

it("различает исходный rerank и cosine, показывает параметры и сбой каждого варианта", () => {
  const original = '{"score_kind":"rerank","min_score":0.18}';
  const current = check([], { params: JSON.stringify({ score_kind: "cosine", min_score: 0.24, variants: [
    { variant_id: 1, applied: { score_kind: "cosine", min_score: 0.24 }, rerank_error: "ошибка модели" },
    { variant_id: 2, applied: { score_kind: "rerank", min_score: 0.18 }, rerank_error: null },
  ] }) });
  const first = variantCheckParams(current, 1), second = variantCheckParams(current, 2);
  expect(scoreKindChanged(original, first.params)).toBe(true);
  expect(scoreKindChanged(original, second.params)).toBe(false);
  expect(first.rerankError).toBe("ошибка модели");
  expect(second.rerankError).toBeNull();
  expect(variantCheckParams(current, 3).params).toBe("{}");
  expect(appliedParams({ ...current, params: original })).toBe("оценка rerank · порог 0,18");
  expect(variantCheckParams(check([], { params: original }), 1).params).toBe(original);
  expect(scoreKindChanged("не json", first.params)).toBe(false);
  expect(outcomeLabel(result({ outcome: "incomplete", reason: "rerank_unavailable" }))).toContain("rerank недоступен");
});
