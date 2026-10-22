// Вид запроса (Т-47, пункт 2.2.3): запрос — отдельный узел, к нему результаты поиска по оценке; вокруг результатов — окружение по ссылкам и
// близости документов. Строится на клиенте поверх готового среза `graph_physical` и ответа поиска, воркер не вызывается.
// Контракт и правила: docs/features/graphrag/query-graph.md. Отклонения от контракта записаны в разделе «Что реализовано в окне».
import type { SearchHit } from "../types";
import type { GEdge, GNode, SliceKind } from "./model";
import type { Slice } from "./slice";

/** Окружение: ближайших соседей на вид связи у результата и у узла первого шага (умолчание принято автором, OQ-119). */
export const FANOUT = 4;
/** Начальные пределы вида и максимум (узлы, рёбра). */
export const QUERY_BUDGET = { nodes: 200, edges: 800 };
export const QUERY_MAX = { nodes: 500, edges: 2000 };
export const DEFAULT_DEPTH = 1;
export const MAX_DEPTH = 2;

export type ScoreKind = "rerank" | "cosine" | "none";
export type RetrievalKind = "hybrid" | "mix" | "graph";

/** Что известно о завершённом поиске: хватает для вида и не зависит от остального состояния окна. */
export interface QueryInput {
  /** Идентификатор поиска: ключ возврата в общий вид и защита от позднего ответа. */
  id: string;
  text: string;
  hits: SearchHit[];
  scoreKind: ScoreKind;
  requested: RetrievalKind;
  applied: RetrievalKind;
  fallback?: string | null;
  /** Порог поиска, показанный при пустой выдаче. */
  minScore: number;
}

export interface QueryResult {
  /** Индекс узла документа в срезе; null — документа нет в графе. */
  node: number | null;
  rank: number;
  path: string;
  title: string;
  score: number | null;
  kind: ScoreKind;
  /** Строки совпавших фрагментов. */
  fragments: { heading: string; first: number; last: number; score: number }[];
  status: "ready" | "no_graph_object";
  reason?: string;
}

export interface NeighbourRef {
  other: number;
  kind: Extract<SliceKind, "markdown_link" | "document_similarity">;
  s: number;
}

export interface QueryInfo {
  id: string;
  text: string;
  /** Индекс узла запроса в срезе. */
  queryNode: number;
  requested: RetrievalKind;
  applied: RetrievalKind;
  fallback?: string | null;
  scoreKind: ScoreKind;
  results: QueryResult[];
  /** Окружение результатов: ближайшие `FANOUT` по ссылкам и по смыслу для результатов и узлов первого шага. */
  nbrs: Map<number, NeighbourRef[]>;
  minScore: number;
}

export interface QueryBuild {
  slice: Slice;
  info: QueryInfo;
}

const LINK = "markdown_link";

/**
 * Строит срез вида запроса. Документ результата берётся по пути; у документа нескольких чанков оценка — наибольшая (одного вида).
 * Результат без узла в срезе остаётся в списке с причиной. Оценки не придумываются: `s_q = clamp(score, 0, 1)`, у `graph` оценки нет.
 */
