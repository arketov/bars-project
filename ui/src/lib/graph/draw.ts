// Отрисовка Canvas2D по образцу графа Obsidian (раунд 27): узлы главнее линий — линии тонкие и серые, размер узла
// растёт с числом связей, цвет — цвет модуля, подписи проявляются с приближением. Дизайн: physical-graph-design.md, «Рисунок как в Obsidian».
import type { PLink, PNode } from "./layout";
import { drawStrength, mixColor, parseColor, widthOf, type EdgeKind, type GNode } from "./model";

export interface Palette {
  bg: [number, number, number];
  bgCss: string;
  text: string;
  muted: string;
  faint: string;
  accent: string;
  accentSoft: string;
  line: string;
  /** Линия ребра в покое: нейтральный серый. */
  edge: [number, number, number];
  kind: Record<EdgeKind, [number, number, number]>;
  kindCss: Record<EdgeKind, string>;
  /** Цвета палитры Т-38 по идентификатору. */
  pal: Record<string, string>;
}

const KIND_COLORS = {
  light: { markdown_link: "#d6336c", document_similarity: "#2b7bb9", chunk_similarity: "#7b4fc2", query_result: "#d9730d" },
  dark: { markdown_link: "#f0568c", document_similarity: "#5eb0ef", chunk_similarity: "#b18cf0", query_result: "#ff9a4d" },
};

const EDGE_GRAY = { light: "#cfcfcf", dark: "#4b4b4b" };

/** Палитра Т-38 (colors-icons.md): те же имена, что в theme.css; токены `--pal-<id>` темы важнее, если заданы. */
const PALETTE = {
  light: { red: "#d04545", orange: "#d9782a", amber: "#b58a14", green: "#2f9a5c", teal: "#1f9a95", blue: "#3f84cf", violet: "#8059c9", pink: "#d14a80", gray: "#85808a" },
  dark: { red: "#ec7070", orange: "#f0a35c", amber: "#e0bb45", green: "#5cc287", teal: "#46c1bb", blue: "#6aa8ec", violet: "#a98cea", pink: "#ec79a6", gray: "#8a8590" },
};

/** Порядок цветов модулей: модули по алфавиту получают их по кругу; серый — у корневых документов. */
export const MODULE_ORDER = ["blue", "green", "orange", "violet", "teal", "pink", "amber", "red"];

/** Цвет (идентификатор палитры) каждого модуля: по алфавиту, по кругу; корень «.» и пустой модуль — серый. */
export function moduleColors(modules: Iterable<string>): Map<string, string> {
  const named = [...new Set(modules)].filter((m) => m && m !== ".").sort((a, b) => a.localeCompare(b));
  const map = new Map<string, string>(named.map((m, i) => [m, MODULE_ORDER[i % MODULE_ORDER.length]]));
  map.set(".", "gray");
  map.set("", "gray");
  return map;
}

export function readPalette(el: HTMLElement): Palette {
  const css = getComputedStyle(el);
  const get = (name: string) => css.getPropertyValue(name).trim();
  const dark = document.documentElement.dataset.mode === "dark";
  const set = KIND_COLORS[dark ? "dark" : "light"];
  const base = PALETTE[dark ? "dark" : "light"];
  const pal: Record<string, string> = {};
  for (const [id, value] of Object.entries(base)) pal[id] = get(`--pal-${id}`) || value;
  return {
    bg: parseColor(get("--panel")),
    bgCss: get("--panel"),
    text: get("--text"),
    muted: get("--muted"),
    faint: get("--faint"),
    accent: get("--accent"),
    accentSoft: get("--accent-soft"),
    line: get("--line-2"),
    edge: parseColor(get("--graph-edge") || EDGE_GRAY[dark ? "dark" : "light"]),
    kind: {
      markdown_link: parseColor(set.markdown_link),
      document_similarity: parseColor(set.document_similarity),
      chunk_similarity: parseColor(set.chunk_similarity),
      query_result: parseColor(set.query_result),
    },
    kindCss: set,
    pal,
  };
}

export interface Camera {
  x: number;
  y: number;
  k: number;
}

export type Target = { type: "node"; index: number } | { type: "edge"; id: number } | null;

const pairKey = (a: number, b: number) => (a < b ? `${a}:${b}` : `${b}:${a}`);

/**
 * Одна линия на пару узлов и вид: ссылки A→B и B→A рисуются одной линией со стрелкой на каждом конце.
 * Разные виды одной пары смещаются параллельно, чтобы не лежать друг на друге.
 */
