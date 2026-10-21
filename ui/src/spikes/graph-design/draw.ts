// Отрисовка Canvas2D стенда: сила связи — толщина и яркость, вид — цвет, штрих и стрелка, выбор — ореол.
import type { PLink, PNode } from "./layout";
import { brightnessOf, drawStrength, mixColor, parseColor, widthOf, type EdgeKind, type ScaleName } from "./model";

export interface Palette {
  bg: [number, number, number];
  bgCss: string;
  text: string;
  muted: string;
  faint: string;
  accent: string;
  accentSoft: string;
  line: string;
  kind: Record<EdgeKind, [number, number, number]>;
  kindCss: Record<EdgeKind, string>;
}

const KIND_COLORS = {
  light: { markdown_link: "#d6336c", document_similarity: "#2b7bb9", chunk_similarity: "#7b4fc2", query_result: "#d9730d" },
  dark: { markdown_link: "#f0568c", document_similarity: "#5eb0ef", chunk_similarity: "#b18cf0", query_result: "#ffa94d" },
};

export function readPalette(el: HTMLElement): Palette {
  const css = getComputedStyle(el);
  const get = (name: string) => css.getPropertyValue(name).trim();
  const dark = document.documentElement.dataset.mode === "dark";
  const set = KIND_COLORS[dark ? "dark" : "light"];
  return {
    bg: parseColor(get("--panel")),
    bgCss: get("--panel"),
    text: get("--text"),
    muted: get("--muted"),
    faint: get("--faint"),
    accent: get("--accent"),
    accentSoft: get("--accent-soft"),
    line: get("--line-2"),
    kind: {
      markdown_link: parseColor(set.markdown_link),
      document_similarity: parseColor(set.document_similarity),
      chunk_similarity: parseColor(set.chunk_similarity),
      query_result: parseColor(set.query_result),
    },
    kindCss: set,
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
      offset: (slot - (present.length - 1) / 2) * 4,
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
  scale: ScaleName;
  kinds: Record<EdgeKind, boolean>;
  selected: Target;
  hovered: Target;
  /** Узлы, связанные с выбранным; пустое множество — выбора нет. */
  related: Set<number>;
  /** Узлы, отмеченные фильтром списка или поиском по узлам. */
  marked: Set<number>;
  groups: { owner: number; shown: number; total: number }[];
  mini: boolean;
}

/** Радиус узла на экране: растёт с масштабом медленнее самой карты. */
export function radiusOf(n: PNode, cam: Camera): number {
  return Math.max(n.node.kind === "document" ? 3.2 : 2.2, n.r * Math.min(1.4, Math.max(0.55, Math.sqrt(cam.k))));
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

export function draw(ctx: CanvasRenderingContext2D, scene: Scene): void {
  const { w, h, dpr, cam, palette, selected } = scene;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  const dimOthers = scene.related.size > 0 || selected?.type === "edge";

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

  // Рёбра: сначала приглушённые, затем подсвеченные, чтобы выбор не прятался под фоном.
  const incident = (d: Drawn) => selected?.type === "node" && (d.link.source.index === selected.index || d.link.target.index === selected.index);
  const chosenEdge = (d: Drawn) => selected?.type === "edge" && d.ids.includes(selected.id);
  const lines = layoutLines(scene.links, scene.kinds).sort((a, b) => Number(incident(a) || chosenEdge(a)) - Number(incident(b) || chosenEdge(b)));
  for (const d of lines) {
    const l = d.link;
    const emphasised = incident(d) || chosenEdge(d);
    const faded = dimOthers && !emphasised;
    const s = drawStrength(l.edge, scene.scale);
    const [ax, ay, bx, by] = lineEnds(d, cam);
    const width = widthOf(s);
    const color = mixColor(palette.kind[l.edge.kind], palette.bg, brightnessOf(s));
    ctx.globalAlpha = faded ? 0.14 : 1;
    if (chosenEdge(d)) {
      ctx.strokeStyle = palette.accent;
      ctx.globalAlpha = 0.28;
      ctx.lineWidth = width + 5;
      pathEdge(ctx, ax, ay, bx, by);
      ctx.stroke();
      ctx.globalAlpha = 1;
    }
    ctx.strokeStyle = color;
    ctx.fillStyle = color;
    ctx.lineWidth = width;
    ctx.setLineDash(l.edge.kind === "chunk_similarity" ? [5, 3] : []);
    pathEdge(ctx, ax, ay, bx, by);
    ctx.stroke();
    ctx.setLineDash([]);
    for (const end of d.arrowsAt) {
      const atB = (l.source.index < l.target.index ? l.target : l.source).index === end;
      const node = l.source.index === end ? l.source : l.target;
      arrow(ctx, atB ? ax : bx, atB ? ay : by, atB ? bx : ax, atB ? by : ay, radiusOf(node, cam), width, cam.k);
    }
  }
  ctx.globalAlpha = 1;

  // Узлы.
  const isRelated = (n: PNode) => !dimOthers || (selected?.type === "node" && (n.index === selected.index || scene.related.has(n.index))) || (selected?.type === "edge" && scene.related.has(n.index));
  for (const n of scene.nodes) {
    const [sx, sy] = toScreen(cam, n.x, n.y);
    if (sx < -20 || sy < -20 || sx > w + 20 || sy > h + 20) continue;
    const r = radiusOf(n, cam);
    ctx.globalAlpha = isRelated(n) ? 1 : 0.28;
    const fill = n.node.kind === "query" ? palette.accent : (n.node.color ?? (n.node.kind === "document" ? palette.muted : palette.faint));
    ctx.fillStyle = fill;
    ctx.strokeStyle = palette.bgCss;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    if (n.node.kind === "query") {
      const q = r * 1.5;
      ctx.rect(sx - q, sy - q, q * 2, q * 2);
    } else if (n.node.kind === "document") ctx.arc(sx, sy, r, 0, Math.PI * 2);
    else {
      ctx.moveTo(sx, sy - r * 1.25);
      ctx.lineTo(sx + r * 1.25, sy);
      ctx.lineTo(sx, sy + r * 1.25);
      ctx.lineTo(sx - r * 1.25, sy);
      ctx.closePath();
    }
    ctx.fill();
    ctx.stroke();
    const chosen = selected?.type === "node" && selected.index === n.index;
    const hot = scene.hovered?.type === "node" && scene.hovered.index === n.index;
    const marked = scene.marked.has(n.index);
    if (chosen || hot || marked) {
      ctx.globalAlpha = 1;
      ctx.beginPath();
      ctx.arc(sx, sy, r + (chosen ? 5 : 4), 0, Math.PI * 2);
      ctx.strokeStyle = chosen ? palette.accent : palette.muted;
      ctx.lineWidth = chosen ? 2 : 1.25;
      ctx.stroke();
    }
    if (n.pinned) {
      ctx.globalAlpha = 1;
      ctx.fillStyle = palette.accent;
      ctx.fillRect(sx + r - 1, sy - r - 4, 4, 4);
    }
  }
  ctx.globalAlpha = 1;
  labels(ctx, scene);
}

function pathEdge(ctx: CanvasRenderingContext2D, ax: number, ay: number, bx: number, by: number): void {
  ctx.beginPath();
  ctx.moveTo(ax, ay);
  ctx.lineTo(bx, by);
}

/** Стрелка на конце линии (fromX,fromY → toX,toY), отступ от края узла радиуса r. */
function arrow(ctx: CanvasRenderingContext2D, fromX: number, fromY: number, toX: number, toY: number, r: number, width: number, k: number): void {
  const angle = Math.atan2(toY - fromY, toX - fromX);
  const back = r + 3;
  const tx = toX - Math.cos(angle) * back;
  const ty = toY - Math.sin(angle) * back;
  const size = 4 + 3.5 * Math.min(1.3, k) + width * 0.8;
  ctx.beginPath();
  ctx.moveTo(tx, ty);
  ctx.lineTo(tx - Math.cos(angle - 0.4) * size, ty - Math.sin(angle - 0.4) * size);
  ctx.lineTo(tx - Math.cos(angle + 0.4) * size, ty - Math.sin(angle + 0.4) * size);
  ctx.closePath();
  ctx.fill();
}

function labels(ctx: CanvasRenderingContext2D, scene: Scene): void {
  const { cam, palette, selected, hovered, nodes } = scene;
  ctx.font = '11px "Segoe UI Variable Text","Segoe UI",system-ui,sans-serif';
  ctx.textBaseline = "middle";
  ctx.textAlign = "left";
  const budget = scene.mini ? 0 : Math.round(Math.min(70, Math.max(10, 8 + 14 * cam.k * cam.k)));
  const rank = (n: PNode): number => {
    if (selected?.type === "node" && selected.index === n.index) return 1e9;
    if (hovered?.type === "node" && hovered.index === n.index) return 9e8;
    if (scene.related.has(n.index)) return 5e8 + n.degree;
    if (n.pinned) return 4e8;
    return n.degree + (n.node.kind === "document" ? 0.5 : 0);
  };
  const cap = scene.related.size ? Math.max(budget, 14) : budget;
  const placed: [number, number, number, number][] = [];
  let used = 0;
  for (const n of [...nodes].sort((a, b) => rank(b) - rank(a))) {
    const must = rank(n) >= 9e8 || n.pinned || n.node.kind === "query";
    if (!must && (used >= cap || scene.mini)) continue;
    const [sx, sy] = toScreen(cam, n.x, n.y);
    if (sx < 0 || sy < 0 || sx > scene.w || sy > scene.h) continue;
    const text = trim(n.node.label, 28);
    const width = ctx.measureText(text).width;
    const box: [number, number, number, number] = [sx + n.r + 4, sy - 7, sx + n.r + 4 + width, sy + 7];
    if (!must && placed.some((p) => box[0] < p[2] && box[2] > p[0] && box[1] < p[3] && box[3] > p[1])) continue;
    placed.push(box);
    used++;
    ctx.lineWidth = 3;
    ctx.strokeStyle = palette.bgCss;
    ctx.globalAlpha = 0.9;
    ctx.strokeText(text, box[0], sy);
    ctx.globalAlpha = 1;
    ctx.fillStyle = rank(n) >= 5e8 ? palette.text : palette.muted;
    ctx.fillText(text, box[0], sy);
  }
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
