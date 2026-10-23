// Стенд 2.3.1: уровни карты и операции работы с графом над записанным снимком (build-fixture.mjs).
// Это иллюстрация дизайна, а не продукт: путь по карте и локальный граф в продукте тоже считает клиент,
// а смысловой путь, влияние, контекст, обзор и аудит — воркер (`graph_query`); здесь они повторены по снимку.
import fixtureJson from "./fixture.json";
import type { GEdge, GNode, LayerState } from "../../lib/graph/model";

type Doc = [title: string, path: string, docsPath: string, module: string, chunks: number];
type Chunk = [doc: number, first: number, last: number, heading: string[], roles: string[]];
type Ev = [doc: number, first: number, last: number, quote: string, method: string];
export interface Entity { label: string; doc: number; aliases: string[]; evidence: string[] }
export interface Assertion { from: number; to: number; predicate: string; polarity: "positive" | "negative"; qualifiers: Record<string, string>; origin: string; evidence: string[] }
export interface ContextReply { id: string; query: string; profile: string; groups: { role: string; evidence: string[] }[]; missing: string[]; used: number; budget: number | null; mode: string }
export interface Fixture {
  name: string;
  captured: string;
  method: string;
  snapshot: string;
  semantic_state: string;
  warnings: string[];
  docs: Doc[];
  chunks: Chunk[];
  links: [number, number, number[]][];
  chunkLinks: [number, number, number[]][];
  docSims: [number, number, number][];
  chunkSims: [number, number, number][];
  entities: Entity[];
  mentions: [number, number, string[]][];
  assertions: Assertion[];
  broken: [number, string, number, string][];
  topics: { label: string; members: number[]; representatives: number[] }[];
  audit: { kind: string; doc: number | null; detail: string; evidence: string[] }[];
  context: ContextReply[];
  evidence: Record<string, Ev>;
}

export const F = fixtureJson as unknown as Fixture;

export type Level = "documents" | "chunks" | "entities" | "mixed";
export const LEVELS: Level[] = ["documents", "chunks", "entities", "mixed"];
export const LEVEL_NAME: Record<Level, string> = { documents: "Документы", chunks: "Чанки", entities: "Сущности", mixed: "Смешанный" };

/** Виды рёбер стенда сверх трёх видов Т-46 и связи запроса. */
export type XKind = "markdown_link" | "document_similarity" | "chunk_similarity" | "query_result" | "assertion" | "mention" | "unresolved" | "sequence" | "aggregate";
export type XNodeKind = "document" | "chunk" | "query" | "entity" | "ghost" | "topic";

export const XKIND_TITLE: Record<XKind, string> = {
  markdown_link: "Ссылка",
  document_similarity: "Близость документов",
  chunk_similarity: "Близость чанков",
  query_result: "Связь с запросом",
  assertion: "Утверждение",
  mention: "Упоминание",
  unresolved: "Неразрешённая ссылка",
  sequence: "Порядок чанков",
  aggregate: "Связи между темами",
};

export const PREDICATE_GROUP: Record<string, "structural" | "mention" | "semantic" | "derived"> = {
  contains: "structural", covers: "structural", next_chunk: "structural", links_to: "structural",
  mentions: "mention",
  depends_on: "semantic", uses: "semantic", implements: "semantic", verifies: "semantic", supersedes: "semantic", requires: "semantic",
  similar_to: "derived", co_occurs: "derived", member_of: "derived",
};
export const GROUP_NAME = { structural: "Структурные", mention: "Упоминания", semantic: "Смысловые", derived: "Производные" };
export const PREDICATE_NAME: Record<string, string> = {
  depends_on: "зависит от", uses: "использует", implements: "реализует", verifies: "проверяет", supersedes: "заменяет", requires: "требует",
  links_to: "ссылается на", mentions: "упоминает", similar_to: "похоже на", co_occurs: "встречается с",
};
/** Предикаты зависимости: «A предикат B» — изменение B затрагивает A (tools.md, bars_impact). */
const DEPENDS = new Set(["depends_on", "uses", "implements", "verifies", "requires"]);