export interface Drawn {
  link: PLink;
  /** Смещение линии в px поперёк хорды, от меньшего индекса к большему. */
  offset: number;
  /** Концы, на которых рисуется стрелка (индексы узлов). */
  arrowsAt: number[];
  /** Идентификаторы всех рёбер, которые изображает линия (ссылка в обе стороны — два). */
  ids: number[];
}

const KIND_SLOT: EdgeKind[] = ["markdown_link", "document_similarity", "chunk_similarity", "query_result"];

export function layoutLines(links: PLink[], kinds: Record<EdgeKind, boolean>): Drawn[] {
  const groups = new Map<string, PLink[]>();
  for (const l of links) {
    if (!kinds[l.edge.kind]) continue;
    const key = `${pairKey(l.source.index, l.target.index)}|${l.edge.kind}`;
    groups.set(key, [...(groups.get(key) ?? []), l]);
  }
  const perPair = new Map<string, EdgeKind[]>();
  for (const key of groups.keys()) {
    const [pair, kind] = key.split("|");
    perPair.set(pair, [...(perPair.get(pair) ?? []), kind as EdgeKind]);
  }
  const lines: Drawn[] = [];
  for (const [key, list] of groups) {
    const [pair, kind] = key.split("|");
    const present = (perPair.get(pair) ?? []).sort((x, y) => KIND_SLOT.indexOf(x) - KIND_SLOT.indexOf(y));
    const slot = present.indexOf(kind as EdgeKind);
    const first = list[0];
    lines.push({
      link: first,
      offset: (slot - (present.length - 1) / 2) * 3,
      arrowsAt: kind === "markdown_link" ? [...new Set(list.map((l) => l.target.index))] : [],
      ids: list.map((l) => l.edge.id),
    });
  }
  return lines;
}

/** Концы линии с учётом смещения. */
export function lineEnds(d: Drawn, cam: Camera): [number, number, number, number] {
  const a = d.link.source.index < d.link.target.index ? d.link.source : d.link.target;
  const b = a === d.link.source ? d.link.target : d.link.source;
  const [ax, ay] = toScreen(cam, a.x, a.y);
  const [bx, by] = toScreen(cam, b.x, b.y);
  const len = Math.hypot(bx - ax, by - ay) || 1;
  const nx = -(by - ay) / len;
  const ny = (bx - ax) / len;
  return [ax + nx * d.offset, ay + ny * d.offset, bx + nx * d.offset, by + ny * d.offset];
}

export interface Scene {
  w: number;
  h: number;
  dpr: number;
  cam: Camera;
  nodes: PNode[];
  links: PLink[];
  palette: Palette;
  kinds: Record<EdgeKind, boolean>;
  selected: Target;
  hovered: Target;
  /** Узлы, связанные с выбранным; пустое множество — выбора нет. */
  related: Set<number>;
  /** Узлы, отмеченные фильтром списка или поиском по узлам. */
  marked: Set<number>;
  highlighted?: Set<number>;
  groups: { owner: number; shown: number; total: number }[];
  mini: boolean;
  /** Цвет модуля (идентификатор палитры) для узлов без назначенного цвета. */
  modules: Map<string, string>;
  /** Назначенный цвет документа или папки по Т-38 (идентификатор палитры); чанк берёт цвет документа. */
  assigned?: (node: GNode) => string | null;
  /** Документ в центре локального графа мини-панели. */
  focus?: number | null;
}

/** До стольких узлов локальный граф мини-панели подписан целиком. */
export const LOCAL_LABEL_ALL = 25;

/** Масштаб, ниже которого стрелки скрыты на неподсвеченных рёбрах (общий план). */
export const ARROW_ZOOM = 0.6;

/** Радиус узла на экране: растёт с масштабом медленнее самой карты и не меньше 2 px. */
export function radiusOf(n: PNode, cam: Camera): number {
  return Math.max(n.node.kind === "chunk" ? 1.6 : 2, n.r * Math.pow(cam.k, 0.8));
}

/** Непрозрачность подписи по экранному радиусу узла: хабы подписаны раньше, с приближением — все. */
export const labelAlpha = (r: number): number => Math.min(1, Math.max(0, (r - 3.5) / 2));

/** Толщина ребра в покое: ссылка 1 px, близость 0,5–1,5 px по силе окна модели, связь запроса — прежняя шкала. */
export function restWidth(kind: EdgeKind, s: number): number {
  if (kind === "markdown_link") return 1;
  if (kind === "query_result") return widthOf(s);
  return 0.5 + 1.0 * s;
}

