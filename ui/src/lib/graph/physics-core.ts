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

export const DEFAULT_FORCES: Forces = { link: 1, repel: 1, gravity: 1, length: 80 };

/** Подбор раскладки (замер на снимке bars-project, docs/knowledge/graph-render.md): умолчания повторяют прежнее поведение. */
export interface Tuning {
  /** Заряд узла растёт с числом связей: `charge *= 1 + hubCharge·√степень`, хабы расталкивают окружение, а не слипаются. */
  hubCharge: number;
  /** Пружина делится на корень из меньшей степени концов (как по умолчанию в d3-force). */
  springByDegree: boolean;
  /** Одиночки без связей не участвуют в расчёте и выстраиваются сеткой под связной частью. */
  shelf: boolean;
}

/** Принято замером 22-10-2026 на снимке bars-project (323 документа, 1209 пар ссылок, мини-панель 313×300 px): docs/knowledge/graph-render.md. */
export const DEFAULT_TUNING: Tuning = { hubCharge: 0.4, springByDegree: false, shelf: true };
/** Прежнее поведение: для сравнения в замерах и тестах. */
export const LEGACY_TUNING: Tuning = { hubCharge: 0, springByDegree: false, shelf: false };

/** Шаг сетки одиночек в единицах карты: узел 6 + зазор. */
export const SHELF_CELL = 16;
/** Доля ширины связной части, которую занимает сетка одиночек: вся ширина обрезается краями панели при вписывании. */
export const SHELF_WIDTH = 0.6;

/**
 * Позиции одиночек сеткой под связной частью: ширина сетки — `SHELF_WIDTH` от ширины связной части (но не меньше 4 столбцов), центр по x общий.
 * Порядок — по входному порядку, чтобы раскладка была детерминированной.
 */
export function shelfPositions(count: number, bounds: { x0: number; x1: number; y1: number } | null): { x: number; y: number }[] {
  if (!count) return [];
  const width = bounds ? Math.max(bounds.x1 - bounds.x0, SHELF_CELL * 4) : SHELF_CELL * 4;
  const columns = Math.max(1, Math.min(count, Math.floor((width * SHELF_WIDTH) / SHELF_CELL)));
  const cx = bounds ? (bounds.x0 + bounds.x1) / 2 : 0;
  const top = (bounds?.y1 ?? 0) + SHELF_CELL * 2.5;
  return Array.from({ length: count }, (_, i) => {
    const row = Math.floor(i / columns);
    const inRow = Math.min(columns, count - row * columns);
    return { x: cx + ((i % columns) - (inRow - 1) / 2) * SHELF_CELL, y: top + row * SHELF_CELL };
  });
}

export interface CoreNode {
  index: number;
  kind: "document" | "chunk" | "query";
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
  kind: "document" | "chunk" | "query";
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
/** Вдвое сильнее прежнего −60: замер показал, что при −60 связные документы налезают друг на друга при вписывании в мини-панель. */
const BASE_CHARGE = -120;
const CHILD_BUDGET = 35;
const CENTER = 0.04;
const FAR = 2;
const GROUP_RATIO = 0.58;

/** Документ без связей и без закрепления: чанки раскрытого документа держит группа владельца, их на полку не уводим. */
const onShelf = (p: SimNode) => p.kind === "document" && p.degree === 0 && !p.pinned;

export class PhysicsCore {
  readonly sim: Simulation<SimNode, SimLink>;
  nodes: SimNode[] = [];
  forces: Forces = { ...DEFAULT_FORCES };
  #links: SimLink[] = [];
  #byIndex = new Map<number, SimNode>();

  tuning: Tuning;

  constructor(seed = 7, tuning: Partial<Tuning> = {}) {
    this.tuning = { ...DEFAULT_TUNING, ...tuning };
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
    const base = Math.min(1, LINK_BASE * this.forces.link * link.k);
    return this.tuning.springByDegree ? base / Math.sqrt(Math.max(1, Math.min(link.source.degree, link.target.degree))) : base;
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
    const base = p.kind === "chunk" ? -CHILD_BUDGET / (children.get(p.owner) ?? 1) : BASE_CHARGE * (1 + this.tuning.hubCharge * Math.sqrt(p.degree));
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
    this.sim.nodes(this.tuning.shelf ? next.filter((p) => !onShelf(p)) : next);
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

  /** Одиночки сеткой под связной частью; закреплённые и перетаскиваемые остаются на своих местах. */
  #placeShelf(): void {
    const connected = this.nodes.filter((p) => !onShelf(p));
    const loose = this.nodes.filter((p) => onShelf(p) && p.fx == null);
    if (!loose.length) return;
    const bounds = connected.length ? { x0: Math.min(...connected.map((p) => p.x)), x1: Math.max(...connected.map((p) => p.x)), y1: Math.max(...connected.map((p) => p.y)) } : null;
    const spots = shelfPositions(loose.length, bounds);
    loose.forEach((p, i) => {
      p.x = spots[i].x;
      p.y = spots[i].y;
      p.vx = p.vy = 0;
    });
  }

  /** Координаты в порядке `nodes`: пара [x, y] на узел. */
  snapshot(): { indices: Int32Array; coords: Float32Array } {
    if (this.tuning.shelf) this.#placeShelf();
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
