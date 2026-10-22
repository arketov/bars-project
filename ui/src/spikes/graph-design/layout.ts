// Физика стенда: d3-force по формулам physical-graph.md. Основной поток, ручные ticks; в продукте — worker (ADR-0004).
// Параметры подобраны замером цепочек на снимке bars-project (docs/chats/2026-10-21-graph-design/tools/physics-bench.mjs).
import { forceCollide, forceLink, forceManyBody, forceSimulation, forceX, forceY, type Simulation } from "d3-force";
import { hash, springCoefficients, type GEdge, type GNode } from "./model";

export interface PNode {
  index: number;
  node: GNode;
  x: number;
  y: number;
  vx: number;
  vy: number;
  fx?: number | null;
  fy?: number | null;
  r: number;
  charge: number;
  degree: number;
  pinned: boolean;
}

export interface PLink {
  source: PNode;
  target: PNode;
  edge: GEdge;
  /** Коэффициент пружины после ограничения суммарного притяжения пары владельцев. */
  k: number;
}

/** Настройки пользователя: множители вокруг подобранных значений, длина — в единицах карты. */
export interface Forces {
  /** Притяжение по связям. */
  link: number;
  /** Отталкивание узлов. */
  repel: number;
  /** Слабое притяжение всех узлов друг к другу и к центру. */
  gravity: number;
  /** Длина связи между документами. */
  length: number;
}

export const DEFAULT_FORCES: Forces = { link: 1, repel: 1, gravity: 1, length: 60 };

/** Пружина по формуле проекта умножается на этот коэффициент: без него отталкивание рвёт цепочки (замер 21-10-2026). */
const LINK_BASE = 3;
const BASE_CHARGE = -60;
const ISOLATE_FACTOR = 1;
const CHILD_BUDGET = 35;
const CENTER = 0.04;
const FAR = 2;
const GROUP_RATIO = 0.58;

export interface LayoutOptions {
  seed: number;
}

export class Layout {
  readonly sim: Simulation<PNode, PLink>;
  nodes: PNode[] = [];
  links: PLink[] = [];
  forces: Forces = { ...DEFAULT_FORCES };
  #byIndex = new Map<number, PNode>();
  #pins = new Set<number>();

  constructor(options: LayoutOptions) {
    let state = options.seed >>> 0 || 1;
    this.sim = forceSimulation<PNode, PLink>([])
      .randomSource(() => ((state = (Math.imul(state, 1664525) + 1013904223) >>> 0) / 4294967296))
      // Медленное охлаждение и малое трение: узлы скользят, раскладка не «замерзает» раньше времени.
      .alphaDecay(0.006)
      .velocityDecay(0.3)
      .force("link", forceLink<PNode, PLink>([]).strength((l) => this.#strength(l)).distance((l) => this.#length(l)))
      .force("charge", forceManyBody<PNode>().strength((n) => n.charge).distanceMax(420))
      .force("far", forceManyBody<PNode>().strength(() => FAR * this.forces.gravity).distanceMax(5000))
      .force("collide", forceCollide<PNode>((n) => n.r + 1.5))
      .force("x", forceX<PNode>(0).strength(CENTER))
      .force("y", forceY<PNode>(0).strength(CENTER))
      .force("group", this.#groupForce)
      .stop();
  }

  #strength(link: PLink): number {
    return Math.min(1, LINK_BASE * this.forces.link * link.k);
  }

  #length(link: PLink): number {
    return link.source.node.owner === link.target.node.owner && link.source.node.kind === "chunk" ? this.forces.length * GROUP_RATIO : this.forces.length;
  }

  /** Слабое позиционное ограничение чанков в области владельца: не ребро и не пружина `contains`. */
  #groupForce = (alpha: number) => {
    const counts = new Map<number, number>();
    for (const n of this.nodes) if (n.node.kind === "chunk") counts.set(n.node.owner, (counts.get(n.node.owner) ?? 0) + 1);
    for (const n of this.nodes) {
      if (n.node.kind !== "chunk") continue;
      const owner = this.#byIndex.get(n.node.owner);
      if (!owner) continue;
      const radius = 20 + 4.5 * Math.sqrt(counts.get(n.node.owner) ?? 1);
      const dx = owner.x - n.x;
      const dy = owner.y - n.y;
      const dist = Math.hypot(dx, dy) || 1;
      const pull = (dist > radius ? 0.25 : 0.02) * alpha;
      n.vx += dx * pull;
      n.vy += dy * pull;
    }
  };