/** Непрозрачность ребра в покое: близость слабее — прозрачнее. */
export function restAlpha(kind: EdgeKind, s: number): number {
  return kind === "document_similarity" || kind === "chunk_similarity" ? 0.35 + 0.55 * s : 1;
}

export function toScreen(cam: Camera, x: number, y: number): [number, number] {
  return [x * cam.k + cam.x, y * cam.k + cam.y];
}
export function toWorld(cam: Camera, sx: number, sy: number): [number, number] {
  return [(sx - cam.x) / cam.k, (sy - cam.y) / cam.k];
}

function trim(s: string, max: number): string {
  return s.length > max ? `${s.slice(0, max - 1)}…` : s;
}

const rgb = (c: [number, number, number]) => `rgb(${c[0]},${c[1]},${c[2]})`;

/** Узел и его соседи по видимым рёбрам: окружение наведения. */
function neighbours(links: PLink[], kinds: Record<EdgeKind, boolean>, index: number): Set<number> {
  const set = new Set<number>([index]);
  for (const l of links) {
    if (!kinds[l.edge.kind]) continue;
    if (l.source.index === index) set.add(l.target.index);
    else if (l.target.index === index) set.add(l.source.index);
  }
  return set;
}

export function nodeFill(n: PNode, palette: Palette, modules: Map<string, string>, assigned?: Scene["assigned"]): string {
  if (n.node.kind === "query") return palette.kindCss.query_result;
  const own = assigned?.(n.node);
  if (own && palette.pal[own]) return palette.pal[own];
  if (n.node.color) return palette.pal[n.node.color] ?? n.node.color;
  return palette.pal[modules.get(n.node.module) ?? "gray"] ?? palette.muted;
}

