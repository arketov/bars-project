import { expect, it } from "vitest";
import recorded from "../fixtures/graph.json";
import { BarsError } from "../types";
import type { GraphReply } from "../graph";
import { auditRequests, contextRequest, digestAudit, digestContext, digestImpact, digestMeaningPaths, digestTopics, emptyImpact, entitiesRequest, entityRoots, impactRequest, meaningPathRequest, pathDocuments, replyNotes, roleShort, semanticError, topicsRequest } from "./semantic";

// Записанные ответы настоящего bars-graph на тестовых документах (браузерный макет окна).
const records = recorded as unknown as Record<string, GraphReply>;
const reply = (key: string): GraphReply => structuredClone(records[key]);
const scope = { modules: ["features"], files: [] };

it("запросы собираются по контракту v1: корень — документ, путь смысловой или со структурой, контекст по профилю и бюджету", () => {
  expect(entitiesRequest("features/a.md")).toMatchObject({ operation: "graph", view: "entities", scope: { modules: [], files: ["features/a.md"] } });
  expect(impactRequest([{ kind: "entity", id: "e1", revision: "r1" }], scope)).toMatchObject({ operation: "impact", roots: [{ kind: "entity", id: "e1", revision: "r1" }], scope, budgets: { max_depth: 3 } });
  expect(meaningPathRequest({ id: "a", revision: "1" }, { id: "b", revision: "2" }, false, scope)).toMatchObject({ operation: "paths", path_kind: "semantic", from: { id: "a" }, to: { id: "b" }, max_paths: 3 });
  expect(meaningPathRequest({ id: "a", revision: "1" }, { id: "b", revision: "2" }, true, scope)).toMatchObject({ path_kind: "mixed" });
  expect(contextRequest("  как скачать модель ", "decision", 2048, scope)).toMatchObject({ operation: "context", query: "как скачать модель", task_profile: "decision", token_budget: 2048, scope });
  expect(topicsRequest("modules", scope)).toMatchObject({ operation: "explore", analysis: "topics", whole_scope: true, grouping: "modules" });
  expect(auditRequests(scope).map((r) => r.analysis)).toEqual(["connectivity", "coverage", "conflict_candidates"]);
  expect(auditRequests(scope).every((r) => r.whole_scope === true && r.operation === "explore")).toBe(true);
});

it("влияние: прямое и косвенное разделены, документы затронутых объектов сведены к документам карты, неполный слой назван", () => {
  const raw = reply("impact");
  const root = raw.sources[0].reference.id;
  const view = digestImpact(raw, root, 2);
  expect(view.entities).toBe(2);
  const items = raw.items.filter((i) => i.type === "impact");
  expect(view.direct.length + view.indirect.length).toBe(items.length);
  expect(view.direct.every((e) => e.depth === 1) && view.indirect.every((e) => e.depth > 1)).toBe(true);
  expect(view.direct[0].steps[0]).toMatch(/зависит от|требует|использует|реализует|проверяет/);
  expect(view.documents.has(root)).toBe(false);
  expect(view.documents.size).toBeGreaterThan(0);
  for (const depth of view.documents.values()) expect(depth).toBeGreaterThan(0);
  expect(view.notes.semanticIncomplete).toBe(true);
  expect(view.notes.lines.join(" ")).toContain("Смысловой слой неполон");
  expect(view.notes.truncated).toBe(true);
  expect(view.notes.lines.join(" ")).toContain("предел глубины");
});

it("пустое влияние не превращается в «ничего не затронет»: ответ без элементов даёт пустые группы и оговорку о слое", () => {
  const raw = reply("impact");
  raw.items = [];
  raw.truncated = false;
  raw.limit_reasons = [];
  const view = digestImpact(raw, "x", 1);
  expect(view.direct).toEqual([]);
  expect(view.indirect).toEqual([]);
  expect(view.documents.size).toBe(0);
  expect(view.notes.semanticIncomplete).toBe(true);
});

it("корни влияния — сущности документа из ответа графа; документ без сущностей не запрашивает влияние и не обещает «ничего не затронет»", () => {
  const raw = reply("graph:entities");
  const roots = entityRoots(raw);
  expect(roots.length).toBe(raw.nodes.filter((n) => n.reference.kind === "entity").length);
  expect(roots.every((r) => r.kind === "entity")).toBe(true);
  expect(entityRoots({ ...raw, nodes: [] })).toEqual([]);
  const empty = emptyImpact();
  expect(empty).toMatchObject({ entities: 0, direct: [], indirect: [] });
  expect(empty.notes.lines.join(" ")).toContain("Это не значит, что правка ничего не затронет");
});

it("слой с актуальной полной полнотой не получает оговорки о неполноте, устаревший — получает предупреждение", () => {
  const raw = reply("impact");
  raw.layers = { semantic: { state: "ready", freshness: "current", completeness: "complete", availability: "enabled", generation: "g", fingerprint: "f", source_revisions: {}, coverage: { unit: "documents", processed: 1, total: 1, missing: 0, comparisons: 0, reason: null }, processed: 1, total: 1, failed: 0 } };
  raw.truncated = false;
  raw.limit_reasons = [];
  raw.warnings = [];
  expect(replyNotes(raw).lines).toEqual([]);
  raw.layers.semantic.freshness = "stale";
  expect(replyNotes(raw).lines.join(" ")).toContain("устарел");
  expect(replyNotes(raw, false).lines.join(" ")).not.toContain("неполон");
});