  #chargeOf(p: PNode, children: Map<number, number>): number {
    const base = p.node.kind === "chunk" ? -CHILD_BUDGET / (children.get(p.node.owner) ?? 1) : p.degree === 0 ? BASE_CHARGE * ISOLATE_FACTOR : BASE_CHARGE;
    return base * this.forces.repel;
  }

  /** Заменяет видимый набор, сохраняя координаты неизменившихся узлов; новые появляются рядом с владельцем или соседом. */
  setGraph(nodes: GNode[], edges: GEdge[], ownerOf: (index: number) => number): void {
    const next: PNode[] = [];
    const nextMap = new Map<number, PNode>();
    const neighbour = new Map<number, number>();
    for (const e of edges) {
      neighbour.set(e.a, neighbour.get(e.a) ?? e.b);
      neighbour.set(e.b, neighbour.get(e.b) ?? e.a);
    }
    for (const node of nodes) {
      const existing = this.#byIndex.get(node.index);
      if (existing) {
        existing.node = node;
        nextMap.set(node.index, existing);
        next.push(existing);
      }
    }
    for (const node of nodes) {
      if (this.#byIndex.has(node.index)) continue;
      const anchor = nextMap.get(node.owner) ?? nextMap.get(neighbour.get(node.index) ?? -1);
      const angle = hash(node.index, 1) * Math.PI * 2;
      const spread = node.kind === "chunk" ? 14 + hash(node.index, 2) * 14 : 40 + hash(node.index, 2) * 80;
      const p: PNode = {
        index: node.index,
        node,
        x: anchor ? anchor.x + Math.cos(angle) * spread : Math.cos(angle) * spread * 2,
        y: anchor ? anchor.y + Math.sin(angle) * spread : Math.sin(angle) * spread * 2,
        vx: 0,
        vy: 0,
        r: node.kind === "chunk" ? 3.6 : node.kind === "query" ? 9 : 6,
        charge: BASE_CHARGE,
        degree: 0,
        pinned: false,
      };
      nextMap.set(node.index, p);
      next.push(p);
    }
    const k = springCoefficients(edges, ownerOf);
    const links: PLink[] = [];
    for (const edge of edges) {
      const source = nextMap.get(edge.a);
      const target = nextMap.get(edge.b);
      if (!source || !target) continue;
      links.push({ source, target, edge, k: k.get(edge.id) ?? 0 });
    }
    const children = new Map<number, number>();
    for (const p of next) {
      p.degree = 0;
      p.pinned = this.#pins.has(p.index);
      if (p.node.kind === "chunk") children.set(p.node.owner, (children.get(p.node.owner) ?? 0) + 1);
    }
    for (const l of links) {
      l.source.degree++;
      l.target.degree++;
    }
    this.nodes = next;
    this.links = links;
    this.#byIndex = nextMap;
    for (const p of next) p.charge = this.#chargeOf(p, children);
    this.sim.nodes(next);
    // d3 перезаписывает node.index позицией в массиве; выбор, pin и рёбра опираются на индекс графа.
    for (const p of next) p.index = p.node.index;
    (this.sim.force("link") as ReturnType<typeof forceLink<PNode, PLink>>).links(links);
  }

  /** Применяет настройки пользователя к построенному набору. */
  setForces(forces: Forces): void {
    this.forces = { ...forces };
    const children = new Map<number, number>();
    for (const p of this.nodes) if (p.node.kind === "chunk") children.set(p.node.owner, (children.get(p.node.owner) ?? 0) + 1);
    for (const p of this.nodes) p.charge = this.#chargeOf(p, children);
    // d3 кэширует силы при initialize: повторная установка функций пересчитывает их.
    const link = this.sim.force("link") as ReturnType<typeof forceLink<PNode, PLink>>;
    link.strength((l) => this.#strength(l)).distance((l) => this.#length(l));
    (this.sim.force("charge") as ReturnType<typeof forceManyBody<PNode>>).strength((n) => n.charge);
    (this.sim.force("far") as ReturnType<typeof forceManyBody<PNode>>).strength(() => FAR * this.forces.gravity);
    (this.sim.force("x") as ReturnType<typeof forceX<PNode>>).strength(CENTER * this.forces.gravity);
    (this.sim.force("y") as ReturnType<typeof forceY<PNode>>).strength(CENTER * this.forces.gravity);
  }

  /**
   * Раскладка с нуля: позиции незакреплённых узлов берутся из детерминированного рассева, а не из прошлого режима.
   * Нужна при смене режима связей, чтобы расположение определяли только связи текущего режима.
   */
  reseed(): void {
    for (const p of this.nodes) {
      if (p.pinned || p.node.kind === "chunk") continue;
      const angle = hash(p.node.index, 11) * Math.PI * 2;
      const radius = Math.sqrt(hash(p.node.index, 12)) * 260;
      p.x = Math.cos(angle) * radius;
      p.y = Math.sin(angle) * radius;
      p.vx = p.vy = 0;
    }
    for (const p of this.nodes) {
      if (p.pinned || p.node.kind !== "chunk") continue;
      const owner = this.#byIndex.get(p.node.owner);
      const angle = hash(p.node.index, 13) * Math.PI * 2;
      p.x = (owner?.x ?? 0) + Math.cos(angle) * 20;
      p.y = (owner?.y ?? 0) + Math.sin(angle) * 20;
      p.vx = p.vy = 0;
    }
  }

  get(index: number): PNode | undefined {
    return this.#byIndex.get(index);
  }

  /** Координаты всех узлов: общий вид запоминает их при входе в вид запроса и возвращает при выходе. */
  snapshot(): Map<number, [number, number]> {
    return new Map(this.nodes.map((n) => [n.index, [n.x, n.y] as [number, number]]));
  }

  /** Ставит узлы на заданные места без движения; закреплённые узлы остаются закреплёнными на новом месте. */
  place(positions: Map<number, [number, number]>): void {
    for (const p of this.nodes) {
      const at = positions.get(p.index);
      if (!at) continue;
      p.x = at[0];
      p.y = at[1];
      p.vx = p.vy = 0;
      if (p.pinned) {
        p.fx = p.x;
        p.fy = p.y;
      }
    }
  }

  reheat(alpha: number): void {
    if (this.sim.alpha() < alpha) this.sim.alpha(alpha);
  }

  get active(): boolean {
    return this.sim.alpha() > this.sim.alphaMin();
  }

  /** Несколько ticks; возвращает, продолжается ли движение. */
  step(ticks: number): boolean {
    for (let i = 0; i < ticks && this.active; i++) this.sim.tick();
    return this.active;
  }

  /** Детерминированная статическая раскладка без анимации: для reduced motion и первого кадра. */
  settle(ticks: number): void {
    this.sim.alpha(Math.max(this.sim.alpha(), 0.9));
    for (let i = 0; i < ticks && this.active; i++) this.sim.tick();
  }

  cool(): void {
    this.sim.alpha(0);
    for (const n of this.nodes) {
      n.vx = 0;
      n.vy = 0;
    }
  }

  togglePin(index: number): boolean {
    const p = this.#byIndex.get(index);
    if (!p) return false;
    if (this.#pins.has(index)) {
      this.#pins.delete(index);
      p.pinned = false;
      p.fx = p.fy = null;
    } else {
      this.#pins.add(index);
      p.pinned = true;
      p.fx = p.x;
      p.fy = p.y;
    }
    return p.pinned;
  }

  /** Временная фиксация на время перетаскивания; постоянным pin она не становится. */
  dragTo(index: number, x: number, y: number): void {
    const p = this.#byIndex.get(index);
    if (!p) return;
    p.fx = x;
    p.fy = y;
    p.x = x;
    p.y = y;
  }

  dragEnd(index: number): void {
    const p = this.#byIndex.get(index);
    if (p && !p.pinned) p.fx = p.fy = null;
    if (p?.pinned) {
      p.fx = p.x;
      p.fy = p.y;
    }
  }

  /** Границы по 2–98 процентилям всех узлов: одиночные выбросы не сжимают карту, малые наборы входят целиком. */
  bounds(): { x0: number; y0: number; x1: number; y1: number } | null {
    const pool = this.nodes;
    if (!pool.length) return null;
    const pick = (values: number[], q: number) => values.sort((a, b) => a - b)[Math.min(values.length - 1, Math.max(0, Math.round(q * (values.length - 1))))];
    const xs = pool.map((n) => n.x);
    const ys = pool.map((n) => n.y);
    return { x0: pick([...xs], 0.02), x1: pick([...xs], 0.98), y0: pick([...ys], 0.02), y1: pick([...ys], 0.98) };
  }

  destroy(): void {
    this.sim.stop();
    this.nodes = [];
    this.links = [];
    this.#byIndex.clear();
  }
}