export function buildQuerySlice(base: Slice, input: QueryInput): QueryBuild {
  const byPath = new Map<string, GNode>();
  for (const n of base.nodes) if (n.kind === "document" && base.visibleDocs.has(n.index)) byPath.set(n.path, n);

  const order: string[] = [];
  const grouped = new Map<string, SearchHit[]>();
  for (const hit of input.hits) {
    if (!grouped.has(hit.path)) order.push(hit.path);
    grouped.set(hit.path, [...(grouped.get(hit.path) ?? []), hit]);
  }
  const results: QueryResult[] = order.map((path, i) => {
    const hits = grouped.get(path)!;
    const node = byPath.get(path);
    const best = input.scoreKind === "none" ? null : Math.max(...hits.map((h) => h.score));
    return {
      node: node ? node.index : null,
      rank: i + 1,
      path,
      title: node?.label ?? hits[0].title,
      score: best,
      kind: input.scoreKind,
      fragments: hits.map((h) => ({ heading: h.heading_chain, first: h.first_line, last: h.last_line, score: h.score })),
      status: node ? "ready" : "no_graph_object",
      reason: node ? undefined : "документа нет в срезе графа: слой связей не построен для него или он вне области",
    };
  });

  // Окружение: ближайшие FANOUT по ссылкам (сначала взаимные) и по смыслу; результаты ранее прочих соседей.
  const resultNodes = new Set(results.flatMap((r) => (r.node === null ? [] : [r.node])));
  const links = new Set(base.edges.filter((e) => e.kind === LINK).map((e) => `${e.a}>${e.b}`));
  const adjacent = new Map<number, GEdge[]>();
  for (const e of base.edges) {
    if (e.kind !== LINK && e.kind !== "document_similarity") continue;
    for (const end of [e.a, e.b]) adjacent.set(end, [...(adjacent.get(end) ?? []), e]);
  }
  const near = (doc: number): NeighbourRef[] => {
    const byLink: NeighbourRef[] = [];
    const bySense: NeighbourRef[] = [];
    for (const e of adjacent.get(doc) ?? []) {
      const other = e.a === doc ? e.b : e.a;
      if (other === doc || base.nodes[other]?.kind !== "document" || !base.visibleDocs.has(other)) continue;
      if (e.kind === LINK) byLink.push({ other, kind: LINK, s: 1 });
      else bySense.push({ other, kind: "document_similarity", s: e.s });
    }
    const rank = (n: NeighbourRef) => (resultNodes.has(n.other) ? 0 : 1);
    const mutual = (n: NeighbourRef) => (links.has(`${doc}>${n.other}`) && links.has(`${n.other}>${doc}`) ? 0 : 1);
    byLink.sort((x, y) => rank(x) - rank(y) || mutual(x) - mutual(y) || x.other - y.other);
    bySense.sort((x, y) => rank(x) - rank(y) || y.s - x.s || x.other - y.other);
    const seen = new Set<number>();
    const unique = (list: NeighbourRef[]) => list.filter((n) => (seen.has(n.other) ? false : (seen.add(n.other), true))).slice(0, FANOUT);
    return [...unique(byLink), ...unique(bySense)];
  };
  const nbrs = new Map<number, NeighbourRef[]>();
  for (const r of results) {
    if (r.node === null) continue;
    const list = near(r.node);
    nbrs.set(r.node, list);
    for (const n of list) if (!nbrs.has(n.other)) nbrs.set(n.other, near(n.other));
  }

  // Узлы: копия, чтобы не трогать общий массив хранилища; запрос — последний.
  const nodes = base.nodes.slice();
  for (const r of results) {
    if (r.node === null) continue;
    nodes[r.node] = { ...nodes[r.node], qrole: "result", rank: r.rank };
  }
  const queryNode = nodes.length;
  nodes.push({ index: queryNode, id: `query:${input.id}`, kind: "query", label: input.text, path: "", docsPath: "", module: "запрос", owner: queryNode, chunks: 0, ordinal: null, firstLine: 0, lastLine: 0 });

  const edges = base.edges.slice();
  for (const r of results) {
    if (r.node === null) continue;
    edges.push({
      id: edges.length,
      key: `query_result:${queryNode}:${r.node}`,
      kind: "query_result",
      a: queryNode,
      b: r.node,
      raw: r.score,
      s: r.score === null ? 0.5 : Math.min(1, Math.max(0, r.score)),
      lines: [],
      neutral: r.score === null,
      scoreKind: r.kind,
    });
  }

  const visibleDocs = new Set(base.visibleDocs);
  visibleDocs.add(queryNode);
  const info: QueryInfo = {
    id: input.id,
    text: input.text,
    queryNode,
    requested: input.requested,
    applied: input.applied,
    fallback: input.fallback,
    scoreKind: input.scoreKind,
    results,
    nbrs,
    minScore: input.minScore,
  };
  return { slice: { ...base, nodes, edges, visibleDocs }, info };
}