export const ROLE_LETTER: Record<string, string> = { requirement: "Т", decision: "Р", behavior: "П", architecture: "А", constraint: "О", verification: "Пр" };
export const ROLE_NAME: Record<string, string> = { requirement: "Требования", decision: "Решения", behavior: "Поведение", architecture: "Архитектура", constraint: "Ограничения", verification: "Проверки" };

// ---------- Сценарии состояний GR-05

export type Scenario = "" | "empty" | "notbuilt" | "nolinks" | "lagging" | "fileerrors" | "unresolved" | "ambiguous" | "conflict" | "conditions" | "limit" | "stale" | "deleted" | "norender";
export const SCENARIOS: Scenario[] = ["empty", "notbuilt", "nolinks", "lagging", "fileerrors", "unresolved", "ambiguous", "conflict", "conditions", "limit", "stale", "deleted", "norender"];

const docIdx = (part: string) => F.docs.findIndex((d) => d[2].toLowerCase().includes(part.toLowerCase()) || d[0].toLowerCase().includes(part.toLowerCase()));
/** Объекты, на которые сценарии накладывают состояние (настоящие документы и утверждения снимка). */
export const SCENARIO_TARGETS = {
  stale: ["features/search/hybrid-search", "features/indexing/updates", "manual/search"].map(docIdx).filter((i) => i >= 0),
  deleted: [docIdx("features/window/notifications")].filter((i) => i >= 0),
  fileErrors: [["features/models/local-models.md", "файл занят другим процессом"], ["knowledge/inference-2026-10-18/Index.md", "не удалось прочитать UTF-8 в строке 212"], ["backlog/ideas.md", "превышен предел извлечения 2 с"]] as [string, string][],
  conflict: [F.assertions.findIndex((a) => a.predicate === "uses" && F.entities[a.from].label === "bars"), F.assertions.findIndex((a) => a.predicate === "requires" && F.entities[a.to].label.startsWith("исключительного"))].filter((i) => i >= 0),
  condition: F.assertions.findIndex((a) => F.entities[a.from].label.startsWith("Без драйвера Vulkan")),
};

// ---------- Модель уровня

export interface XNode extends Omit<GNode, "kind"> {
  kind: XNodeKind;
  /** Номер в массиве снимка: документ, чанк, сущность, тема или неразрешённая ссылка. */
  ref: number;
  heading?: string[];
  roles?: string[];
  ambiguous?: boolean;
  stale?: boolean;
  deleted?: boolean;
  members?: number;
}

export interface XEdge extends Omit<GEdge, "kind"> {
  kind: XKind;
  /** Утверждение снимка (уровни сущностей). */
  assertion?: number;
  /** Упоминание: основания. */
  evidence?: string[];
  /** Агрегированное ребро тем: число исходных связей по видам. */
  count?: Record<string, number>;
  conflict?: boolean;
}

/** Общее хранилище узлов стенда: индекс узла устойчив между уровнями, поэтому выбор и места переживают переключение. */
export class XStore {
  nodes: XNode[] = [];
  #byId = new Map<string, number>();
  indexOf(id: string): number | undefined {
    return this.#byId.get(id);
  }
  upsert(n: Omit<XNode, "index" | "owner"> & { owner?: number }): number {
    const known = this.#byId.get(n.id);
    const index = known ?? this.nodes.length;
    this.nodes[index] = { ...n, index, owner: n.owner ?? index } as XNode;
    this.#byId.set(n.id, index);
    return index;
  }
}

export interface LevelSlice {
  visible: number[];
  edges: XEdge[];
  /** Группы документов уровня «Чанки»: подпись и члены. */
  hulls: { doc: number; label: string; members: number[] }[];
  totals: { nodes: number; shown: number };
  truncated: boolean;
  limitReasons: string[];
  layers: { links: LayerState; meaning: LayerState; semantic: LayerState };
  banner: string[];
}

export const CHUNK_LIMIT = 500;
export const CHUNK_MAX = 2000;

export interface LevelOptions {
  level: Level;
  expanded: Record<number, number>;
  scenario: Scenario;
  chunkLimit: number;
  showUnresolved: boolean;
  /** Видимые темы обзора: индексы раскрытых тем, остальные свёрнуты; null — обзор выключен. */
  topics: Set<number> | null;
}

