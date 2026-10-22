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
  /** Вид запроса: что запрос нашёл и пул окружения (стенд подбирает результаты по словам названий). */
  queryInfo?: QueryInfo;
}

/** Вид оценки поиска: rerank и cosine — разные величины (query-graph.md, «Сила связи запроса с результатом»). */
export type ScoreKind = "rerank" | "cosine" | "none";
export type ResultStatus = "ready" | "no_graph_object" | "changed_since_search" | "deleted_since_search";

export interface QueryResult {
  /** Индекс узла в срезе. */
  node: number;
  rank: number;
  channel: "primary" | "related" | "graph";
  score: number | null;
  kind: ScoreKind;
  status: ResultStatus;
  /** Причина: у related — основание включения, у результата без объекта графа — почему его нет. */
  reason?: string;
  /** У related без физического ребра узел на карту не выводится. */
  onMap: boolean;
  /** Совпавшие фрагменты документа с оценками (в стенде — названия чанков, диапазонов нет). */
  chunks: { label: string; score: number | null }[];
}

export interface NeighbourRef {
  other: number;
  kind: "markdown_link" | "document_similarity";
  s: number;
  edge: number;
}

export interface QueryInfo {
  id: number;
  text: string;
  /** Индекс узла запроса в срезе. */
  queryNode: number;
  /** Запрошенный и применённый режим поиска: при откате `mix` → `hybrid` причина видна. */
  requested: "hybrid" | "mix" | "graph";
  applied: "hybrid" | "mix" | "graph";
  fallbackNote?: string;
  scoreKind: ScoreKind;
  results: QueryResult[];
  /** Окружение результатов: ближайшие `fanout` по ссылкам и по смыслу для результатов и узлов первого шага. */
  nbrs: Map<number, NeighbourRef[]>;
  fanout: number;
  threshold: number;
  /** Вид построен на снимке до обновления индекса. */
  changedSinceSearch: boolean;
  /** Слои связей недоступны: вид состоит из запроса и результатов. */
  layersUnavailable: boolean;
  /** Общее число рёбер/узлов известно (иначе «Показано X» без «из Y»). */
  totalKnown: boolean;
  /** Начальные пределы вида и максимум (узлы, рёбра): query-graph.md, «Пределы и усечение». */
  budget: { nodes: number; edges: number; maxNodes: number; maxEdges: number };
  /** Порог поиска, показанный при пустой выдаче. */
  minScore: number;
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

/** Параметр `qs=` адреса: признаки сценария вида запроса через запятую (см. physical-graph-design.md). */
export const parseFlags = (value: string | null): Set<string> => new Set((value ?? "").split(",").map((x) => x.trim()).filter(Boolean));

export const QUERY_SCENARIOS: Record<string, string> = {
  "": "Обычный поиск, rerank, шесть результатов",
  cosine: "Без реранка: оценка — косинус, подпись вида оценки",
  mix: "«Со связями»: основные результаты и связанные источники с основанием включения",
  mixfallback: "«Со связями» выполнен как «Обычный»: причина отката видна",
  graph: "«Отношения и пути»: оценки нет, связь запроса нейтральна",
  graphnr: "Слой связей недоступен: только запрос и результаты",
  nograph: "Результаты без объекта графа: не построен, изменён, удалён",
  many: "Двадцать результатов, окружение глубины 1–2",
  limit: "Усечение по пределу: «Показано X из Y», «Показать ещё»",
  limitu: "Усечение, общее число неизвестно",
  stale: "Индекс обновился после поиска: вид помечен",
  empty: "Ничего не найдено",
};

const REASONS: Record<Exclude<ResultStatus, "ready">, string> = {
  no_graph_object: "слой связей не построен для этого документа",
  changed_since_search: "документ изменён после поиска; связи могут не соответствовать тексту",
  deleted_since_search: "документ удалён после поиска",
};

/**
 * Вид запроса Т-47 (стенд): запрос — отдельный узел, к нему результаты по оценке; пул окружения — ближайшие `fanout` результатов и узлов
 * первого шага по ссылкам и по смыслу вместе. Режим связей, глубину и пределы применяет `shapeQuery` на клиенте, без запроса к воркеру.
 * Результаты стенд подбирает по словам названий, а не поиском bars; оценки сочинены по месту в выдаче.
 */
export function queryView(base: Slice, text: string, id: number, flags: Set<string>): Slice | null {
  const label = text.trim();
  if (!label) return null;
  const area = base.nodes.filter((n) => n.kind === "document" && (!base.visibleDocs || base.visibleDocs.has(n.index)));
  const requested: QueryInfo["requested"] = flags.has("graph") ? "graph" : flags.has("mix") || flags.has("mixfallback") ? "mix" : "hybrid";
  const applied: QueryInfo["applied"] = flags.has("mixfallback") ? "hybrid" : requested;
  const scoreKind: ScoreKind = applied === "graph" ? "none" : flags.has("cosine") ? "cosine" : "rerank";
  const tokens = (label.toLowerCase().match(/[\p{L}\p{N}]{3,}/gu) ?? []).map((t) => t.slice(0, Math.max(4, t.length - 2)));
  const scored = area.map((n) => ({ n, score: tokens.reduce((sum, t) => sum + (n.label.toLowerCase().includes(t) ? 1 : 0), 0) }));
  // Поиск возвращает топ-N и при слабом совпадении слов: недостающие места занимают документы по детерминированному порядку.
  const matched = scored.filter((x) => x.score > 0).sort((a, b) => b.score - a.score || hash(a.n.index, 5) - hash(b.n.index, 5));
  const rest = scored.filter((x) => x.score === 0).sort((a, b) => hash(a.n.index, 77) - hash(b.n.index, 77));
  const ranked = [...matched, ...rest];
  const total = flags.has("empty") ? 0 : flags.has("many") ? 20 : 6;
  const picks = ranked.slice(0, total).map((x) => x.n);
  const scoreAt = (i: number): number | null => (scoreKind === "none" ? null : scoreKind === "rerank" ? Math.max(0.12, Math.round((0.97 - 0.045 * i) * 1000) / 1000) : Math.max(0.5, Math.round((0.83 - 0.03 * i) * 1000) / 1000));

  // Окружение: ближайшие fanout по ссылкам и по смыслу вместе. Узлы без объекта графа в окружение не попадают.
  const fanout = 4;
  const threshold = 0.5;
  const unavailable = flags.has("graphnr");
  const hide = new Set<number>();
  const links = new Set(base.edges.filter((e) => e.kind === "markdown_link").map((e) => `${e.a}>${e.b}`));
  const near = (doc: number): NeighbourRef[] => {
    const byLink: NeighbourRef[] = [];
    const bySense: NeighbourRef[] = [];
    for (const e of base.edges) {
      if (e.a !== doc && e.b !== doc) continue;
      const other = e.a === doc ? e.b : e.a;
      if (other === doc || base.nodes[other].kind !== "document" || hide.has(other)) continue;
      if (e.kind === "markdown_link") byLink.push({ other, kind: "markdown_link", s: 1, edge: e.id });
      else if (e.kind === "document_similarity" && e.s >= threshold) bySense.push({ other, kind: "document_similarity", s: e.s, edge: e.id });
    }
    const mutual = (n: NeighbourRef) => (links.has(`${doc}>${n.other}`) && links.has(`${n.other}>${doc}`) ? 0 : 1);
    byLink.sort((x, y) => mutual(x) - mutual(y) || x.other - y.other);
    bySense.sort((x, y) => y.s - x.s || x.other - y.other);
    const seen = new Set<number>();
    const unique = (list: NeighbourRef[]) => list.filter((n) => (seen.has(n.other) ? false : (seen.add(n.other), true))).slice(0, fanout);
    return [...unique(byLink), ...unique(bySense)];
  };

  // «Со связями»: первый связанный источник — сосед первого результата по физическому ребру, второй — без физической связи.
  const primaryCount = applied === "mix" ? Math.max(1, picks.length - 2) : picks.length;
  if (applied === "mix" && !unavailable && picks.length > primaryCount) {
    const neighbour = near(picks[0].index).find((n) => !picks.some((p) => p.index === n.other));
    if (neighbour) picks[primaryCount] = base.nodes[neighbour.other];
    if (picks.length > primaryCount + 1) {
      const close = new Set<number>(picks.map((p) => p.index));
      for (const p of picks.slice(0, primaryCount)) for (const n of near(p.index)) close.add(n.other);
      const far = area.filter((n) => !close.has(n.index) && !near(n.index).some((x) => picks.slice(0, primaryCount).some((p) => p.index === x.other))).sort((a, b) => hash(a.index, 31) - hash(b.index, 31))[0];
      if (far) picks[primaryCount + 1] = far;
    }
  }

  const nodes = base.nodes.slice();
  const chunksOf = base.chunksOf.slice();
  const results: QueryResult[] = [];
  const ghostStatus: Exclude<ResultStatus, "ready">[] = ["no_graph_object", "changed_since_search", "deleted_since_search"];
  picks.forEach((pick, i) => {
    const rank = i + 1;
    const related = applied === "mix" && i >= primaryCount;
    const ghostAt = flags.has("nograph") && i >= picks.length - 3 ? ghostStatus[i - (picks.length - 3)] : null;
    const chunkNames = base.chunksOf[pick.index].slice(0, 1 + Math.floor(hash(pick.index, 9) * 3)).map((c) => base.nodes[c].label);
    const score = related ? null : scoreAt(i);
    const chunks = related ? [] : chunkNames.map((name, j) => ({ label: name, score: score === null ? null : Math.round(Math.max(0, score - 0.02 * j) * 1000) / 1000 }));
    if (ghostAt) {
      hide.add(pick.index);
      const index = nodes.length;
      nodes.push({ index, kind: "document", label: pick.label, path: pick.path, module: pick.module, owner: index, color: pick.color, qrole: "ghost", rank, ghost: REASONS[ghostAt] });
      chunksOf[index] = [];
      results.push({ node: index, rank, channel: "primary", score, kind: scoreKind, status: ghostAt, reason: REASONS[ghostAt], onMap: true, chunks });
      return;
    }
    nodes[pick.index] = { ...pick, qrole: related ? "related" : "result", rank };
    results.push({ node: pick.index, rank, channel: related ? "related" : applied === "graph" ? "graph" : "primary", score, kind: related ? "none" : scoreKind, status: "ready", onMap: true, chunks });
  });

  const nbrs = new Map<number, NeighbourRef[]>();
  const sources = results.filter((r) => r.status === "ready" && r.channel !== "related").map((r) => r.node);
  if (!unavailable) {
    for (const r of sources) {
      const list = near(r);
      nbrs.set(r, list);
      for (const n of list) if (!nbrs.has(n.other)) nbrs.set(n.other, near(n.other));
    }
    // Связанный источник виден только при физическом ребре с результатом; иначе остаётся в списке (query-graph.md, «Режимы поиска»).
    for (const r of results) {
      if (r.channel !== "related") continue;
      const anchor = sources.find((d) => (nbrs.get(d) ?? []).some((n) => n.other === r.node));
      r.onMap = !!anchor;
      const via = anchor ? nbrs.get(anchor)!.find((n) => n.other === r.node)! : null;
      r.reason = via
        ? `связан с результатом «${base.nodes[anchor!].label}» ${via.kind === "markdown_link" ? "ссылкой" : "близостью по смыслу"}`
        : "основание включено поиском, физической связи с результатами нет: ребро не выдумывается";
    }
  } else {
    for (const r of results) {
      if (r.channel !== "related") continue;
      r.onMap = false;
      r.reason = "слои связей недоступны: источник остаётся в списке";
    }
  }

  const queryNode = nodes.length;
  nodes.push({ index: queryNode, kind: "query", label, path: null, module: "запрос", owner: queryNode });
  chunksOf[queryNode] = [];
  const edges = [...base.edges];
  for (const r of results) {
    if (!r.onMap || r.channel === "related") continue;
    edges.push({
      id: edges.length,
      kind: "query_result",
      a: queryNode,
      b: r.node,
      raw: r.score,
      s: r.score === null ? 0.5 : Math.min(1, Math.max(0, r.score)),
      support: 0,
      lines: [],
      neutral: r.score === null,
      scoreKind: r.kind,
    });
  }
  const limited = flags.has("limit") || flags.has("limitu");
  const down = (note: string) => ({ state: "failed" as LayerState, note });
  const layers = unavailable ? { ...base.layers, markdown_link: down("Слой ссылок недоступен (graph_not_ready): вид состоит из запроса и результатов"), document_similarity: down("Слой близости недоступен (graph_not_ready)") } : base.layers;
  const info: QueryInfo = {
    id,
    text: label,
    queryNode,
    requested,
    applied,
    fallbackNote: flags.has("mixfallback") ? "Режим «Со связями» выполнен как «Обычный»: слой связей поиска не готов. Вид построен по применённому режиму." : undefined,
    scoreKind,
    results,
    nbrs,
    fanout,
    threshold,
    changedSinceSearch: flags.has("stale"),
    layersUnavailable: unavailable,
    totalKnown: !flags.has("limitu"),
    budget: limited ? { nodes: 36, edges: 110, maxNodes: 120, maxEdges: 400 } : { nodes: 200, edges: 800, maxNodes: 500, maxEdges: 2000 },
    minScore: 0.18,
  };
  return { ...base, nodes, chunksOf, edges, visibleDocs: undefined, layers, queryInfo: info };
}

export interface Shape {
  /** Показанные узлы в порядке включения: запрос, результаты по месту, окружение по кругу. */
  nodes: number[];
  /** Разрешённые рёбра между показанными узлами; null — все рёбра включённых видов. */
  edgeIds: Set<number> | null;
  known: { nodes: number; edges: number };
  shown: { nodes: number; edges: number };
  truncated: { nodes: boolean; edges: boolean } | null;
  /** Число шагов от результатов (0 — результат) и результат, к окружению которого узел отнесён. */
  depthOf: Map<number, number>;
  anchorOf: Map<number, number>;
}

/**
 * Выбор узлов и рёбер вида запроса по режиму связей, глубине и пределам (query-graph.md, «Построение» и «Пределы и усечение»).
 * Порядок включения: запрос; результаты по месту; соседи по кругу (по одному у каждого результата, затем по второму), внутри результата
 * ссылки, затем близость; результаты окружением не вытесняются. Рёбра — только между показанными узлами.
 */
export function shapeQuery(slice: Slice, kinds: EdgeKind[], minS: number, depth: number, budget: { nodes: number; edges: number }): Shape {
  const info = slice.queryInfo!;
  const order: number[] = [info.queryNode];
  const set = new Set(order);
  const depthOf = new Map<number, number>();
  const anchorOf = new Map<number, number>();
  const results = info.results.filter((r) => r.onMap && r.channel !== "related");
  for (const r of results) {
    order.push(r.node);
    set.add(r.node);
    depthOf.set(r.node, 0);
    anchorOf.set(r.node, r.node);
  }
  const allowed = (n: NeighbourRef) => kinds.includes(n.kind) && (n.kind === "markdown_link" || n.s >= minS);
  let sources = results.filter((r) => r.status === "ready").map((r) => r.node);
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
    slice.edges.filter((e) => e.kind !== "query_result" && kinds.includes(e.kind) && (e.kind === "markdown_link" || e.s >= minS) && nodes.has(e.a) && nodes.has(e.b));
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