export function draw(ctx: CanvasRenderingContext2D, scene: Scene): void {
  const { w, h, dpr, cam, palette, selected, hovered } = scene;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  // Наведение подсвечивает окружение узла, как в Obsidian; без наведения — окружение выбора.
  const hoverNode = hovered?.type === "node" ? hovered.index : null;
  const around = hoverNode !== null ? neighbours(scene.links, scene.kinds, hoverNode) : null;
  const centre = hoverNode ?? (selected?.type === "node" ? selected.index : null);
  const dimOthers = around !== null || scene.related.size > 0 || selected?.type === "edge";

  // Группы раскрытых документов — контейнер владельца, не ребро.
  const byIndex = new Map(scene.nodes.map((n) => [n.index, n]));
  for (const g of scene.groups) {
    const owner = byIndex.get(g.owner);
    if (!owner) continue;
    let radius = 0;
    for (const n of scene.nodes) if (n.node.kind === "chunk" && n.node.owner === g.owner) radius = Math.max(radius, Math.hypot(n.x - owner.x, n.y - owner.y));
    const [sx, sy] = toScreen(cam, owner.x, owner.y);
    const r = (radius + 10) * cam.k;
    ctx.beginPath();
    ctx.arc(sx, sy, Math.max(r, 14), 0, Math.PI * 2);
    ctx.fillStyle = palette.accentSoft;
    ctx.globalAlpha = 0.35;
    ctx.fill();
    ctx.globalAlpha = 1;
    ctx.setLineDash([3, 3]);
    ctx.strokeStyle = palette.faint;
    ctx.lineWidth = 1;
    ctx.stroke();
    ctx.setLineDash([]);
  }

  // Рёбра под узлами: сначала фон, затем подсвеченные, чтобы выбор не прятался.
  const incident = (d: Drawn) => centre !== null && (d.link.source.index === centre || d.link.target.index === centre);
  const chosenEdge = (d: Drawn) => selected?.type === "edge" && d.ids.includes(selected.id);
  const lines = layoutLines(scene.links, scene.kinds).sort((a, b) => Number(incident(a) || chosenEdge(a)) - Number(incident(b) || chosenEdge(b)));
  for (const d of lines) {
    const l = d.link;
    const kind = l.edge.kind;
    const emphasised = incident(d) || chosenEdge(d) || d.ids.some(id => scene.highlighted?.has(id));
    const faded = dimOthers && !emphasised;
    const s = l.edge.neutral ? 0.5 : drawStrength(l.edge);
    const [ax, ay, bx, by] = lineEnds(d, cam);
    const width = restWidth(kind, s) + (emphasised ? 0.8 : 0);
    // В покое — нейтральный серый (у близости с примесью цвета вида), при подсветке и у связи запроса — цвет вида.
    const color = emphasised || kind === "query_result" ? rgb(palette.kind[kind]) : kind === "markdown_link" ? rgb(palette.edge) : mixColor(palette.kind[kind], palette.edge, 0.3);
    ctx.globalAlpha = faded ? 0.15 : emphasised ? 1 : restAlpha(kind, s);
    if (chosenEdge(d)) {
      ctx.strokeStyle = palette.accent;
      ctx.globalAlpha = 0.28;
      ctx.lineWidth = width + 5;
      pathEdge(ctx, ax, ay, bx, by);
      ctx.stroke();
      ctx.globalAlpha = 1;
    }
    ctx.strokeStyle = color;
    ctx.lineWidth = width;
    ctx.setLineDash(kind === "chunk_similarity" ? [5, 3] : l.edge.neutral ? [4, 3] : []);
    pathEdge(ctx, ax, ay, bx, by);
    ctx.stroke();
    ctx.setLineDash([]);
    // Стрелки — маленькие тёмные треугольники у края узла; на общем плане только у подсвеченных рёбер.
    if (d.arrowsAt.length && (emphasised || cam.k >= ARROW_ZOOM)) {
      ctx.fillStyle = emphasised ? color : palette.muted;
      for (const end of d.arrowsAt) {
        const atB = (l.source.index < l.target.index ? l.target : l.source).index === end;
        const node = l.source.index === end ? l.source : l.target;
        arrow(ctx, atB ? ax : bx, atB ? ay : by, atB ? bx : ax, atB ? by : ay, radiusOf(node, cam), cam.k);
      }
    }
  }
  ctx.globalAlpha = 1;

  // Узлы: цвет назначенный по Т-38 или цвет модуля, размер по числу связей.
  const isRelated = (n: PNode) => {
    if (!dimOthers) return true;
    if (around) return around.has(n.index);
    if (selected?.type === "node") return n.index === selected.index || scene.related.has(n.index);
    return scene.related.has(n.index);
  };
  for (const n of scene.nodes) {
    const [sx, sy] = toScreen(cam, n.x, n.y);
    const r = radiusOf(n, cam);
    if (sx < -r - 20 || sy < -r - 20 || sx > w + r + 20 || sy > h + r + 20) continue;
    ctx.globalAlpha = isRelated(n) ? 1 : 0.3;
    ctx.fillStyle = n.node.ghost ? palette.bgCss : nodeFill(n, palette, scene.modules, scene.assigned);
    ctx.strokeStyle = palette.bgCss;
    ctx.lineWidth = 1;
    ctx.beginPath();
    if (n.node.kind === "document") ctx.arc(sx, sy, r, 0, Math.PI * 2);
    else if (n.node.kind === "query") ctx.rect(sx - r, sy - r, r * 2, r * 2);
    else {
      ctx.moveTo(sx, sy - r * 1.25);
      ctx.lineTo(sx + r * 1.25, sy);
      ctx.lineTo(sx, sy + r * 1.25);
      ctx.lineTo(sx - r * 1.25, sy);
      ctx.closePath();
    }
    ctx.fill();
    ctx.stroke();
    if (n.node.qrole) {
      ctx.setLineDash(n.node.qrole === "related" || n.node.ghost ? [3, 3] : []);
      // Результат поиска: кольцо цвета связи запроса.
      ctx.beginPath();
      ctx.arc(sx, sy, r + 3, 0, Math.PI * 2);
      ctx.strokeStyle = palette.kindCss.query_result;
      ctx.lineWidth = 1.8;
      ctx.stroke();
    }
    ctx.setLineDash([]);
    const chosen = selected?.type === "node" && selected.index === n.index;
    const hot = hoverNode === n.index;
    const marked = scene.marked.has(n.index);
    const focus = scene.focus === n.index;
    if (chosen || hot || marked || focus) {
      ctx.globalAlpha = 1;
      ctx.beginPath();
      ctx.arc(sx, sy, r + (chosen || focus ? 4 : 3), 0, Math.PI * 2);
      ctx.strokeStyle = chosen || focus ? palette.accent : palette.muted;
      ctx.lineWidth = chosen || focus ? 2 : 1.25;
      ctx.stroke();
    }
    if (n.pinned) {
      ctx.globalAlpha = 1;
      ctx.fillStyle = palette.accent;
      ctx.fillRect(sx + r - 1, sy - r - 4, 4, 4);
    }
  }
  ctx.globalAlpha = 1;
  labels(ctx, scene, around);
}

