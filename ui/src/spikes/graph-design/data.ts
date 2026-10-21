// Данные стенда дизайна физического графа 2.1.4: срез из сохранённого снимка bars-project и синтетические сценарии.
// Только стенд: настоящий срез `PhysicalGraphSlice` строит Rust (physical-graph.md, «Срез, бюджеты и договор данных»).
import fixture from "./fixture.json";
import { EDGE_KINDS, hash, strengthOf, type EdgeKind, type GEdge, type GNode, type LayerState } from "./model";

export interface Layer {
  state: LayerState;
  note: string;
}

export interface Slice {
  nodes: GNode[];
  docCount: number;
  edges: GEdge[];
  /** Чанки документа в порядке нарезки; в карте они появляются порциями по 50. */
  chunksOf: number[][];
  totals: { documents: number; chunks: number };
  truncated: boolean;
  /** Документы, вошедшие в бюджет среза; у полного среза не задано. Индексы узлов при продолжении не меняются. */
  visibleDocs?: Set<number>;
  layers: Record<EdgeKind, Layer>;
  scope: string;
  demo: string;
  /** Вид запроса: что запрос нашёл и что показано (стенд подбирает результаты по словам названий). */
  queryInfo?: { text: string; results: number[]; scores: number[]; fallback: boolean };
}

interface Fixture {
  docs: [string, string | null, string][];
  chunks: [string, number][];
  links: [number, number, number[]][];
  dsim: [number, number, number][];
  csim: [number, number, number][];
  captured: string;
}

const data = fixture as unknown as Fixture;
export const PORTION = 50;

const MODULE_COLORS: Record<string, string> = { features: "#2f9e8f", adr: "#d99a2b" };

const readyLayers = (): Record<EdgeKind, Layer> => ({
  markdown_link: { state: "ready", note: "Ссылки разрешены Markdown-парсером" },
  document_similarity: { state: "ready", note: "Документные представления посчитаны по готовым векторам" },
  chunk_similarity: { state: "ready", note: "kNN чанков готов" },
  query_result: { state: "ready", note: "Результаты запроса" },
});

function base(): Slice {
  const nodes: GNode[] = data.docs.map(([label, path, module], index) => ({
    index,
    kind: "document",
    label,
    path,
    module,
    owner: index,
    color: MODULE_COLORS[module],
  }));
  const chunksOf: number[][] = data.docs.map(() => []);
  for (const [label, owner] of data.chunks) {
    const index = nodes.length;
    nodes.push({ index, kind: "chunk", label, path: nodes[owner].path, module: nodes[owner].module, owner });
    chunksOf[owner].push(index);
  }
  const edges: GEdge[] = [];
  const add = (kind: EdgeKind, a: number, b: number, raw: number | null, lines: number[] = []) =>
    edges.push({ id: edges.length, kind, a, b, raw, s: strengthOf(kind, raw), support: lines.length, lines });
  // Ссылки настоящие: фикстура разобрала Markdown-ссылки текущих файлов (направление и строки сохранены).
  for (const [a, b, lines] of data.links) add("markdown_link", a, b, null, lines);
  for (const [a, b, w] of data.dsim) add("document_similarity", a, b, w);
  const chunkStart = data.docs.length;
  for (const [a, b, w] of data.csim) add("chunk_similarity", chunkStart + a, chunkStart + b, w);
  return {
    nodes,
    docCount: data.docs.length,
    edges,
    chunksOf,
    totals: { documents: data.docs.length, chunks: data.chunks.length },
    truncated: false,
    layers: readyLayers(),
    scope: "Весь проект",
    demo: `Демо-данные: снимок bars-project от ${data.captured}; ссылки — настоящие Markdown-ссылки текущих файлов; близость документов — из сохранённого kNN, не centroid`,
  };
}

/** Оставляет только перечисленные документы: чанки и рёбра, у которых конец вне набора, отбрасываются. */
function restrict(slice: Slice, keep: number[]): Slice {
  const remap = new Map<number, number>();
  const nodes: GNode[] = [];
  const chunksOf: number[][] = [];
  for (const old of keep) {
    const index = nodes.length;
    remap.set(old, index);
    nodes.push({ ...slice.nodes[old], index, owner: index });
    chunksOf.push([]);
  }
  for (const old of keep) {
    for (const chunk of slice.chunksOf[old]) {
      const index = nodes.length;
      remap.set(chunk, index);
      nodes.push({ ...slice.nodes[chunk], index, owner: remap.get(old)! });
      chunksOf[remap.get(old)!].push(index);
    }
  }
  const edges: GEdge[] = [];
  for (const edge of slice.edges) {
    const a = remap.get(edge.a);
    const b = remap.get(edge.b);
    if (a !== undefined && b !== undefined) edges.push({ ...edge, id: edges.length, a, b });
  }
  return { ...slice, nodes, docCount: keep.length, edges, chunksOf };
}