const docId = (i: number) => `doc:${i}`;
const chunkId = (i: number) => `chunk:${i}`;
const entId = (i: number) => `ent:${i}`;

export function docNode(store: XStore, i: number, scenario: Scenario): number {
  const d = F.docs[i];
  return store.upsert({
    id: docId(i), kind: "document", ref: i, label: d[0], path: d[1], docsPath: d[2], module: d[3], chunks: d[4], ordinal: null, firstLine: 0, lastLine: 0,
    stale: scenario === "stale" && SCENARIO_TARGETS.stale.includes(i),
    deleted: scenario === "deleted" && SCENARIO_TARGETS.deleted.includes(i),
  });
}

function chunkNode(store: XStore, i: number, scenario: Scenario): number {
  const c = F.chunks[i];
  const owner = docNode(store, c[0], scenario);
  const ordinal = F.chunks.slice(0, i).filter((x) => x[0] === c[0]).length;
  return store.upsert({
    id: chunkId(i), kind: "chunk", ref: i, owner, label: c[3][c[3].length - 1]?.split("/").pop() ?? `Чанк ${ordinal + 1}`, path: F.docs[c[0]][1], docsPath: F.docs[c[0]][2], module: F.docs[c[0]][3], chunks: 0, ordinal,
    firstLine: c[1], lastLine: c[2], heading: c[3], roles: c[4],
  });
}

function entityNode(store: XStore, i: number, scenario: Scenario): number {
  const e = F.entities[i];
  const doc = e.doc >= 0 ? F.docs[e.doc] : null;
  return store.upsert({
    id: entId(i), kind: "entity", ref: i, label: e.label, path: doc?.[1] ?? "", docsPath: doc?.[2] ?? "", module: doc?.[3] ?? "", chunks: 0, ordinal: null, firstLine: 0, lastLine: 0,
    ambiguous: scenario === "ambiguous" && (e.label === "MCP" || e.label === "bars"),
  });
}

const chunksOfDoc = (doc: number) => F.chunks.map((c, i) => [c, i] as const).filter(([c]) => c[0] === doc).map(([, i]) => i);

