// Отрисовка стенда 2.3.1: снимок продуктового draw.ts (рисунок как в Obsidian, раунд 27) плюс формы уровней
// сущностей, временные виды (путь, влияние, контекст, локальный граф, аудит), подграф и значки состояний GR-05.
// Дизайн: docs/features/graphrag/graph-actions-design.md. В продукт не входит.
import type { PLink, PNode } from "../../lib/graph/layout";
import { drawStrength, mixColor, parseColor, widthOf, type GNode } from "../../lib/graph/model";
import type { XKind } from "./data";

export interface Palette {
  bg: [number, number, number];
  bgCss: string;
  text: string;
  muted: string;
  faint: string;
  accent: string;
  accentSoft: string;
  line: string;
  edge: [number, number, number];
  kind: Record<XKind, [number, number, number]>;
  kindCss: Record<XKind, string>;
  pal: Record<string, string>;
  warn: string;
  danger: string;
}

const KIND_COLORS: Record<"light" | "dark", Record<XKind, string>> = {
  light: { markdown_link: "#d6336c", document_similarity: "#2b7bb9", chunk_similarity: "#7b4fc2", query_result: "#d9730d", assertion: "#0f8a83", mention: "#8f8a96", unresolved: "#c23a3a", sequence: "#b9b4be", aggregate: "#5b6cd6" },
  dark: { markdown_link: "#f0568c", document_similarity: "#5eb0ef", chunk_similarity: "#b18cf0", query_result: "#ff9a4d", assertion: "#3cc4bc", mention: "#8a8590", unresolved: "#ec7070", sequence: "#55505a", aggregate: "#7d8cec" },
};

const EDGE_GRAY = { light: "#cfcfcf", dark: "#4b4b4b" };

const PALETTE = {
  light: { red: "#d04545", orange: "#d9782a", amber: "#b58a14", green: "#2f9a5c", teal: "#1f9a95", blue: "#3f84cf", violet: "#8059c9", pink: "#d14a80", gray: "#85808a" },
  dark: { red: "#ec7070", orange: "#f0a35c", amber: "#e0bb45", green: "#5cc287", teal: "#46c1bb", blue: "#6aa8ec", violet: "#a98cea", pink: "#ec79a6", gray: "#8a8590" },
};

export const MODULE_ORDER = ["blue", "green", "orange", "violet", "teal", "pink", "amber", "red"];

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
  const kind = {} as Record<XKind, [number, number, number]>;
  for (const [k, v] of Object.entries(set)) kind[k as XKind] = parseColor(v);
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
    kind,
    kindCss: set,
    pal,
    warn: dark ? "#ff9a4d" : "#d9730d",
    danger: dark ? "#ec7070" : "#c23a3a",
  };
}

export interface Camera {
  x: number;
  y: number;
  k: number;
}

export type Target = { type: "node"; index: number } | { type: "edge"; id: number } | null;

const pairKey = (a: number, b: number) => (a < b ? `${a}:${b}` : `${b}:${a}`);

export interface Drawn {
  link: PLink;
  offset: number;
  arrowsAt: number[];
  ids: number[];
}

const KIND_SLOT: XKind[] = ["markdown_link", "document_similarity", "chunk_similarity", "query_result", "assertion", "mention", "unresolved", "sequence", "aggregate"];
const ORIENTED = new Set<XKind>(["markdown_link", "assertion", "unresolved"]);
const kindOf = (l: PLink) => l.edge.kind as unknown as XKind;

export function layoutLines(links: PLink[], kinds: Record<string, boolean>): Drawn[] {
  const groups = new Map<string, PLink[]>();
  for (const l of links) {
    if (!kinds[kindOf(l)]) continue;
    // Утверждения одной пары не склеиваются: у каждого свой предикат.
    const key = `${pairKey(l.source.index, l.target.index)}|${kindOf(l)}${kindOf(l) === "assertion" ? `#${l.edge.id}` : ""}`;
    groups.set(key, [...(groups.get(key) ?? []), l]);
  }
  const perPair = new Map<string, string[]>();
  for (const key of groups.keys()) {
    const [pair, kind] = key.split("|");
    perPair.set(pair, [...(perPair.get(pair) ?? []), kind]);
  }
  const lines: Drawn[] = [];
  for (const [key, list] of groups) {
    const [pair, kind] = key.split("|");
    const base = kind.split("#")[0] as XKind;
    const present = (perPair.get(pair) ?? []).sort((x, y) => KIND_SLOT.indexOf(x.split("#")[0] as XKind) - KIND_SLOT.indexOf(y.split("#")[0] as XKind) || x.localeCompare(y));
    const slot = present.indexOf(kind);
    lines.push({
      link: list[0],
      offset: (slot - (present.length - 1) / 2) * 3,
      arrowsAt: ORIENTED.has(base) ? [...new Set(list.map((l) => l.target.index))] : [],
      ids: list.map((l) => l.edge.id),
    });
  }
  return lines;
}

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