export interface Shape {
  /** Показанные узлы в порядке включения: запрос, результаты по месту, окружение по кругу. */
  nodes: number[];
  /** Разрешённые рёбра между показанными узлами; null — все рёбра включённых видов. */
  edgeIds: Set<number> | null;
  known: { nodes: number; edges: number };
  shown: { nodes: number; edges: number };
  truncated: { nodes: boolean; edges: boolean } | null;
  /** Число шагов от результатов (0 — результат). */
  depthOf: Map<number, number>;
  /** Результат, к окружению которого отнесён узел. */
  anchorOf: Map<number, number>;
}

/**
 * Выбор узлов и рёбер вида запроса по режиму связей, глубине и пределам. Порядок включения: запрос; результаты по месту; соседи по кругу
 * (по одному у каждого результата, затем по второму), внутри результата ссылки, затем близость; результаты окружением не вытесняются.
 * Рёбра — только между показанными узлами; при нехватке места остаются рёбра ближних к запросу узлов, затем более сильные.
 */
export function shapeQuery(slice: Slice, info: QueryInfo, kinds: SliceKind[], minS: number, depth: number, budget = QUERY_BUDGET): Shape {
  const order: number[] = [info.queryNode];
  const set = new Set(order);
  const depthOf = new Map<number, number>();
  const anchorOf = new Map<number, number>();
  const results = info.results.filter((r) => r.node !== null);
  for (const r of results) {
    order.push(r.node!);
    set.add(r.node!);
    depthOf.set(r.node!, 0);
    anchorOf.set(r.node!, r.node!);
  }
  const allowed = (n: NeighbourRef) => kinds.includes(n.kind) && (n.kind === LINK || n.s >= minS);
  let sources = results.map((r) => r.node!);
  for (let d = 1; d <= depth; d++) {
    const lists = sources.map((s) => ({ s, items: (info.nbrs.get(s) ?? []).filter(allowed) }));
    const next: number[] = [];
    const longest = lists.reduce((m, l) => Math.max(m, l.items.length), 0);
    for (let k = 0; k < longest; k++) {
      for (const l of lists) {
        const item = l.items[k];
        if (!item || set.has(item.other)) continue;
        set.add(item.other);
        order.push(item.other);
        next.push(item.other);
        depthOf.set(item.other, d);
        anchorOf.set(item.other, anchorOf.get(l.s) ?? l.s);
      }
    }
    sources = next;
  }
  const edgesAmong = (nodes: Set<number>) =>
    slice.edges.filter((e) => e.kind !== "query_result" && kinds.includes(e.kind as SliceKind) && (e.kind === LINK || e.s >= minS) && nodes.has(e.a) && nodes.has(e.b));
  const queryEdges = slice.edges.filter((e) => e.kind === "query_result").length;
  const cap = Math.max(budget.nodes, 1 + results.length);
  const shownNodes = order.slice(0, cap);
  const shownSet = new Set(shownNodes);
  const knownEdges = edgesAmong(set).length + queryEdges;
  const candidate = edgesAmong(shownSet);
  const room = Math.max(0, budget.edges - queryEdges);
  const nodesCut = order.length > cap;
  const edgesCut = candidate.length > room;
  let edgeIds: Set<number> | null = null;
  if (edgesCut) {
    const place = new Map(order.map((n, i) => [n, i]));
    const kept = [...candidate].sort((x, y) => Math.max(place.get(x.a)!, place.get(x.b)!) - Math.max(place.get(y.a)!, place.get(y.b)!) || y.s - x.s || x.id - y.id).slice(0, room);
    edgeIds = new Set(kept.map((e) => e.id));
  }
  return {
    nodes: shownNodes,
    edgeIds,
    known: { nodes: order.length, edges: knownEdges },
    shown: { nodes: shownNodes.length, edges: (edgesCut ? room : candidate.length) + queryEdges },
    truncated: nodesCut || edgesCut ? { nodes: nodesCut, edges: edgesCut } : null,
    depthOf,
    anchorOf,
  };
}
