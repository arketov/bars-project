// Действия навигации по карте (2.3.6): локальный граф и путь по карте считаются на клиенте над уже загруженным срезом.
// Чистая логика без DOM; правила — docs/features/graphrag/graph-actions-design.md, разделы «Локальный граф» и «Путь между двумя вершинами».
import type { GEdge } from "./model";

export interface Hop {
  edge: GEdge;
  other: number;
  /** Ход по направлению ребра (для ссылок — от источника к цели). */
  forward: boolean;
}

export interface Adj {
  of(n: number): Hop[];
}

/** Смежность по рёбрам: ссылки направленные, остальные виды связей — нет. `directed` оставляет ссылки только с прямым ходом. */
export function adjacency(edges: GEdge[], directed = false): Adj {
  const map = new Map<number, Hop[]>();
  const push = (n: number, hop: Hop) => {
    const list = map.get(n);
    if (list) list.push(hop);
    else map.set(n, [hop]);
  };
  for (const e of edges) {
    push(e.a, { edge: e, other: e.b, forward: true });
    if (!(directed && e.kind === "markdown_link")) push(e.b, { edge: e, other: e.a, forward: false });
  }
  return { of: (n) => map.get(n) ?? [] };
}

/** Пределы локального графа: сначала 200/800, затем «Показать ещё» двумя ступенями до 500/2000. */
export const LOCAL_LIMITS = [
  { nodes: 200, edges: 800 },
  { nodes: 350, edges: 1400 },
  { nodes: 500, edges: 2000 },
] as const;
export const LOCAL_MAX_DEPTH = 3;

export interface LocalGraph {
  /** Узел → шаг от центра (0 — центр). */
  depthOf: Map<number, number>;
  /** Идентификаторы рёбер между показанными узлами. */
  edgeIds: Set<number>;
  /** Сколько узлов и связей нашёл бы обход без пределов. */
  known: { nodes: number; edges: number };
  /** Обход упёрся в предел узлов или связей: следующий шаг глубины недоступен, пока не выбрано «Показать ещё». */
  truncated: boolean;
}

/** Обход в ширину от центра на глубину `depth`; ссылки идут в обе стороны. Узлы сверх предела отбрасываются, сильные связи берутся первыми. */
export function localGraph(adj: Adj, center: number, depth: number, limits: { nodes: number; edges: number } = { nodes: Infinity, edges: Infinity }): LocalGraph {
  const depthOf = new Map<number, number>([[center, 0]]);
  let frontier = [center];
  let truncated = false;
  for (let step = 1; step <= depth && frontier.length; step++) {
    const found = new Map<number, number>();
    for (const n of frontier) for (const { other, edge } of adj.of(n)) if (!depthOf.has(other)) found.set(other, Math.max(found.get(other) ?? 0, edge.s));
    const ordered = [...found.entries()].sort((x, y) => y[1] - x[1] || x[0] - y[0]);
    const room = Math.max(0, limits.nodes - depthOf.size);
    if (ordered.length > room) truncated = true;
    const next = ordered.slice(0, room).map(([n]) => n);
    for (const n of next) depthOf.set(n, step);
    frontier = next;
  }
  const all = new Map<number, GEdge>();
  for (const n of depthOf.keys()) for (const { edge } of adj.of(n)) if (depthOf.has(edge.a) && depthOf.has(edge.b)) all.set(edge.id, edge);
  let edges = [...all.values()];
  if (edges.length > limits.edges) {
    truncated = true;
    edges = edges.sort((x, y) => y.s - x.s || x.id - y.id).slice(0, limits.edges);
  }
  // Сколько нашёл бы обход без пределов: нужно подписи «показано X из Y».
  const total = truncated ? localGraph(adj, center, depth).known : { nodes: depthOf.size, edges: edges.length };
  return { depthOf, edgeIds: new Set(edges.map((e) => e.id)), known: total, truncated };
}

export interface MapPath {
  nodes: number[];
  steps: { edge: GEdge; forward: boolean }[];
  /** Сумма (1 − s) по шагам: при равной длине выше путь с более сильными связями. */
  cost: number;
}

export const PATH_VARIANTS = 3;
export const PATH_MAX_HOPS = 6;

/** Кратчайший по числу шагов путь с разрывом ничьих по силе, без запрещённых узлов и рёбер (нужно алгоритму Йена). */
function shortest(adj: Adj, from: number, to: number, maxHops: number, bannedNodes: Set<number>, bannedEdges: Set<number>): MapPath | null {
  type St = { node: number; hops: number; cost: number; prev: St | null; edge: GEdge | null; forward: boolean };
  if (from === to) return null;
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
        const state: St = { node: other, hops: hop + 1, cost, prev: st, edge, forward };
        best.set(other, state);
        next.push(state);
      }
    }
    if (best.has(to)) break;
    frontier = next.filter((s) => best.get(s.node) === s);
  }
  const end = best.get(to);
  if (!end) return null;
  const nodes: number[] = [];
  const steps: MapPath["steps"] = [];
  for (let s: St | null = end; s; s = s.prev) {
    nodes.unshift(s.node);
    if (s.edge) steps.unshift({ edge: s.edge, forward: s.forward });
  }
  return { nodes, steps, cost: end.cost };
}

/** До `k` кратчайших простых путей (алгоритм Йена), не длиннее `maxHops` шагов. */
export function mapPaths(adj: Adj, from: number, to: number, k = PATH_VARIANTS, maxHops = PATH_MAX_HOPS): MapPath[] {
  const first = shortest(adj, from, to, maxHops, new Set(), new Set());
  if (!first) return [];
  const found: MapPath[] = [first];
  const candidates: MapPath[] = [];
  // Взаимные ссылки — одна линия на карте: варианты различаются по узлам, а не по параллельным рёбрам.
  const keyOf = (p: MapPath) => p.nodes.join(",");
  for (let i = 1; i < k; i++) {
    const last = found[i - 1];
    for (let j = 0; j < last.nodes.length - 1; j++) {
      const spur = last.nodes[j];
      const root = last.steps.slice(0, j);
      const rootKey = last.nodes.slice(0, j + 1).join(",");
      const bannedEdges = new Set<number>();
      for (const p of found) {
        if (p.nodes.length <= j + 1 || p.nodes.slice(0, j + 1).join(",") !== rootKey) continue;
        for (const hop of adj.of(spur)) if (hop.other === p.nodes[j + 1]) bannedEdges.add(hop.edge.id);
      }
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