function pathEdge(ctx: CanvasRenderingContext2D, ax: number, ay: number, bx: number, by: number): void {
  ctx.beginPath();
  ctx.moveTo(ax, ay);
  ctx.lineTo(bx, by);
}

/** Стрелка на конце линии (fromX,fromY → toX,toY) у края узла радиуса r: 4–6 px. */
function arrow(ctx: CanvasRenderingContext2D, fromX: number, fromY: number, toX: number, toY: number, r: number, k: number): void {
  const angle = Math.atan2(toY - fromY, toX - fromX);
  const back = r + 1.5;
  const tx = toX - Math.cos(angle) * back;
  const ty = toY - Math.sin(angle) * back;
  const size = 4 + 2 * Math.min(1, Math.max(0, (k - ARROW_ZOOM) / 1.4));
  ctx.beginPath();
  ctx.moveTo(tx, ty);
  ctx.lineTo(tx - Math.cos(angle - 0.45) * size, ty - Math.sin(angle - 0.45) * size);
  ctx.lineTo(tx - Math.cos(angle + 0.45) * size, ty - Math.sin(angle + 0.45) * size);
  ctx.closePath();
  ctx.fill();
}

/** Подписи под узлами: проявляются по экранному радиусу; выбранный, наведённый, соседи, закреплённые и запрос — всегда. */
function labels(ctx: CanvasRenderingContext2D, scene: Scene, around: Set<number> | null): void {
  const { cam, palette, selected, hovered, nodes } = scene;
  ctx.font = '11px "Segoe UI Variable Text","Segoe UI",system-ui,sans-serif';
  ctx.textBaseline = "top";
  ctx.textAlign = "center";
  const near = (n: PNode) => scene.related.has(n.index) || (around?.has(n.index) ?? false);
  const rank = (n: PNode): number => {
    if (selected?.type === "node" && selected.index === n.index) return 1e9;
    if (hovered?.type === "node" && hovered.index === n.index) return 9.8e8;
    if (n.node.kind === "query") return 9.5e8;
    if (scene.focus === n.index) return 9.2e8;
    if (near(n)) return 5e8 + n.degree;
    if (n.pinned) return 4e8;
    return n.degree + (n.node.kind === "document" ? 0.5 : 0);
  };
  const placed: [number, number, number, number][] = [];
  for (const n of [...nodes].sort((a, b) => rank(b) - rank(a))) {
    const always = rank(n) >= 4e8;
    const r = radiusOf(n, cam);
    // Небольшой локальный граф мини-панели подписан весь, пока подписи не наползают; у хаба с сотней соседей — по размеру, как везде.
    const alpha = always || (scene.mini && scene.focus != null && nodes.length <= LOCAL_LABEL_ALL) ? 1 : labelAlpha(r);
    if (alpha <= 0.02) continue;
    const [sx, sy] = toScreen(cam, n.x, n.y);
    if (sx < -40 || sy < -20 || sx > scene.w + 40 || sy > scene.h + 20) continue;
    const text = trim(n.node.label, scene.mini ? 24 : 32);
    const width = ctx.measureText(text).width;
    const top = sy + r + 2;
    const box: [number, number, number, number] = [sx - width / 2 - 1, top, sx + width / 2 + 1, top + 13];
    if (placed.some((p) => box[0] < p[2] && box[2] > p[0] && box[1] < p[3] && box[3] > p[1])) {
      if (rank(n) < 9e8) continue;
    }
    placed.push(box);
    ctx.globalAlpha = 0.9 * alpha;
    ctx.lineWidth = 3;
    ctx.strokeStyle = palette.bgCss;
    ctx.strokeText(text, sx, top);
    ctx.globalAlpha = alpha;
    ctx.fillStyle = rank(n) >= 5e8 ? palette.text : palette.muted;
    ctx.fillText(text, sx, top);
  }
  ctx.globalAlpha = 1;
}

/** Расстояние от точки до нарисованной линии в px. */
export function lineDistance(d: Drawn, cam: Camera, px: number, py: number): number {
  const [x0, y0, x1, y1] = lineEnds(d, cam);
  const dx = x1 - x0;
  const dy = y1 - y0;
  const len2 = dx * dx + dy * dy || 1;
  const t = Math.max(0, Math.min(1, ((px - x0) * dx + (py - y0) * dy) / len2));
  return Math.hypot(px - (x0 + t * dx), py - (y0 + t * dy));
}