/** Детерминированный обход модулей по кругу, внутри — по пути (правило «Срез, бюджеты» проекта). */
function roundRobin(slice: Slice, limit: number): number[] {
  const groups = new Map<string, number[]>();
  for (let i = 0; i < slice.docCount; i++) {
    const node = slice.nodes[i];
    groups.set(node.module, [...(groups.get(node.module) ?? []), i]);
  }
  const lists = [...groups.entries()].sort(([a], [b]) => a.localeCompare(b)).map(([, list]) => list.sort((x, y) => (slice.nodes[x].path ?? "").localeCompare(slice.nodes[y].path ?? "")));
  const picked: number[] = [];
  for (let round = 0; picked.length < limit; round++) {
    let any = false;
    for (const list of lists) {
      if (round < list.length && picked.length < limit) {
        picked.push(list[round]);
        any = true;
      }
    }
    if (!any) break;
  }
  return picked;
}

function longDocument(slice: Slice): Slice {
  const owner = slice.nodes.findIndex((n) => n.kind === "document" && n.label.startsWith("MAIN_IDEA"));
  const target = slice.nodes[owner];
  const originalChunks = slice.chunksOf[owner].length;
  for (let i = 0; i < 120 - originalChunks; i++) {
    const index = slice.nodes.length;
    slice.nodes.push({ index, kind: "chunk", label: `${target.label}/Т-${100 + i} Синтетический раздел для проверки порций`, path: target.path, module: target.module, owner });
    slice.chunksOf[owner].push(index);
    for (let j = 0; j < 2; j++) {
      const other = slice.nodes.length > 0 ? slice.docCount + Math.floor(hash(i * 7 + j, 3) * (data.chunks.length - 1)) : 0;
      if (slice.nodes[other]?.owner === owner) continue;
      const raw = Math.round((0.62 + hash(i * 3 + j, 5) * 0.3) * 1000) / 1000;
      slice.edges.push({ id: slice.edges.length, kind: "chunk_similarity", a: index, b: other, raw, s: strengthOf("chunk_similarity", raw), support: 0, lines: [] });
    }
  }
  slice.totals.chunks += 120 - originalChunks;
  slice.demo += "; у «MAIN_IDEA» добавлено до 120 синтетических чанков";
  return slice;
}

function sparse(): Slice {
  const names: [string, string][] = [
    ["Оглавление", "docs/Index.md"],
    ["Требования окна", "docs/features/window/requirements.md"],
    ["Решение по хранению данных проекта и расположению индекса на диске пользователя", "docs/adr/0001-storage.md"],
    ["Сверхдлинноеназваниебезпробеловдляпроверкипереносаиобрезкиподписиграфа", "docs/knowledge/long.md"],
    ["Руководство: поиск", "docs/manual/search.md"],
    ["Руководство: настройки", "docs/manual/settings.md"],
    ["Журнал сессии", "docs/chats/2026-10-21-demo/log.md"],
    ["Итог сессии", "docs/chats/2026-10-21-demo/handoff.md"],
    ["Без связей 1", "docs/knowledge/alone-1.md"],
    ["Без связей 2", "docs/knowledge/alone-2.md"],
    ["Без связей 3", "docs/knowledge/alone-3.md"],
  ];
  const nodes: GNode[] = names.map(([label, path], index) => ({ index, kind: "document", label, path, module: path.split("/")[1] ?? ".", owner: index, color: MODULE_COLORS[path.split("/")[1]] }));
  const edgesDef: [EdgeKind, number, number, number | null][] = [
    ["markdown_link", 0, 1, null],
    ["markdown_link", 0, 4, null],
    ["markdown_link", 4, 5, null],
    ["markdown_link", 5, 4, null],
    ["markdown_link", 1, 2, null],
    ["document_similarity", 1, 2, 0.91],
    ["document_similarity", 1, 3, 0.63],
    ["document_similarity", 4, 5, 0.82],
    ["document_similarity", 6, 7, 0.74],
    ["document_similarity", 0, 7, 0.61],
  ];
  const edges = edgesDef.map(([kind, a, b, raw], id): GEdge => ({ id, kind, a, b, raw, s: strengthOf(kind, raw), support: kind === "markdown_link" ? 1 : 0, lines: kind === "markdown_link" ? [3 + id] : [] }));
  return {
    nodes,
    docCount: nodes.length,
    edges,
    chunksOf: nodes.map(() => []),
    totals: { documents: nodes.length, chunks: 0 },
    truncated: false,
    layers: readyLayers(),
    scope: "Выбранные папки",
    demo: "Синтетический набор: изоляты, длинные названия, взаимные ссылки",
  };
}