/** Временный вид и отметки поверх карты. */
export interface Overlay {
  /** Узлы вида (путь, влияние, контекст, локальный граф, аудит): остальное приглушено. null — вида нет. */
  focusSet: Set<number> | null;
  /** Рёбра пути: цвет акцента и толщина выбора. */
  pathEdges: Set<number>;
  /** Номер шага пути у узла. */
  stepOf: Map<number, number>;
  /** Кольца влияния и центра. */
  rings: Map<number, "center" | "direct" | "indirect" | "neighbour" | "isolate" | "hub">;
  /** Буква роли контекста. */
  badges: Map<number, string>;
  /** Узлы подграфа. */
  subgraph: Set<number>;
  /** Группы документов уровня «Чанки». */
  hulls: { label: string; members: number[] }[];
  /** Отметки утверждений: отрицание, условие, конфликт, кандидат, подпись предиката. */
  edgeMeta: Map<number, { neg?: boolean; cond?: boolean; conflict?: boolean; candidate?: boolean; label?: string; count?: number }>;
  /** Выбор второй вершины пути: курсор-перекрестие на наведённом узле. */
  picking: boolean;
}

export const emptyOverlay = (): Overlay => ({ focusSet: null, pathEdges: new Set(), stepOf: new Map(), rings: new Map(), badges: new Map(), subgraph: new Set(), hulls: [], edgeMeta: new Map(), picking: false });

export interface Scene {
  w: number;
  h: number;
  dpr: number;
  cam: Camera;
  nodes: PNode[];
  links: PLink[];
  palette: Palette;
  kinds: Record<string, boolean>;
  selected: Target;
  hovered: Target;
  related: Set<number>;
  marked: Set<number>;
  groups: { owner: number; shown: number; total: number }[];
  mini: boolean;
  modules: Map<string, string>;
  assigned?: (node: GNode) => string | null;
  focus?: number | null;
  x: Overlay;
  /** Reduced motion: подсветка без пульса. */
  still: boolean;
  /** Фаза пульса подсветки (0…1) для выбранного результата. */
  pulse: number;
}

export const ARROW_ZOOM = 0.6;

type XN = Omit<PNode, "node"> & { node: Omit<GNode, "kind"> & { kind: string; members?: number; stale?: boolean; deleted?: boolean; ambiguous?: boolean } };
const nk = (n: PNode): string => n.node.kind;

export function radiusOf(n: PNode, cam: Camera): number {
  const x = n as unknown as XN;
  if (x.node.kind === "topic") return Math.max(9, (6 + Math.sqrt(x.node.members ?? 1) * 2.4) * Math.pow(cam.k, 0.8));
  return Math.max(nk(n) === "chunk" ? 1.6 : 2, n.r * Math.pow(cam.k, 0.8));
}

export const labelAlpha = (r: number): number => Math.min(1, Math.max(0, (r - 3.5) / 2));

export function restWidth(kind: XKind, s: number): number {
  if (kind === "markdown_link" || kind === "assertion" || kind === "unresolved") return 1;
  if (kind === "query_result") return widthOf(s);
  if (kind === "aggregate") return 0.8 + 3 * s;
  if (kind === "mention" || kind === "sequence") return 0.8;
  return 0.5 + 1.0 * s;
}