/** Срез уровня: узлы, рёбра всех видов уровня (режим фильтрует панель), группы, усечение и состояния слоёв. */
export function buildLevel(store: XStore, o: LevelOptions): LevelSlice {
  const sc = o.scenario;
  const visible: number[] = [];
  const edges: XEdge[] = [];
  const hulls: LevelSlice["hulls"] = [];
  const banner: string[] = [];
  const add = (e: Omit<XEdge, "id" | "key">) => edges.push({ ...e, id: edges.length, key: `${e.kind}:${e.a}:${e.b}` } as XEdge);
  const docs = sc === "empty" ? [] : F.docs.map((_, i) => i);
  const noLinks = sc === "nolinks";
  let truncated = false;
  const reasons: string[] = [];
  let total = 0;

  const semantic: LayerState = sc === "lagging" ? "partial" : sc === "notbuilt" ? "not_built" : "partial";
  const layers = { links: (sc === "notbuilt" ? "not_built" : "ready") as LayerState, meaning: (sc === "notbuilt" ? "not_built" : "ready") as LayerState, semantic };

  if (o.topics) {
    // Обзор тем: свёрнутые темы — узлы, раскрытые — их документы; рёбра агрегированы по видам.
    const owner = new Map<number, number>();
    F.topics.forEach((t, ti) => {
      if (o.topics!.has(ti)) for (const d of t.members) owner.set(d, docNode(store, d, sc));
      else {
        const index = store.upsert({ id: `topic:${ti}`, kind: "topic", ref: ti, label: t.label.split(" / ")[0], path: "", docsPath: "", module: "", chunks: 0, ordinal: null, firstLine: 0, lastLine: 0, members: t.members.length });
        for (const d of t.members) owner.set(d, index);
      }
    });
    visible.push(...new Set(owner.values()));
    const agg = new Map<string, Record<string, number>>();
    const bump = (a: number, b: number, kind: string) => {
      const x = owner.get(a);
      const y = owner.get(b);
      if (x === undefined || y === undefined || x === y) return;
      const key = x < y ? `${x}:${y}` : `${y}:${x}`;
      const rec = agg.get(key) ?? {};
      rec[kind] = (rec[kind] ?? 0) + 1;
      agg.set(key, rec);
    };
    for (const [a, b] of F.links) bump(a, b, "markdown_link");
    for (const [a, b] of F.docSims) bump(a, b, "document_similarity");
    for (const [key, count] of agg) {
      const [a, b] = key.split(":").map(Number);
      const both = store.nodes[a].kind === "document" && store.nodes[b].kind === "document";
      const n = Object.values(count).reduce((x, y) => x + y, 0);
      if (both) {
        // Между двумя раскрытыми документами — исходные рёбра, а не агрегат.
        const la = F.links.find(([x, y]) => (docId(x) === store.nodes[a].id && docId(y) === store.nodes[b].id) || (docId(y) === store.nodes[a].id && docId(x) === store.nodes[b].id));
        if (la) add({ kind: "markdown_link", a: store.indexOf(docId(la[0]))!, b: store.indexOf(docId(la[1]))!, raw: null, s: 1, lines: la[2] });
        continue;
      }
      add({ kind: "aggregate", a, b, raw: null, s: Math.min(1, 0.3 + n / 30), lines: [], count });
    }
    return { visible, edges, hulls, totals: { nodes: visible.length, shown: visible.length }, truncated: false, limitReasons: [], layers, banner };
  }

  if (o.level === "documents" || o.level === "mixed") {
    for (const d of docs) visible.push(docNode(store, d, sc));
    if (!noLinks) {
      for (const [a, b, lines] of F.links) add({ kind: "markdown_link", a: docNode(store, a, sc), b: docNode(store, b, sc), raw: null, s: 1, lines });
      for (const [a, b, cos] of F.docSims) add({ kind: "document_similarity", a: docNode(store, a, sc), b: docNode(store, b, sc), raw: cos, s: cos, lines: [] });
    }
    if (o.level === "documents") {
      const shown = new Set<number>();
      for (const [doc, count] of Object.entries(o.expanded)) {
        const docIndex = store.nodes[Number(doc)]?.ref;
        if (docIndex === undefined) continue;
        for (const ci of chunksOfDoc(docIndex).slice(0, count)) {
          visible.push(chunkNode(store, ci, sc));
          shown.add(ci);
        }
      }
      for (const [a, b, cos] of F.chunkSims) if (shown.has(a) && shown.has(b) && !noLinks) add({ kind: "chunk_similarity", a: chunkNode(store, a, sc), b: chunkNode(store, b, sc), raw: cos, s: cos, lines: [] });
    }
    if (o.showUnresolved || sc === "unresolved") {
      F.broken.forEach(([doc, target, line, reason], j) => {
        if (reason !== "target_not_indexed" && reason !== "anchor_missing" && reason !== "target_missing") return;
        const ghost = store.upsert({ id: `ghost:${target}`, kind: "ghost", ref: j, label: target, path: target, docsPath: target, module: "", chunks: 0, ordinal: null, firstLine: line, lastLine: line });
        if (!visible.includes(ghost)) visible.push(ghost);
        add({ kind: "unresolved", a: docNode(store, doc, sc), b: ghost, raw: null, s: 1, lines: [line] });
      });
    }
  }

  if (o.level === "chunks") {
    const all = docs.flatMap((d) => chunksOfDoc(d));
    total = all.length;
    const take = all.slice(0, o.chunkLimit);
    if (take.length < all.length) {
      truncated = true;
      reasons.push(`чанков показано ${take.length} из ${all.length}`);
    }
    const shown = new Set(take);
    for (const ci of take) visible.push(chunkNode(store, ci, sc));
    for (const d of docs) {
      const members = chunksOfDoc(d).filter((ci) => shown.has(ci));
      if (!members.length) continue;
      hulls.push({ doc: docNode(store, d, sc), label: F.docs[d][0], members: members.map((ci) => store.indexOf(chunkId(ci))!) });
      for (let k = 1; k < members.length; k++) add({ kind: "sequence", a: store.indexOf(chunkId(members[k - 1]))!, b: store.indexOf(chunkId(members[k]))!, raw: null, s: 1, lines: [] });
    }
    if (!noLinks) {
      for (const [a, b, lines] of F.chunkLinks) if (shown.has(a) && shown.has(b) && a !== b) add({ kind: "markdown_link", a: chunkNode(store, a, sc), b: chunkNode(store, b, sc), raw: null, s: 1, lines });
      for (const [a, b, cos] of F.chunkSims) if (shown.has(a) && shown.has(b)) add({ kind: "chunk_similarity", a: chunkNode(store, a, sc), b: chunkNode(store, b, sc), raw: cos, s: cos, lines: [] });
    }
  }

  if (o.level === "entities" || o.level === "mixed") {
    if (sc !== "notbuilt" && sc !== "empty") {
      const ents = F.entities.map((_, i) => i);
      for (const i of ents) visible.push(entityNode(store, i, sc));
      F.assertions.forEach((a, k) => {
        add({ kind: "assertion", a: entityNode(store, a.from, sc), b: entityNode(store, a.to, sc), raw: null, s: 0.8, lines: [], assertion: k, conflict: sc === "conflict" && SCENARIO_TARGETS.conflict.includes(k) });
      });
      if (o.level === "mixed") for (const [doc, ent, ev] of F.mentions) add({ kind: "mention", a: docNode(store, doc, sc), b: entityNode(store, ent, sc), raw: null, s: 0.3, lines: [], evidence: ev });
    }
    banner.push(
      sc === "lagging"
        ? "Извлечение отстаёт: обработано 41 из 66 документов, отношения остальных ещё не извлечены — результаты частичные."
        : `Слой сущностей экспериментальный: извлечение неполное (${F.entities.length} сущностей, ${F.assertions.length} утверждений, метод ${F.method}). Отсутствие связи не доказывает её отсутствие.`,
    );
  }

  if (sc === "fileerrors") banner.push(`Не обработано ${SCENARIO_TARGETS.fileErrors.length} файла: список — в инспекторе без выбора.`);
  if (!total) total = visible.length;
  return { visible, edges, hulls, totals: { nodes: total, shown: visible.length }, truncated, limitReasons: reasons, layers, banner };
}