export function loadSlice(scenario: string): Slice {
  const full = base();
  switch (scenario) {
    case "long":
      return longDocument(full);
    case "sparse":
      return sparse();
    case "empty": {
      return { ...full, nodes: [], docCount: 0, edges: [], chunksOf: [], totals: { documents: 0, chunks: 0 }, scope: "Выбранные папки", demo: "Пустая область: ни одного документа" };
    }
    case "nolinks": {
      const slice = restrict(full, Array.from({ length: 12 }, (_, i) => i * 20));
      return { ...slice, edges: [], demo: "Документы без связей: у всех слоёв нет допущенных рёбер" };
    }
    case "truncated": {
      const kept = roundRobin(full, 150);
      return {
        ...full,
        docCount: kept.length,
        visibleDocs: new Set(kept),
        truncated: true,
        demo: `${full.demo}; бюджет 150 узлов: показан обход модулей по кругу`,
      };
    }
    case "building":
    case "partial":
    case "stale":
    case "failed": {
      const state = scenario as LayerState;
      const notes: Record<string, string> = {
        building: "Слой близости документов строится: показаны ссылки и уже готовая часть",
        partial: "Слой близости документов готов не для всех документов: у части векторов нет",
        stale: "Слой близости построен на устаревшем снимке: обновите граф",
        failed: "Слой близости документов не построен: ошибка вычисления; ссылки доступны",
      };
      const edges = scenario === "failed" ? full.edges.filter((e) => e.kind !== "document_similarity") : scenario === "partial" ? full.edges.filter((e) => e.kind !== "document_similarity" || (e.a + e.b) % 3 !== 0) : full.edges;
      return { ...full, edges, layers: { ...readyLayers(), document_similarity: { state, note: notes[scenario] } } };
    }
    default:
      return full;
  }
}

export const SCENARIOS: Record<string, string> = {
  project: "Проект bars-project: 301 документ, ссылки и близость, чанки раскрываются",
  long: "Документ на 120 чанков: раскрытие порциями по 50",
  truncated: "Усечение бюджета: 150 из 301 документа",
  sparse: "Изоляты, длинные названия, взаимные ссылки",
  empty: "Пустая область",
  nolinks: "Документы без связей",
  building: "Слой близости строится",
  partial: "Слой близости неполный",
  stale: "Слой близости устарел",
  failed: "Слой близости не построен",
  norender: "Отказ renderer: работает список",
};

export const KIND_ORDER = EDGE_KINDS;

/**
 * Вид запроса Т-47 (стенд): запрос — отдельный узел, от него связи с найденными документами по оценке; от каждого результата —
 * ближайшие документы по тем видам связей, которые включены в режиме. Результаты стенд подбирает по словам названий, а не поиском bars.
 */
export function queryView(base: Slice, text: string, kinds: EdgeKind[], minS: number): Slice | null {
  const docs = base.nodes.filter((n) => n.kind === "document" && (!base.visibleDocs || base.visibleDocs.has(n.index)));
  if (!docs.length || !text.trim()) return null;
  const tokens = (text.toLowerCase().match(/[\p{L}\p{N}]{3,}/gu) ?? []).map((t) => t.slice(0, Math.max(4, t.length - 2)));
  const scored = docs.map((n) => ({ n, score: tokens.reduce((sum, t) => sum + (n.label.toLowerCase().includes(t) ? 1 : 0), 0) }));
  let ranked = scored.filter((x) => x.score > 0).sort((a, b) => b.score - a.score || hash(a.n.index, 5) - hash(b.n.index, 5));
  const fallback = ranked.length === 0;
  if (fallback) ranked = [...scored].sort((a, b) => hash(a.n.index, 77) - hash(b.n.index, 77));
  const results = ranked.slice(0, 6).map((x) => x.n.index);
  const scores = results.map((_, i) => Math.round((0.95 - 0.05 * i) * 1000) / 1000);
  const shown = new Set(results);
  for (const r of results) {
    const near = base.edges
      .filter((e) => kinds.includes(e.kind) && e.kind !== "chunk_similarity" && (e.kind === "markdown_link" || e.s >= minS) && (e.a === r || e.b === r))
      .map((e) => ({ other: e.a === r ? e.b : e.a, s: e.s }))
      .filter((c) => base.nodes[c.other].kind === "document")
      .sort((x, y) => y.s - x.s);
    for (const c of near.slice(0, 4)) shown.add(c.other);
  }
  const index = base.nodes.length;
  const node: GNode = { index, kind: "query", label: text.trim(), path: null, module: "запрос", owner: index };
  const edges = [...base.edges];
  results.forEach((r, i) => edges.push({ id: edges.length, kind: "query_result", a: index, b: r, raw: scores[i], s: scores[i], support: 0, lines: [] }));
  return { ...base, nodes: [...base.nodes, node], edges, visibleDocs: shown, docCount: shown.size, queryInfo: { text: text.trim(), results, scores, fallback } };
}