it("смысловой путь: шаги словами с направлением, основаниями и условиями; документы вдоль пути пронумерованы", () => {
  const raw = reply("paths");
  const view = digestMeaningPaths(raw);
  expect(view.resolution).toBe("resolved");
  expect(view.paths.length).toBe(raw.items.filter((i) => i.type === "path").length);
  const path = view.paths[0];
  expect(path.nodes.length).toBe(path.steps.length + 1);
  expect(path.steps.every((s) => s.sentence.length > 3 && !s.sentence.includes("связь не найдена"))).toBe(true);
  expect(path.steps.some((s) => s.proofs.length > 0)).toBe(true);
  const numbers = [...pathDocuments(path).values()];
  expect(numbers[0]).toBe("1");
  expect(new Set(pathDocuments(path).keys()).size).toBe(pathDocuments(path).size);
});

it("смысловой путь: неоднозначное имя даёт кандидатов без угаданного пути", () => {
  const raw = reply("paths");
  raw.items = [];
  raw.resolution = "ambiguous";
  raw.candidates = [raw.nodes[0], raw.nodes[1]];
  const view = digestMeaningPaths(raw);
  expect(view.paths).toEqual([]);
  expect(view.resolution).toBe("ambiguous");
  expect(view.candidates.length).toBe(2);
});

it("контекст: группы по ролям с цитатами, непокрытые роли названы, бюджет оценочный; метки ролей на документах", () => {
  const raw = reply("context");
  const view = digestContext(raw);
  expect(view.groups.length).toBe(raw.items.length);
  expect(view.missing).toEqual(raw.missing_roles);
  expect(view.total).toBe(view.groups.length + view.missing.length);
  expect(view.covered).toBe(view.groups.length);
  expect(view.groups[0].proofs[0].quote.length).toBeGreaterThan(0);
  expect(view.groups[0].short).toBe(roleShort(view.groups[0].role));
  expect(view.budgetMode).toBe("estimated");
  expect(view.documents.size).toBeGreaterThan(0);
  for (const badge of view.documents.values()) expect(badge.length).toBeGreaterThan(0);
  expect(roleShort("verification")).toBe("Пр");
});

it("обзор тем: номера по убыванию размера, документ → номер темы, происхождение названий сохранено", () => {
  const raw = reply("explore");
  const view = digestTopics(raw, "communities");
  expect(view.topics.length).toBe(raw.items.length);
  expect(view.topics.map((t) => t.number)).toEqual(view.topics.map((_, i) => i + 1));
  for (let i = 1; i < view.topics.length; i++) expect(view.topics[i - 1].members.length).toBeGreaterThanOrEqual(view.topics[i].members.length);
  for (const t of view.topics) for (const id of t.members) expect(view.documents.get(id)).toBeDefined();
  expect(view.method).toContain("no generated summaries");
  expect(view.topics[0].representatives.length).toBeGreaterThan(0);
});

it("аудит: разделы с оговорками, находки привязаны к документам, ошибка одного прохода не стирает остальные, хабы приходят из окна", () => {
  const connectivity = reply("explore:connectivity");
  const view = digestAudit(
    [
      { ok: true, reply: connectivity },
      { ok: false, error: "Смысловой слой ещё не построен" },
      { ok: true, reply: reply("explore:conflictcandidates") },
    ],
    [{ id: "hub-1", label: "Хаб", links: 7 }],
  );
  expect(view.sections.map((s) => s.id)).toEqual(["isolates", "unresolved", "hubs", "gaps", "conflicts"]);
  const byId = Object.fromEntries(view.sections.map((s) => [s.id, s]));
  expect(byId.isolates.findings.length).toBe(connectivity.items.filter((i) => i.type === "audit" && i.kind === "structural_isolate").length);
  expect(byId.isolates.caution).toContain("не доказана");
  expect(byId.unresolved.findings.length).toBeGreaterThan(0);
  expect(byId.unresolved.findings[0].documents.length).toBeGreaterThan(0);
  expect(byId.unresolved.findings[0].detail).toMatch(/индексе|документации/);
  expect(byId.hubs.findings).toEqual([{ documents: ["hub-1"], label: "Хаб", detail: "7 ссылок", proofs: [] }]);
  expect(byId.gaps.error).toBe("Смысловой слой ещё не построен");
  expect(byId.gaps.findings).toEqual([]);
  expect(byId.gaps.caution).toContain("Кандидат, не вердикт");
  expect(byId.conflicts.error).toBe("");
  expect(byId.conflicts.caution).toContain("не установленный дефект");
  expect(view.documents.has("hub-1")).toBe(true);
});

it("ошибки воркера словами пользователя: слой не построен, документ изменился, слой не умеет — не «ничего не найдено»", () => {
  expect(semanticError(new BarsError("x", "graph_not_ready"))).toContain("не построен");
  expect(semanticError(new BarsError("x", "graph_not_ready"))).toContain("Это не значит, что зависимостей нет");
  expect(semanticError(new BarsError("x", "stale_ref"))).toContain("Документ изменился");
  expect(semanticError(new BarsError("нужна capability", "graph_capability_unavailable"))).toContain("не поддерживает");
  expect(semanticError(new Error("сеть пропала"))).toBe("сеть пропала");
  expect(semanticError("строка")).toBe("строка");
});