export function restAlpha(kind: XKind, s: number): number {
  if (kind === "document_similarity" || kind === "chunk_similarity") return 0.35 + 0.55 * s;
  if (kind === "sequence") return 0.5;
  if (kind === "mention") return 0.7;
  return 1;
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

function neighbours(links: PLink[], kinds: Record<string, boolean>, index: number): Set<number> {
  const set = new Set<number>([index]);
  for (const l of links) {
    if (!kinds[kindOf(l)]) continue;
    if (l.source.index === index) set.add(l.target.index);
    else if (l.target.index === index) set.add(l.source.index);
  }
  return set;
}

export function nodeFill(n: PNode, palette: Palette, modules: Map<string, string>, assigned?: Scene["assigned"]): string {
  const kind = nk(n);
  if (kind === "query") return palette.kindCss.query_result;
  if (kind === "entity") return palette.kindCss.assertion;
  if (kind === "topic") return palette.kindCss.aggregate;
  const own = assigned?.(n.node);
  if (own && palette.pal[own]) return palette.pal[own];
  if (n.node.color) return palette.pal[n.node.color] ?? n.node.color;
  return palette.pal[modules.get(n.node.module) ?? "gray"] ?? palette.muted;
}

function hexagon(ctx: CanvasRenderingContext2D, x: number, y: number, r: number): void {
  for (let i = 0; i < 6; i++) {
    const a = Math.PI / 6 + (i * Math.PI) / 3;
    if (i === 0) ctx.moveTo(x + r * Math.cos(a), y + r * Math.sin(a));
    else ctx.lineTo(x + r * Math.cos(a), y + r * Math.sin(a));
  }
  ctx.closePath();
}

function badge(ctx: CanvasRenderingContext2D, x: number, y: number, text: string, fill: string, fg: string): void {
  ctx.font = 'bold 9px "Segoe UI Variable Text","Segoe UI",system-ui,sans-serif';
  const w = Math.max(11, ctx.measureText(text).width + 6);
  ctx.globalAlpha = 1;
  ctx.fillStyle = fill;
  ctx.beginPath();
  ctx.roundRect(x - w / 2, y - 6, w, 12, 6);
  ctx.fill();
  ctx.fillStyle = fg;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(text, x, y + 0.5);
}

export function draw(ctx: CanvasRenderingContext2D, scene: Scene): void {
  const { w, h, dpr, cam, palette, selected, hovered, x } = scene;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  const hoverNode = hovered?.type === "node" ? hovered.index : null;
  const around = hoverNode !== null && !x.focusSet ? neighbours(scene.links, scene.kinds, hoverNode) : null;
  const centre = hoverNode ?? (selected?.type === "node" ? selected.index : null);
  const viewDim = !!x.focusSet;
  const dimOthers = viewDim || around !== null || scene.related.size > 0 || selected?.type === "edge";
  const byIndex = new Map(scene.nodes.map((n) => [n.index, n]));

  // Группы документов уровня «Чанки» и раскрытые документы: штриховой контейнер, не ребро.
  const hull = (members: PNode[], label?: string) => {
    if (!members.length) return;
    const cx = members.reduce((s, n) => s + n.x, 0) / members.length;
    const cy = members.reduce((s, n) => s + n.y, 0) / members.length;
    const radius = Math.max(...members.map((n) => Math.hypot(n.x - cx, n.y - cy)));
    const [sx, sy] = toScreen(cam, cx, cy);
    const r = Math.max((radius + 10) * cam.k, 12);
    ctx.beginPath();
    ctx.arc(sx, sy, r, 0, Math.PI * 2);
    ctx.fillStyle = palette.accentSoft;
    ctx.globalAlpha = viewDim ? 0.12 : 0.28;
    ctx.fill();
    ctx.globalAlpha = viewDim ? 0.3 : 1;
    ctx.setLineDash([3, 3]);
    ctx.strokeStyle = palette.faint;
    ctx.lineWidth = 1;
    ctx.stroke();
    ctx.setLineDash([]);
    if (label && r > 22) {
      ctx.font = '10px "Segoe UI Variable Text","Segoe UI",system-ui,sans-serif';
      ctx.textAlign = "center";
      ctx.textBaseline = "bottom";
      ctx.fillStyle = palette.faint;
      ctx.fillText(trim(label, Math.max(12, Math.round(r / 3.5))), sx, sy - r - 2);
    }
    ctx.globalAlpha = 1;
  };
  for (const g of scene.groups) {
    const owner = byIndex.get(g.owner);
    if (!owner) continue;
    hull([owner, ...scene.nodes.filter((n) => n.node.kind === "chunk" && n.node.owner === g.owner)]);
  }
  // Группы документов уровня «Чанки»: только группа выбранного или наведённого чанка — сотни кругов закрыли бы карту.
  const hot = new Set([selected?.type === "node" ? selected.index : -1, hovered?.type === "node" ? hovered.index : -1]);
  for (const g of x.hulls) if (g.members.some((i) => hot.has(i))) hull(g.members.map((i) => byIndex.get(i)).filter((n): n is PNode => !!n), g.label);

  // Рёбра под узлами.
  const incident = (d: Drawn) => !viewDim && centre !== null && (d.link.source.index === centre || d.link.target.index === centre);
  const chosenEdge = (d: Drawn) => selected?.type === "edge" && d.ids.includes(selected.id);
  const onPath = (d: Drawn) => d.ids.some((id) => x.pathEdges.has(id));
  const inView = (d: Drawn) => viewDim && x.focusSet!.has(d.link.source.index) && x.focusSet!.has(d.link.target.index);
  const order = (d: Drawn) => (onPath(d) ? 3 : chosenEdge(d) || incident(d) ? 2 : inView(d) ? 1 : 0);
  const lines = layoutLines(scene.links, scene.kinds).sort((a, b) => order(a) - order(b));
  for (const d of lines) {
    const l = d.link;
    const kind = kindOf(l);
    const meta = x.edgeMeta.get(l.edge.id);
    const path = onPath(d);
    const emphasised = path || incident(d) || chosenEdge(d) || inView(d);
    const faded = dimOthers && !emphasised;
    const s = l.edge.neutral ? 0.5 : kind === "aggregate" ? l.edge.s : drawStrength(l.edge as never);
    const [ax, ay, bx, by] = lineEnds(d, cam);
    const width = restWidth(kind, s) + (path ? 1.8 : emphasised ? 0.8 : 0);
    const color = path ? palette.accent : emphasised || kind === "query_result" || kind === "assertion" || kind === "unresolved" ? rgb(palette.kind[kind]) : kind === "markdown_link" ? rgb(palette.edge) : mixColor(palette.kind[kind], palette.edge, 0.3);
    ctx.globalAlpha = faded ? 0.12 : emphasised ? 1 : restAlpha(kind, s);
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
    const dash =
      kind === "chunk_similarity" ? [5, 3]
      : kind === "mention" ? [1, 3]
      : kind === "unresolved" ? [3, 3]
      : meta?.neg ? [6, 2, 1, 2]
      : meta?.candidate ? [2, 3]
      : l.edge.neutral ? [4, 3]
      : [];
    ctx.setLineDash(dash);
    pathEdge(ctx, ax, ay, bx, by);
    ctx.stroke();
    ctx.setLineDash([]);
    if (d.arrowsAt.length && (emphasised || cam.k >= ARROW_ZOOM || kind === "assertion")) {
      ctx.fillStyle = emphasised ? color : kind === "assertion" ? color : palette.muted;
      for (const end of d.arrowsAt) {
        const atB = (l.source.index < l.target.index ? l.target : l.source).index === end;
        const node = l.source.index === end ? l.source : l.target;
        arrow(ctx, atB ? ax : bx, atB ? ay : by, atB ? bx : ax, atB ? by : ay, radiusOf(node, cam), cam.k);
      }
    }
    // Отметки посередине линии: «не», «если», «!», число связей агрегата, подпись предиката.
    if (!faded && (meta || kind === "aggregate")) {
      const mx = (ax + bx) / 2;
      const my = (ay + by) / 2;
      let off = 0;
      if (kind === "aggregate" && meta?.count) badge(ctx, mx, my, String(meta.count), palette.bgCss, palette.text);
      if (meta?.neg) badge(ctx, mx + (off++ * 16), my, "не", palette.bgCss, palette.danger);
      if (meta?.cond) badge(ctx, mx + (off++ * 16), my, "если", palette.bgCss, palette.warn);
      if (meta?.conflict) badge(ctx, mx + (off++ * 16), my, "!", palette.danger, "#fff");
      if (meta?.label && (emphasised || cam.k >= 1.4)) {
        ctx.font = '10px "Segoe UI Variable Text","Segoe UI",system-ui,sans-serif';
        ctx.textAlign = "center";
        ctx.textBaseline = "bottom";
        ctx.lineWidth = 3;
        ctx.strokeStyle = palette.bgCss;
        ctx.strokeText(meta.label, mx, my - 7);
        ctx.fillStyle = color;
        ctx.fillText(meta.label, mx, my - 7);
      }
    }
  }
  ctx.globalAlpha = 1;

  const isRelated = (n: PNode) => {
    if (viewDim) return x.focusSet!.has(n.index);
    if (!dimOthers) return true;
    if (around) return around.has(n.index);
    if (selected?.type === "node") return n.index === selected.index || scene.related.has(n.index);
    return scene.related.has(n.index);
  };
  for (const pn of scene.nodes) {
    const n = pn as unknown as XN;
    const [sx, sy] = toScreen(cam, n.x, n.y);
    const r = radiusOf(pn, cam);
    if (sx < -r - 20 || sy < -r - 20 || sx > w + r + 20 || sy > h + r + 20) continue;
    const kind = n.node.kind;
    const hollow = kind === "ghost" || n.node.deleted;
    ctx.globalAlpha = (isRelated(pn) ? 1 : 0.22) * (n.node.stale ? 0.5 : 1);
    ctx.fillStyle = nodeFill(pn, palette, scene.modules, scene.assigned);
    ctx.strokeStyle = palette.bgCss;
    ctx.lineWidth = 1;
    ctx.beginPath();
    if (kind === "document" || kind === "topic" || kind === "ghost") ctx.arc(sx, sy, r, 0, Math.PI * 2);
    else if (kind === "query") ctx.rect(sx - r, sy - r, r * 2, r * 2);
    else if (kind === "entity") hexagon(ctx, sx, sy, r * 1.25);
    else {
      ctx.moveTo(sx, sy - r * 1.25);
      ctx.lineTo(sx + r * 1.25, sy);
      ctx.lineTo(sx, sy + r * 1.25);
      ctx.lineTo(sx - r * 1.25, sy);
      ctx.closePath();
    }
    if (hollow) {
      ctx.setLineDash([2, 2]);
      ctx.strokeStyle = kind === "ghost" ? palette.danger : palette.muted;
      ctx.lineWidth = 1.4;
      ctx.stroke();
      ctx.setLineDash([]);
      if (kind === "ghost" && r > 3) badge(ctx, sx, sy, "?", palette.bgCss, palette.danger);
      if (n.node.deleted) {
        ctx.beginPath();
        ctx.moveTo(sx - r, sy + r);
        ctx.lineTo(sx + r, sy - r);
        ctx.stroke();
      }
    } else {
      ctx.fill();
      ctx.stroke();
    }
    if (kind === "topic") {
      ctx.globalAlpha = 1;
      ctx.font = 'bold 10px "Segoe UI Variable Text","Segoe UI",system-ui,sans-serif';
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillStyle = "#fff";
      ctx.fillText(String(n.node.members ?? ""), sx, sy);
    }
    if (n.node.ambiguous) {
      ctx.globalAlpha = 1;
      ctx.beginPath();
      hexagon(ctx, sx, sy, r * 1.25 + 3);
      ctx.strokeStyle = palette.warn;
      ctx.lineWidth = 1.2;
      ctx.stroke();
      badge(ctx, sx + r + 6, sy - r - 4, "?", palette.warn, "#fff");
    }
    if (n.node.stale) {
      ctx.globalAlpha = 1;
      // Часы: устарел после снимка.
      ctx.beginPath();
      ctx.arc(sx + r + 4, sy - r - 3, 4, 0, Math.PI * 2);
      ctx.fillStyle = palette.bgCss;
      ctx.fill();
      ctx.strokeStyle = palette.warn;
      ctx.lineWidth = 1.2;
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(sx + r + 4, sy - r - 5.5);
      ctx.lineTo(sx + r + 4, sy - r - 3);
      ctx.lineTo(sx + r + 6, sy - r - 2);
      ctx.stroke();
    }
    if (n.node.qrole === "result") {
      ctx.beginPath();
      ctx.arc(sx, sy, r + 3, 0, Math.PI * 2);
      ctx.strokeStyle = palette.kindCss.query_result;
      ctx.lineWidth = 1.8;
      ctx.stroke();
    }
    const ring = x.rings.get(n.index);
    if (ring) {
      ctx.globalAlpha = 1;
      ctx.beginPath();
      ctx.arc(sx, sy, r + (ring === "center" ? 6 : 4), 0, Math.PI * 2);
      ctx.strokeStyle = ring === "neighbour" || ring === "isolate" ? palette.muted : ring === "hub" ? palette.kindCss.aggregate : ring === "center" ? palette.accent : palette.warn;
      ctx.lineWidth = ring === "center" ? 2 : 1.8;
      ctx.setLineDash(ring === "indirect" || ring === "isolate" ? [3, 2] : []);
      ctx.stroke();
      ctx.setLineDash([]);
      if (ring === "center") {
        ctx.beginPath();
        ctx.arc(sx, sy, r + 9, 0, Math.PI * 2);
        ctx.lineWidth = 1;
        ctx.stroke();
      }
    }
    const chosen = selected?.type === "node" && selected.index === n.index;
    const hot = hoverNode === n.index;
    const marked = scene.marked.has(n.index);
    const focus = scene.focus === n.index;
    if (chosen || hot || marked || focus) {
      ctx.globalAlpha = 1;
      ctx.beginPath();
      const pulse = chosen && !scene.still ? scene.pulse * 4 : 0;
      ctx.arc(sx, sy, r + (chosen || focus ? 4 : 3) + pulse, 0, Math.PI * 2);
      ctx.strokeStyle = chosen || focus ? palette.accent : palette.muted;
      ctx.lineWidth = chosen || focus ? 2 : 1.25;
      if (hot && x.picking) ctx.setLineDash([2, 2]);
      ctx.stroke();
      ctx.setLineDash([]);
    }
    if (x.subgraph.has(n.index)) {
      // Подграф: маленький квадрат-галочка слева сверху и тонкое кольцо.
      ctx.globalAlpha = 1;
      ctx.beginPath();
      ctx.arc(sx, sy, r + 2.2, 0, Math.PI * 2);
      ctx.strokeStyle = palette.accent;
      ctx.lineWidth = 1;
      ctx.stroke();
      ctx.fillStyle = palette.accent;
      ctx.fillRect(sx - r - 7, sy - r - 7, 7, 7);
      ctx.strokeStyle = "#fff";
      ctx.lineWidth = 1.3;
      ctx.beginPath();
      ctx.moveTo(sx - r - 5.8, sy - r - 3.6);
      ctx.lineTo(sx - r - 4.2, sy - r - 2.2);
      ctx.lineTo(sx - r - 1.4, sy - r - 5.6);
      ctx.stroke();
    }
    const step = x.stepOf.get(n.index);
    if (step !== undefined) badge(ctx, sx, sy - r - 9, String(step), palette.accent, "#fff");
    const role = x.badges.get(n.index);
    if (role) badge(ctx, sx + r + 7, sy - r - 3, role, palette.text, palette.bgCss);
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

function labels(ctx: CanvasRenderingContext2D, scene: Scene, around: Set<number> | null): void {
  const { cam, palette, selected, hovered, nodes, x } = scene;
  ctx.font = '11px "Segoe UI Variable Text","Segoe UI",system-ui,sans-serif';
  ctx.textBaseline = "top";
  ctx.textAlign = "center";
  const near = (n: PNode) => scene.related.has(n.index) || (around?.has(n.index) ?? false);
  const rank = (n: PNode): number => {
    if (selected?.type === "node" && selected.index === n.index) return 1e9;
    if (hovered?.type === "node" && hovered.index === n.index) return 9.8e8;
    if (n.node.kind === "query") return 9.5e8;
    if (scene.focus === n.index || x.rings.get(n.index) === "center") return 9.2e8;
    if (x.stepOf.has(n.index)) return 9e8;
    if (nk(n) === "topic") return 8e8;
    if (x.focusSet?.has(n.index) && (x.rings.has(n.index) || x.badges.has(n.index))) return 6e8 + n.degree;
    if (near(n)) return 5e8 + n.degree;
    if (n.pinned) return 4e8;
    return n.degree + (n.node.kind === "document" ? 0.5 : 0);
  };
  const placed: [number, number, number, number][] = [];
  for (const n of [...nodes].sort((a, b) => rank(b) - rank(a))) {
    if (x.focusSet && !x.focusSet.has(n.index) && rank(n) < 9e8) continue;
    const always = rank(n) >= 4e8;
    const r = radiusOf(n, cam);
    const alpha = always || (scene.mini && scene.focus != null) ? 1 : labelAlpha(r);
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

export function lineDistance(d: Drawn, cam: Camera, px: number, py: number): number {
  const [x0, y0, x1, y1] = lineEnds(d, cam);
  const dx = x1 - x0;
  const dy = y1 - y0;
  const len2 = dx * dx + dy * dy || 1;
  const t = Math.max(0, Math.min(1, ((px - x0) * dx + (py - y0) * dy) / len2));
  return Math.hypot(px - (x0 + t * dx), py - (y0 + t * dy));
}