// ---------- Операции над видимым графом (клиент)

export interface Adj {
  /** Соседи узла: ребро, сосед, ход по направлению ребра. */
  of(n: number): { edge: XEdge; other: number; forward: boolean }[];
}

export function adjacency(edges: XEdge[], directed = false): Adj {
  const map = new Map<number, { edge: XEdge; other: number; forward: boolean }[]>();
  const push = (n: number, v: { edge: XEdge; other: number; forward: boolean }) => map.set(n, [...(map.get(n) ?? []), v]);
  for (const e of edges) {
    push(e.a, { edge: e, other: e.b, forward: true });
    const oriented = e.kind === "markdown_link" || e.kind === "assertion" || e.kind === "unresolved";
    if (!directed || !oriented) push(e.b, { edge: e, other: e.a, forward: false });
  }
  return { of: (n) => map.get(n) ?? [] };
}

export interface MapPath {
  nodes: number[];
  steps: { edge: XEdge; forward: boolean }[];
  /** Сумма (1 − s) по шагам: при равной длине выше путь с более сильными связями. */
  cost: number;
}

/** Кратчайший путь по числу шагов с разрывом ничьих по силе, без запрещённых узлов и рёбер (для алгоритма Йена). */
function shortest(adj: Adj, from: number, to: number, maxHops: number, bannedNodes: Set<number>, bannedEdges: Set<number>): MapPath | null {
  type St = { node: number; hops: number; cost: number; prev: St | null; edge: XEdge | null; forward: boolean };
  const best = new Map<number, St>();
  let frontier: St[] = [{ node: from, hops: 0, cost: 0, prev: null, edge: null, forward: true }];
  best.set(from, frontier[0]);
  for (let hop = 0; hop < maxHops && frontier.length; hop++) {
    const next: St[] = [];
    for (const st of frontier) {
      for (const { edge, other, forward } of adj.of(st.node)) {
        if (bannedNodes.has(other) || bannedEdges.has(edge.id)) continue;
        const cost = st.cost + (1 - Math.min(1, edge.s));
        const known = best.get(other);
        if (known && (known.hops < hop + 1 || (known.hops === hop + 1 && known.cost <= cost))) continue;
        const ns: St = { node: other, hops: hop + 1, cost, prev: st, edge, forward };
        best.set(other, ns);
        next.push(ns);
      }
    }
    if (best.has(to)) break;
    frontier = next.filter((s) => best.get(s.node) === s);
  }
  const end = best.get(to);
  if (!end || to === from) return null;
  const nodes: number[] = [];
  const steps: MapPath["steps"] = [];
  for (let s: St | null = end; s; s = s.prev) {
    nodes.unshift(s.node);
    if (s.edge) steps.unshift({ edge: s.edge, forward: s.forward });
  }
  return { nodes, steps, cost: end.cost };
}

