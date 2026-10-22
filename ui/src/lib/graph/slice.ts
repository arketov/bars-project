// Срез физического графа в окне: ответ `graph_physical` (physical-v1) и устойчивая модель узлов панели.
// Каждый ответ полный для своей области, пределов и раскрытых документов; индексы узлов при этом не меняются,
// чтобы раскладка, выбор и закрепления переживали раскрытие чанков и «Показать ещё».
import { type GEdge, type GNode, type LayerState, type SliceKind } from "./model";

/** Чанки раскрываются порциями по 50. */
export const PORTION = 50;

export interface ServerNode {
  id: string;
  revision: string;
  kind: "document" | "chunk";
  label: string;
  path: string;
  docs_path: string;
  module: string;
  owner: string;
  chunks: number;
  ordinal: number | null;
  first_line: number;
  last_line: number;
}

export interface ServerEdge {
  kind: SliceKind;
  /** Индексы в `nodes` того же среза; у ссылки `from → to` — направление ссылки. */
  from: number;
  to: number;
  raw: number | null;
  strength: number;
  lines: number[];
}

export interface ServerLayer {
  state: LayerState;
  note: string;
}

export interface Limits {
  max_documents: number;
  max_chunks: number;
  max_edges: number;
  max_bytes: number;
}

export interface ServerSlice {
  schema_version: number;
  method: string;
  snapshot: string;
  scope: { modules: string[]; files: string[] };
  nodes: ServerNode[];
  edges: ServerEdge[];
  layers: Record<SliceKind, ServerLayer>;
  totals: { documents: number; chunks: number; links: number; document_pairs: number };
  shown: { documents: number; chunks: number; edges: number };
  applied: Limits;
  truncated: boolean;
  limit_reasons: string[];
  warnings: string[];
  used_bytes: number;
}

export interface PhysicalRequest {
  scope: { modules: string[]; files: string[] };
  limits: Limits;
  expanded: { document: string; shown: number }[];
  snapshot: string | null;
}

export const INITIAL_LIMITS: Limits = { max_documents: 500, max_chunks: 500, max_edges: 4000, max_bytes: 1 << 20 };
export const MAX_LIMITS: Limits = { max_documents: 2000, max_chunks: 2000, max_edges: 10_000, max_bytes: 2 << 20 };

export interface Slice {
  /** Все узлы, когда-либо показанные панелью; видимость задают `visibleDocs` и `chunksOf`. */
  nodes: GNode[];
  /** Показанные документы. */
  docCount: number;
  edges: GEdge[];
  /** Загруженные чанки документа в порядке нарезки: на карте их столько, сколько раскрыто (порциями по 50). */
  chunksOf: Map<number, number[]>;
  visibleDocs: Set<number>;
  totals: ServerSlice["totals"];
  truncated: boolean;
  limitReasons: string[];
  warnings: string[];
  layers: Record<SliceKind, ServerLayer>;
  snapshot: string;
  applied: Limits;
  usedBytes: number;
}

export class SliceStore {
  nodes: GNode[] = [];
  #byId = new Map<string, number>();

  /** Индекс узла по `ObjectRef.id`. */
  indexOf(id: string): number | undefined {
    return this.#byId.get(id);
  }

  #upsert(n: ServerNode, owner: number | null): number {
    const known = this.#byId.get(n.id);
    const index = known ?? this.nodes.length;
    const node: GNode = {
      index,
      id: n.id,
      kind: n.kind,
      label: n.label,
      path: n.path,
      docsPath: n.docs_path,
      module: n.module,
      owner: owner ?? index,
      chunks: n.chunks,
      ordinal: n.ordinal,
      firstLine: n.first_line,
      lastLine: n.last_line,
    };
    this.nodes[index] = node;
    this.#byId.set(n.id, index);
    return index;
  }

  apply(server: ServerSlice): Slice {
    const local: number[] = new Array(server.nodes.length).fill(-1);
    const visibleDocs = new Set<number>();
    server.nodes.forEach((n, i) => {
      if (n.kind === "document") {
        local[i] = this.#upsert(n, null);
        visibleDocs.add(local[i]);
      }
    });
    const chunksOf = new Map<number, number[]>();
    server.nodes.forEach((n, i) => {
      if (n.kind !== "chunk") return;
      const owner = this.#byId.get(n.owner);
      if (owner === undefined || !visibleDocs.has(owner)) return;
      local[i] = this.#upsert(n, owner);
      chunksOf.set(owner, [...(chunksOf.get(owner) ?? []), local[i]]);
    });
    const edges: GEdge[] = [];
    for (const e of server.edges) {
      const a = local[e.from];
      const b = local[e.to];
      if (a === undefined || b === undefined || a < 0 || b < 0) continue;
      edges.push({ id: edges.length, key: `${e.kind}:${a}:${b}`, kind: e.kind, a, b, raw: e.raw, s: e.strength, lines: e.lines });
    }
    return {
      nodes: this.nodes,
      docCount: visibleDocs.size,
      edges,
      chunksOf,
      visibleDocs,
      totals: server.totals,
      truncated: server.truncated,
      limitReasons: server.limit_reasons,
      warnings: server.warnings,
      layers: server.layers,
      snapshot: server.snapshot,
      applied: server.applied,
      usedBytes: server.used_bytes,
    };
  }
}

/** Пустой срез для состояний «ещё не загружено» и «ошибка». */
export function emptySlice(): Slice {
  const layer: ServerLayer = { state: "not_built", note: "Граф не загружен" };
  return {
    nodes: [],
    docCount: 0,
    edges: [],
    chunksOf: new Map(),
    visibleDocs: new Set(),
    totals: { documents: 0, chunks: 0, links: 0, document_pairs: 0 },
    truncated: false,
    limitReasons: [],
    warnings: [],
    layers: { markdown_link: layer, document_similarity: layer, chunk_similarity: layer },
    snapshot: "",
    applied: INITIAL_LIMITS,
    usedBytes: 0,
  };
}
