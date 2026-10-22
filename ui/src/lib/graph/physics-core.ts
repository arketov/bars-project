// Ядро физики: d3-force над обезличенными узлами (индекс, вид, владелец, координаты). Работает в Web Worker
// (ADR-0004) и, без Worker (тесты), на основном потоке. Текст, источники и evidence остаются в модели окна.
// Параметры подобраны замером цепочек на снимке bars-project (docs/knowledge/graph-render.md, «Подбор параметров раскладки»).
import { forceCollide, forceLink, forceManyBody, forceSimulation, forceX, forceY, type Simulation } from "d3-force";

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

export interface CoreNode {
  index: number;
  kind: "document" | "chunk";
  owner: number;
  x: number;
  y: number;
  pinned: boolean;
}

export interface CoreLink {
  a: number;
  b: number;
  /** Коэффициент пружины после ограничения суммарного притяжения пары владельцев. */
  k: number;
}

interface SimNode {
  index: number;
  kind: "document" | "chunk";
  owner: number;
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

interface SimLink {
  source: SimNode;
  target: SimNode;
  k: number;
}

/** Пружина по формуле проекта умножается на этот коэффициент: без него отталкивание рвёт цепочки (замер 21-10-2026). */
const LINK_BASE = 3;
const BASE_CHARGE = -60;
const CHILD_BUDGET = 35;
const CENTER = 0.04;
const FAR = 2;
const GROUP_RATIO = 0.58;

export class PhysicsCore {
  readonly sim: Simulation<SimNode, SimLink>;
  nodes: SimNode[] = [];
  forces: Forces = { ...DEFAULT_FORCES };
  #links: SimLink[] = [];
  #byIndex = new Map<number, SimNode>();

  constructor(seed = 7) {
    let state = seed >>> 0 || 1;
    this.sim = forceSimulation<SimNode, SimLink>([])
      .randomSource(() => ((state = (Math.imul(state, 1664525) + 1013904223) >>> 0) / 4294967296))
      // Медленное охлаждение и малое трение: узлы скользят, раскладка не «замерзает» раньше времени.
      .alphaDecay(0.006)
      .velocityDecay(0.3)
      .force("link", forceLink<SimNode, SimLink>([]).strength((l) => this.#strength(l)).distance((l) => this.#length(l)))
      .force("charge", forceManyBody<SimNode>().strength((n) => n.charge).distanceMax(420))
      .force("far", forceManyBody<SimNode>().strength(() => FAR * this.forces.gravity).distanceMax(5000))
      .force("collide", forceCollide<SimNode>((n) => n.r + 1.5))
      .force("x", forceX<SimNode>(0).strength(CENTER))
      .force("y", forceY<SimNode>(0).strength(CENTER))
      .force("group", this.#groupForce)
      .stop();
  }

  #strength(link: SimLink): number {
    return Math.min(1, LINK_BASE * this.forces.link * link.k);
  }

  #length(link: SimLink): number {
    return link.source.owner === link.target.owner && link.source.kind === "chunk" ? this.forces.length * GROUP_RATIO : this.forces.length;
  }

  /** Слабое позиционное ограничение чанков в области владельца: не ребро и не пружина `contains`. */
  #groupForce = (alpha: number) => {
    const counts = new Map<number, number>();
    for (const n of this.nodes) if (n.kind === "chunk") counts.set(n.owner, (counts.get(n.owner) ?? 0) + 1);
    for (const n of this.nodes) {
      if (n.kind !== "chunk") continue;
      const owner = this.#byIndex.get(n.owner);
      if (!owner) continue;
      const radius = 20 + 4.5 * Math.sqrt(counts.get(n.owner) ?? 1);
      const dx = owner.x - n.x;
      const dy = owner.y - n.y;
      const dist = Math.hypot(dx, dy) || 1;
      const pull = (dist > radius ? 0.25 : 0.02) * alpha;
      n.vx += dx * pull;
      n.vy += dy * pull;
    }
  };

  #chargeOf(p: SimNode, children: Map<number, number>): number {
    const base = p.kind === "chunk" ? -CHILD_BUDGET / (children.get(p.owner) ?? 1) : BASE_CHARGE;
    return base * this.forces.repel;
  }