/** До `k` кратчайших простых путей (алгоритм Йена), не длиннее `maxHops` шагов. */
export function mapPaths(adj: Adj, from: number, to: number, k = 3, maxHops = 6): MapPath[] {
  const first = shortest(adj, from, to, maxHops, new Set(), new Set());
  if (!first) return [];
  const found: MapPath[] = [first];
  const candidates: MapPath[] = [];
  const keyOf = (p: MapPath) => p.steps.map((s) => s.edge.id).join(",");
  for (let i = 1; i < k; i++) {
    const last = found[i - 1];
    for (let j = 0; j < last.nodes.length - 1; j++) {
      const spur = last.nodes[j];
      const root = last.steps.slice(0, j);
      const bannedEdges = new Set<number>();
      for (const p of found) if (p.steps.slice(0, j).map((s) => s.edge.id).join(",") === root.map((s) => s.edge.id).join(",") && p.steps[j]) bannedEdges.add(p.steps[j].edge.id);
      const bannedNodes = new Set(last.nodes.slice(0, j));
      const tail = shortest(adj, spur, to, maxHops - j, bannedNodes, bannedEdges);
      if (!tail) continue;
      const path: MapPath = { nodes: [...last.nodes.slice(0, j), ...tail.nodes], steps: [...root, ...tail.steps], cost: root.reduce((c, s) => c + 1 - Math.min(1, s.edge.s), 0) + tail.cost };
      if (path.steps.length <= maxHops && !candidates.some((c) => keyOf(c) === keyOf(path)) && !found.some((c) => keyOf(c) === keyOf(path))) candidates.push(path);
    }
    if (!candidates.length) break;
    candidates.sort((a, b) => a.steps.length - b.steps.length || a.cost - b.cost);
    found.push(candidates.shift()!);
  }
  return found;
}

export interface LocalResult {
  depthOf: Map<number, number>;
  edges: Set<number>;
  known: number;
  truncated: boolean;
  /** Следующий шаг глубины упрётся в предел. */
  limited: boolean;
}

/** Локальный граф: обход в ширину до глубины `depth` с пределом узлов; ссылки в обе стороны. */
export function localGraph(adj: Adj, center: number, depth: number, maxNodes: number): LocalResult {
  const depthOf = new Map<number, number>([[center, 0]]);
  let frontier = [center];
  let known = 1;
  let truncated = false;
  for (let d = 1; d <= depth && frontier.length; d++) {
    const next: number[] = [];
    for (const n of frontier)
      for (const { other } of adj.of(n)) {
        if (depthOf.has(other) || next.includes(other)) continue;
        known++;
        if (depthOf.size + next.length >= maxNodes) {
          truncated = true;
          continue;
        }
        next.push(other);
      }
    for (const n of next) depthOf.set(n, d);
    frontier = next;
  }
  const edges = new Set<number>();
  for (const n of depthOf.keys()) for (const { edge, other } of adj.of(n)) if (depthOf.has(other)) edges.add(edge.id);
  return { depthOf, edges, known, truncated, limited: truncated };
}

// ---------- Операции воркера, повторённые по снимку

