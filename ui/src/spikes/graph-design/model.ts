// Чистая модель стенда дизайна физического графа 2.1.4: сила связи, стиль и коэффициенты пружин.
// Формулы — из docs/features/graphrag/physical-graph.md («Сила, видимость и физика»); численные параметры инженерные.

export type EdgeKind = "markdown_link" | "document_similarity" | "chunk_similarity" | "query_result";
/** Режим карты: какие виды связей видны одновременно. */
export type Mode = "links" | "meaning" | "chunks" | "all";
export type LayerState = "ready" | "building" | "partial" | "stale" | "failed" | "cancelled";
/** Шкала рисунка: `project` — абсолютная по проекту 2.1.3; `window` — абсолютное окно cosine модели (сравнение). */
export type ScaleName = "project" | "window";

export interface GNode {
  index: number;
  kind: "document" | "chunk" | "query";
  label: string;
  path: string | null;
  module: string;
  /** Индекс документа-владельца: у документа — он сам. */
  owner: number;
  /** Цвет документа по Т-38, если назначен. */
  color?: string;
  /** Роль в виде запроса: результат поиска, связанный источник режима «Со связями» или результат без объекта графа. */
  qrole?: "result" | "related" | "ghost";
  /** Место результата в выдаче (с 1). */
  rank?: number;
  /** Причина, по которой результат без объекта графа: слой не построен, документ изменён или удалён после поиска. */
  ghost?: string;
}

export interface GEdge {
  id: number;
  kind: EdgeKind;
  a: number;
  b: number;
  /** Исходный cosine; у ссылки отсутствует. */
  raw: number | null;
  /** Сила `s`: clamp(cosine,0,1) для близости, 1 для ссылки как факта. */
  s: number;
  /** Число независимых диапазонов ссылки; только сведения, не множитель силы. */
  support: number;
  /** Строки текста, где стоит ссылка (у настоящих ссылок). */
  lines: number[];
  /** Связь запроса без оценки (режим «Отношения и пути»): рисуется нейтрально, толщина не зависит от оценки. */
  neutral?: boolean;
  /** Вид оценки связи запроса: rerank рисуется по абсолютной оценке, cosine — по окну модели (query-graph.md). */
  scoreKind?: "rerank" | "cosine" | "none";
}

export const EDGE_KINDS: EdgeKind[] = ["markdown_link", "document_similarity", "chunk_similarity"];
export const ALL_KINDS: EdgeKind[] = [...EDGE_KINDS, "query_result"];

export const KIND_TITLE: Record<EdgeKind, string> = {
  markdown_link: "Ссылка",
  document_similarity: "Близость документов",
  chunk_similarity: "Близость чанков",
  query_result: "Связь с запросом",
};

export function strengthOf(kind: EdgeKind, cosine: number | null): number {
  if (kind === "markdown_link") return 1;
  return Math.min(1, Math.max(0, cosine ?? 0));
}

/** Окно cosine, в котором модель bge-m3 даёт значимые пары; по снимкам 2.1.1 допущенные пары лежат в 0,61–0,92. */
export const WINDOW = { lo: 0.6, hi: 0.95 };

/** Сила для рисунка. Шкала `project` повторяет `s`; `window` растягивает фиксированное окно модели, а не диапазон экрана. */
export function drawStrength(edge: Pick<GEdge, "kind" | "s" | "raw" | "scoreKind">, scale: ScaleName): number {
  if (scale === "project" || edge.kind === "markdown_link" || edge.raw === null) return edge.s;
  // Оценка rerank — не cosine модели: окно 0,60–0,95 к ней не применяется.
  if (edge.kind === "query_result" && edge.scoreKind === "rerank") return edge.s;
  return Math.min(1, Math.max(0, (edge.raw - WINDOW.lo) / (WINDOW.hi - WINDOW.lo)));
}

/** Толщина в CSS px: w(s)=0.6+1.8s. */
export const widthOf = (s: number): number => 0.6 + 1.8 * s;
/** Коэффициент смешения цвета вида с фоном: b(s)=0.25+0.65s. */
export const brightnessOf = (s: number): number => 0.25 + 0.65 * s;
/** Притяжение логического ребра: q(s)=0.05+0.45s. */
export const springOf = (s: number): number => 0.05 + 0.45 * s;

/**
 * Коэффициенты пружин: для каждой пары разных владельцев `Q=max(q)`, `k_e=Q*q_e/sum(q)`;
 * внутридокументные рёбра нормализуются отдельно в корзине владельца. Суммарное притяжение пары равно Q.
 */
export function springCoefficients(edges: GEdge[], ownerOf: (node: number) => number): Map<number, number> {
  const buckets = new Map<string, GEdge[]>();
  for (const edge of edges) {
    const a = ownerOf(edge.a);
    const b = ownerOf(edge.b);
    const key = a === b ? `in:${a}` : a < b ? `${a}:${b}` : `${b}:${a}`;
    const bucket = buckets.get(key);
    if (bucket) bucket.push(edge);
    else buckets.set(key, [edge]);
  }
  const result = new Map<number, number>();
  for (const bucket of buckets.values()) {
    const q = bucket.map((edge) => springOf(edge.s));
    const total = q.reduce((sum, value) => sum + value, 0);
    const max = Math.max(...q);
    bucket.forEach((edge, i) => result.set(edge.id, (max * q[i]) / total));
  }
  return result;
}

/** Смешение цвета вида с фоном: коэффициент 1 — чистый цвет, 0 — фон. */
export function mixColor(kind: [number, number, number], bg: [number, number, number], b: number): string {
  const channel = (i: number) => Math.round(bg[i] + (kind[i] - bg[i]) * b);
  return `rgb(${channel(0)},${channel(1)},${channel(2)})`;
}

export function parseColor(value: string): [number, number, number] {
  const hex = value.trim();
  if (/^#[0-9a-f]{6}$/i.test(hex)) return [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)) as [number, number, number];
  if (/^#[0-9a-f]{3}$/i.test(hex)) return [1, 2, 3].map((i) => parseInt(hex[i] + hex[i], 16)) as [number, number, number];
  const m = /rgba?\(([^)]+)\)/.exec(hex);
  if (m) return m[1].split(",").slice(0, 3).map((v) => parseFloat(v)) as [number, number, number];
  return [128, 128, 128];
}

export function hash(index: number, salt = 0): number {
  let h = (index + 1) * 2654435761 + salt * 40503;
  h = Math.imul(h ^ (h >>> 15), 2246822507);
  h = Math.imul(h ^ (h >>> 13), 3266489909);
  return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
}