  /** Заменяет набор узлов и пружин; координаты и закрепления приходят от окна, оно же их хранит. */
  setGraph(nodes: CoreNode[], links: CoreLink[]): void {
    const next: SimNode[] = [];
    const map = new Map<number, SimNode>();
    for (const n of nodes) {
      const existing = this.#byIndex.get(n.index);
      const p: SimNode = existing ?? { index: n.index, kind: n.kind, owner: n.owner, x: n.x, y: n.y, vx: 0, vy: 0, r: n.kind === "chunk" ? 3.6 : 6, charge: BASE_CHARGE, degree: 0, pinned: false };
      p.kind = n.kind;
      p.owner = n.owner;
      p.x = n.x;
      p.y = n.y;
      p.pinned = n.pinned;
      if (n.pinned) {
        p.fx = n.x;
        p.fy = n.y;
      } else {
        p.fx = p.fy = null;
      }
      map.set(n.index, p);
      next.push(p);
    }
    const simLinks: SimLink[] = [];
    for (const l of links) {
      const source = map.get(l.a);
      const target = map.get(l.b);
      if (source && target) simLinks.push({ source, target, k: l.k });
    }
    const children = new Map<number, number>();
    for (const p of next) {
      p.degree = 0;
      if (p.kind === "chunk") children.set(p.owner, (children.get(p.owner) ?? 0) + 1);
    }
    for (const l of simLinks) {
      l.source.degree++;
      l.target.degree++;
    }
    this.nodes = next;
    this.#links = simLinks;
    this.#byIndex = map;
    for (const p of next) p.charge = this.#chargeOf(p, children);
    this.sim.nodes(next);
    // d3 перезаписывает node.index позицией в массиве; окно и рёбра опираются на индекс графа.
    next.forEach((p, i) => (p.index = nodes[i].index));
    (this.sim.force("link") as ReturnType<typeof forceLink<SimNode, SimLink>>).links(simLinks);
  }

  /** Применяет настройки пользователя к построенному набору. */
  setForces(forces: Forces): void {
    this.forces = { ...forces };
    const children = new Map<number, number>();
    for (const p of this.nodes) if (p.kind === "chunk") children.set(p.owner, (children.get(p.owner) ?? 0) + 1);
    for (const p of this.nodes) p.charge = this.#chargeOf(p, children);
    // d3 кэширует силы при initialize: повторная установка функций пересчитывает их.
    const link = this.sim.force("link") as ReturnType<typeof forceLink<SimNode, SimLink>>;
    link.strength((l) => this.#strength(l)).distance((l) => this.#length(l));
    (this.sim.force("charge") as ReturnType<typeof forceManyBody<SimNode>>).strength((n) => n.charge);
    (this.sim.force("far") as ReturnType<typeof forceManyBody<SimNode>>).strength(() => FAR * this.forces.gravity);
    (this.sim.force("x") as ReturnType<typeof forceX<SimNode>>).strength(CENTER * this.forces.gravity);
    (this.sim.force("y") as ReturnType<typeof forceY<SimNode>>).strength(CENTER * this.forces.gravity);
  }

  /** Перекладывает координаты из окна (рассев, перетаскивание): окно — источник правды о позициях. */
  place(positions: { index: number; x: number; y: number }[]): void {
    for (const p of positions) {
      const n = this.#byIndex.get(p.index);
      if (!n) continue;
      n.x = p.x;
      n.y = p.y;
      n.vx = n.vy = 0;
      if (n.pinned) {
        n.fx = p.x;
        n.fy = p.y;
      }
    }
  }

  pin(index: number, on: boolean): void {
    const n = this.#byIndex.get(index);
    if (!n) return;
    n.pinned = on;
    n.fx = on ? n.x : null;
    n.fy = on ? n.y : null;
  }

  /** Временная фиксация на время перетаскивания; постоянным закрепление она не становится. */
  drag(index: number, x: number, y: number): void {
    const n = this.#byIndex.get(index);
    if (!n) return;
    n.fx = x;
    n.fy = y;
    n.x = x;
    n.y = y;
  }

  dragEnd(index: number): void {
    const n = this.#byIndex.get(index);
    if (!n) return;
    if (n.pinned) {
      n.fx = n.x;
      n.fy = n.y;
    } else n.fx = n.fy = null;
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

  /** Детерминированная раскладка без анимации: для reduced motion, остановки и первого кадра. */
  settle(ticks: number): void {
    this.sim.alpha(Math.max(this.sim.alpha(), 0.9));
    for (let i = 0; i < ticks && this.active; i++) this.sim.tick();
  }

  cool(): void {
    this.sim.alpha(0);
    for (const n of this.nodes) n.vx = n.vy = 0;
  }

  /** Координаты в порядке `nodes`: пара [x, y] на узел. */
  snapshot(): { indices: Int32Array; coords: Float32Array } {
    const indices = new Int32Array(this.nodes.length);
    const coords = new Float32Array(this.nodes.length * 2);
    this.nodes.forEach((n, i) => {
      indices[i] = n.index;
      coords[i * 2] = n.x;
      coords[i * 2 + 1] = n.y;
    });
    return { indices, coords };
  }

  destroy(): void {
    this.sim.stop();
    this.nodes = [];
    this.#links = [];
    this.#byIndex.clear();
  }
}