export interface SemStep {
  kind: "assertion" | "mention" | "link";
  from: string;
  to: string;
  label: string;
  predicate: string;
  negative: boolean;
  reversed: boolean;
  qualifiers: Record<string, string>;
  evidence: string[];
  lines: number[];
}
export interface SemPath { nodes: string[]; steps: SemStep[] }
export interface SemResult { paths: SemPath[]; reason: string; searched: string; ambiguous: string[] }

const entLabel = (i: number) => F.entities[i].label;

/** Смысловой путь `bars_paths`: semantic — только утверждения; mixed — ещё ссылки и упоминания. Концы — документы или сущности. */
export function semanticPaths(from: string, to: string, kind: "semantic" | "mixed", maxHops = 4): SemResult {
  const searched = `глубина ${maxHops}, до 3 путей, смысловой слой «частично» (${F.method})`;
  type E = { a: string; b: string; step: Omit<SemStep, "from" | "to" | "reversed"> };
  const list: E[] = [];
  F.assertions.forEach((a) => list.push({ a: entId(a.from), b: entId(a.to), step: { kind: "assertion", label: `${entLabel(a.from)} ${a.polarity === "negative" ? "не " : ""}${PREDICATE_NAME[a.predicate] ?? a.predicate} ${entLabel(a.to)}`, predicate: a.predicate, negative: a.polarity === "negative", qualifiers: a.qualifiers, evidence: a.evidence, lines: [] } }));
  if (kind === "mixed") {
    for (const [doc, ent, ev] of F.mentions) list.push({ a: docId(doc), b: entId(ent), step: { kind: "mention", label: `«${F.docs[doc][0]}» упоминает «${entLabel(ent)}»`, predicate: "mentions", negative: false, qualifiers: {}, evidence: ev, lines: [] } });
    for (const [a, b, lines] of F.links) list.push({ a: docId(a), b: docId(b), step: { kind: "link", label: `«${F.docs[a][0]}» ссылается на «${F.docs[b][0]}»`, predicate: "links_to", negative: false, qualifiers: {}, evidence: [], lines } });
  } else {
    // Документ-конец в semantic: семена — сущности, определённые или упомянутые в документе.
    for (const [doc, ent] of F.mentions) list.push({ a: docId(doc), b: entId(ent), step: { kind: "mention", label: "", predicate: "seed", negative: false, qualifiers: {}, evidence: [], lines: [] } });
  }
  const adj = new Map<string, { other: string; e: E; reversed: boolean }[]>();
  for (const e of list) {
    adj.set(e.a, [...(adj.get(e.a) ?? []), { other: e.b, e, reversed: false }]);
    adj.set(e.b, [...(adj.get(e.b) ?? []), { other: e.a, e, reversed: true }]);
  }
  const paths: SemPath[] = [];
  const queue: { node: string; nodes: string[]; steps: SemStep[] }[] = [{ node: from, nodes: [from], steps: [] }];
  while (queue.length && paths.length < 3) {
    const cur = queue.shift()!;
    if (cur.steps.length >= maxHops) continue;
    for (const { other, e, reversed } of adj.get(cur.node) ?? []) {
      if (cur.nodes.includes(other)) continue;
      const step: SemStep = { ...e.step, from: reversed ? e.b : e.a, to: reversed ? e.a : e.b, reversed };
      const next = { node: other, nodes: [...cur.nodes, other], steps: [...cur.steps, step] };
      if (other === to) {
        // Шаги-семена semantic-режима не показываются как связь.
        if (kind === "semantic" && next.steps.filter((s) => s.predicate !== "seed").length === 0) continue;
        paths.push({ nodes: next.nodes, steps: next.steps.filter((s) => s.predicate !== "seed") });
        if (paths.length >= 3) break;
      } else queue.push(next);
    }
  }
  return {
    paths,
    reason: paths.length ? "" : kind === "semantic" ? "Смысловой путь не найден. В снимке нет двух последовательных смысловых утверждений; отсутствие пути не доказывает отсутствие связи." : "Путь не найден в пределах глубины.",
    searched,
    ambiguous: [],
  };
}

