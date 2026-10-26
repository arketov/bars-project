// Адаптер Rust QueryGraphSlice. Отбор окружения и бюджеты принадлежат воркеру.
import type { SearchHit } from "../types";
import type { Mode } from "./model";
import { SliceStore, type ServerSlice, type Slice } from "./slice";

export const FANOUT = 4;
export const QUERY_BUDGET = { nodes: 200, edges: 800 };
export const QUERY_MAX = { nodes: 500, edges: 2000 };
export const DEFAULT_DEPTH = 1;
export const MAX_DEPTH = 2;
export type ScoreKind = "rerank" | "cosine" | "none";
export type RetrievalKind = "hybrid" | "mix" | "graph";
export const SEARCH_MODE: Record<RetrievalKind, string> = { hybrid: "Обычный", mix: "Со связями", graph: "Отношения и пути" };
export function fallbackText(reason: string): string {
  return reason.replace("include_related=false", "«Добавлять связанные источники» выключено");
}
export interface QueryInput {
  id: string;
  text: string;
  hits: SearchHit[];
  scope: { modules: string[]; files: string[] };
  scoreKind: ScoreKind;
  requested: RetrievalKind;
  applied: RetrievalKind;
  fallback?: string | null;
  minScore: number;
}
export interface QueryRequest {
  id: string;
  text: string;
  scope: QueryInput["scope"];
  hits: { path: string; title: string; heading: string; first: number; last: number; score: number | null }[];
  score_kind: ScoreKind;
  depth: number;
  fanout: number;
  min_strength: number;
  max_nodes: number;
  max_edges: number;
  max_bytes: number;
  snapshot: string | null;
}
export function queryRequest(input: QueryInput, depth: number, minStrength: number, budget = QUERY_BUDGET, snapshot: string | null = null): QueryRequest {
  return { id: input.id, text: input.text, scope: input.scope,
    hits: input.hits.map(h => ({ path: h.path, title: h.title, heading: h.heading_chain, first: h.first_line, last: h.last_line, score: input.scoreKind === "none" ? null : h.score })),
    score_kind: input.scoreKind, depth, fanout: FANOUT, min_strength: minStrength,
    max_nodes: budget.nodes, max_edges: budget.edges, max_bytes: budget.nodes > 200 ? 524288 : 262144, snapshot };
}
export interface QueryResult {
  node: number | null;
  rank: number;
  path: string;
  title: string;
  score: number | null;
  kind: ScoreKind;
  fragments: { heading: string; first: number; last: number; score: number | null }[];
  status: "ready" | "no_graph_object";
  reason?: string;
}
export interface ServerQuerySlice {
  physical: ServerSlice;
  fingerprint: string;
  query_node: { id: string; label: string };
  query_edges: { to: number; raw_score: number | null; score_kind: ScoreKind; strength: number; neutral: boolean }[];
  id: string;
  text: string;
  score_kind: ScoreKind;
  results: { path: string; title: string; rank: number; node: number | null; score: number | null; fragments: QueryRequest["hits"] }[];
  views: Record<Mode, number[]>;
  depth_of: number[];
  anchor_of: number[];
  known_nodes: number;
  known_edges: number;
  used_bytes: number;
}
export interface QueryInfo {
  id: string;
  text: string;
  queryNode: number;
  requested: RetrievalKind;
  applied: RetrievalKind;
  fallback?: string | null;
  scoreKind: ScoreKind;
  results: QueryResult[];
  minScore: number;
  server: ServerQuerySlice;
}
export interface QueryBuild { slice: Slice; info: QueryInfo }

/** Только перевод wire-индексов в модель Canvas, без поиска соседей и повторного отбора. */
export function acceptQuerySlice(server: ServerQuerySlice, input: QueryInput): QueryBuild {
  if (server.id !== input.id) throw new Error("Ответ графа относится к другому поиску");
  const slice = new SliceStore().apply(server.physical);
  // Документные узлы идут в том же порядке; query-v1 не содержит раскрытых чанков.
  const queryNode = slice.nodes.length;
  slice.nodes = slice.nodes.slice();
  slice.nodes.push({ index: queryNode, id: server.query_node.id, kind: "query", label: server.query_node.label, path: "", docsPath: "", module: "запрос", owner: queryNode, chunks: 0, ordinal: null, firstLine: 0, lastLine: 0 });
  slice.visibleDocs.add(queryNode);
  const results: QueryResult[] = server.results.map(r => ({ ...r, kind: server.score_kind,
    fragments: r.fragments,
    status: r.node === null ? "no_graph_object" : "ready",
    reason: r.node === null ? "документа нет в срезе запроса: он вне снимка, области или пределов" : undefined }));
  for (const r of results) {
    if (r.node === null) continue;
    slice.nodes[r.node] = { ...slice.nodes[r.node], qrole: "result", rank: r.rank };
  }
  for (const e of server.query_edges) slice.edges.push({ id: slice.edges.length, key: `query_result:${queryNode}:${e.to}`, kind: "query_result", a: queryNode, b: e.to, raw: e.raw_score, s: e.strength, lines: [], neutral: e.neutral, scoreKind: e.score_kind });
  return { slice, info: { id: server.id, text: server.text, queryNode, requested: input.requested, applied: input.applied, fallback: input.fallback, scoreKind: server.score_kind, results, minScore: input.minScore, server } };
}
export interface Shape {
  nodes: number[];
  edgeIds: Set<number> | null;
  known: { nodes: number; edges: number };
  shown: { nodes: number; edges: number };
  truncated: { nodes: boolean; edges: boolean } | null;
}
/** Готовый список Rust для выбранного режима. Окно лишь считает видимые рёбра. */
export function shapeQuery(slice: Slice, info: QueryInfo, mode: Mode): Shape {
  const nodes = [info.queryNode, ...info.server.views[mode]];
  const shown = new Set(nodes);
  const edges = slice.edges.filter(e => shown.has(e.a) && shown.has(e.b) && (e.kind === "query_result" || mode === "all" || (mode === "links" && e.kind === "markdown_link") || (mode === "meaning" && e.kind === "document_similarity")));
  return { nodes, edgeIds: new Set(edges.map(e => e.id)), known: { nodes: info.server.known_nodes, edges: info.server.known_edges },
    shown: { nodes: nodes.length, edges: edges.length }, truncated: slice.truncated ? { nodes: slice.limitReasons.includes("nodes") || slice.limitReasons.includes("bytes"), edges: slice.limitReasons.includes("edges") || slice.limitReasons.includes("bytes") } : null };
}