export interface ImpactItem { id: string; label: string; direct: boolean; depth: number; path: SemStep[] }
export interface ImpactResult { direct: ImpactItem[]; indirect: ImpactItem[]; neighbours: { doc: number; incoming: boolean; lines: number[] }[]; note: string }

/** Влияние `bars_impact`: обратный обход зависимостей до глубины 3; ссылки-соседи — отдельно, не зависимость. */
export function impactOf(root: string, maxDepth = 3): ImpactResult {
  const seeds = new Set<string>([root]);
  if (root.startsWith("doc:")) {
    const d = Number(root.slice(4));
    F.entities.forEach((e, i) => e.doc === d && seeds.add(entId(i)));
    for (const [doc, ent] of F.mentions) if (doc === d) seeds.add(entId(ent));
  }
  const seen = new Map<string, ImpactItem>();
  let frontier = [...seeds].map((id) => ({ id, path: [] as SemStep[] }));
  for (let depth = 1; depth <= maxDepth && frontier.length; depth++) {
    const next: typeof frontier = [];
    for (const cur of frontier)
      F.assertions.forEach((a) => {
        if (!DEPENDS.has(a.predicate) || a.polarity === "negative" || entId(a.to) !== cur.id) return;
        const id = entId(a.from);
        if (seeds.has(id) || seen.has(id)) return;
        const step: SemStep = { kind: "assertion", from: id, to: cur.id, label: `${entLabel(a.from)} ${PREDICATE_NAME[a.predicate] ?? a.predicate} ${entLabel(a.to)}`, predicate: a.predicate, negative: false, reversed: true, qualifiers: a.qualifiers, evidence: a.evidence, lines: [] };
        const item = { id, label: entLabel(a.from), direct: depth === 1, depth, path: [...cur.path, step] };
        seen.set(id, item);
        next.push({ id, path: item.path });
      });
    frontier = next;
  }
  const neighbours: ImpactResult["neighbours"] = [];
  if (root.startsWith("doc:")) {
    const d = Number(root.slice(4));
    for (const [a, b, lines] of F.links) {
      if (b === d) neighbours.push({ doc: a, incoming: true, lines });
      else if (a === d) neighbours.push({ doc: b, incoming: false, lines });
    }
  }
  const items = [...seen.values()];
  return {
    direct: items.filter((x) => x.direct),
    indirect: items.filter((x) => !x.direct),
    neighbours,
    note: items.length ? "" : "Документированных зависимостей не найдено; смысловой слой неполон — проверьте ссылки-соседей.",
  };
}

/** Записанный ответ контекста, ближайший к задаче по словам (стенд не вызывает воркер). */
export function contextFor(task: string): ContextReply {
  const words = (s: string) => new Set(s.toLowerCase().split(/[^a-zа-яё0-9]+/i).filter((w) => w.length > 3));
  const q = words(task);
  let best = F.context[0];
  let score = -1;
  for (const c of F.context) {
    const w = words(c.query);
    const s = [...q].filter((x) => w.has(x)).length;
    if (s > score) {
      best = c;
      score = s;
    }
  }
  return best;
}

export const evidenceOf = (id: string) => {
  const e = F.evidence[id];
  return e ? { doc: e[0], first: e[1], last: e[2], quote: e[3], method: e[4], path: F.docs[e[0]][1], docsPath: F.docs[e[0]][2], title: F.docs[e[0]][0] } : null;
};

export const AUDIT_NAME: Record<string, string> = { structural_isolate: "Изоляты", target_not_indexed: "Ссылка на неиндексированный файл", anchor_missing: "Ссылка на несуществующий раздел" };

/** Хабы: документы с наибольшим числом ссылок в обе стороны. */
export function hubs(limit = 5): { doc: number; degree: number }[] {
  const deg = new Map<number, number>();
  for (const [a, b] of F.links) {
    deg.set(a, (deg.get(a) ?? 0) + 1);
    deg.set(b, (deg.get(b) ?? 0) + 1);
  }
  return [...deg].map(([doc, degree]) => ({ doc, degree })).sort((x, y) => y.degree - x.degree).slice(0, limit);
}
