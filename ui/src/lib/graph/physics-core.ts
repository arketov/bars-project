// Ядро физики: d3-force над обезличенными узлами (индекс, вид, владелец, координаты). Работает в Web Worker
// (ADR-0004) и, без Worker (тесты), на основном потоке. Текст, источники и evidence остаются в модели окна.
// Параметры подобраны замером на выгрузках bars-project и overdrive-bots (docs/knowledge/graph-render.md, «Плавная и послушная физика»).
import { forceCollide, forceLink, forceManyBody, forceSimulation, forceX, forceY, type Simulation } from "d3-force";

export interface Forces {
  /** Притяжение по связям. */
  link: number;
  /** Отталкивание узлов. */
  repel: number;
  /** Общее притяжение: все узлы тянутся к общему центру, как «Center force» Obsidian. */
  gravity: number;
  /** Длина связи между документами. */
  length: number;
}

export const DEFAULT_FORCES: Forces = { link: 1, repel: 1, gravity: 1, length: 150 };

/** Подбор раскладки (docs/knowledge/graph-render.md): все численные параметры физики при ползунках ×1. */
export interface Tuning {
  /** Пружина `min(1, linkBase·k_e)`. */
  linkBase: number;
  /** Пружина делится на `степень^springDegree` меньшего конца: 0 — без поправки, 1 — как по умолчанию в d3-force. */
  springDegree: number;
  /** Заряд документа при длине связи `chargeScaleLength`; растёт как квадрат длины, чтобы «Длина связи» меняла масштаб карты целиком, а не только тянула пружины против отталкивания. */
  charge: number;
  /** Длина связи, при которой заряд равен `charge`; 0 — заряд не зависит от длины (раунд 27). */
  chargeScaleLength: number;
  /** Заряд растёт с числом связей: `charge *= 1 + hubCharge·√степень`, хабы расталкивают окружение, а не слипаются. */
  hubCharge: number;
  /** Дальность отталкивания в длинах связи; `Infinity` — на любом расстоянии. */
  chargeRange: number;
  /** Притяжение к общему центру. */
  center: number;
  /** Трение d3: доля скорости, теряемая за шаг. */
  velocityDecay: number;
  /** Охлаждение d3 за шаг. */
  alphaDecay: number;
  /** Жёсткость столкновения (0…1). */
  collide: number;
}

export const DEFAULT_TUNING: Tuning = { linkBase: 0.9, springDegree: 0, charge: -100, chargeScaleLength: 100, hubCharge: 0.2, chargeRange: 3, center: 0.04, velocityDecay: 0.45, alphaDecay: 0.02, collide: 0.7 };
/** Физика раунда 27 (до 23-10-2026): для сравнения в замерах и тестах. */
export const LEGACY_TUNING: Tuning = { linkBase: 0.9, springDegree: 0, charge: -360, chargeScaleLength: 0, hubCharge: 0.4, chargeRange: Infinity, center: 0.16, velocityDecay: 0.3, alphaDecay: 0.006, collide: 1 };

/** Радиус узла в единицах карты: хаб крупнее, как в Obsidian. `degree` — число видимых связей узла. */
export function nodeRadius(kind: "document" | "chunk" | "query", degree: number): number {
  if (kind === "chunk") return 3.6;
  if (kind === "query") return 8;
  return Math.min(18, 4 + 2 * Math.sqrt(degree));
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
  /** Номер узла в графе окна. `index` принадлежит d3: по нему d3 хранит заряд и радиус столкновения, перезаписывать его нельзя. */
  id: number;
  index?: number;
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

const CHILD_BUDGET = 35;
/** Кольцо одиночек: радиус — 95-й процентиль удаления связных узлов от их центра плюс длина связи; сила к кольцу. */
const RING_PULL = 0.3;
const GROUP_RATIO = 0.58;

/** Документ без связей и без закрепления: встаёт на кольцо одиночек. */
const isLone = (n: SimNode) => n.kind === "document" && n.degree === 0 && n.fx == null;

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
      .alphaDecay(this.tuning.alphaDecay)
      .velocityDecay(this.tuning.velocityDecay)
      .force("link", forceLink<SimNode, SimLink>([]).strength((l) => this.#strength(l)).distance((l) => this.#length(l)))
      .force("charge", forceManyBody<SimNode>().strength((n) => n.charge).distanceMax(this.tuning.chargeRange * this.forces.length))
      .force("collide", forceCollide<SimNode>((n) => n.r + 1.5).strength(this.tuning.collide))
      .force("x", forceX<SimNode>(0).strength((n) => this.#center(n)))
      .force("y", forceY<SimNode>(0).strength((n) => this.#center(n)))
      .force("group", this.#groupForce)
      .force("ring", this.#ringForce)
      .stop();
  }

  #strength(link: SimLink): number {
    const base = Math.min(1, this.tuning.linkBase * this.forces.link * link.k);
    return base / Math.max(1, Math.min(link.source.degree, link.target.degree)) ** this.tuning.springDegree;
  }

  #center(n: SimNode): number {
    return this.tuning.center * this.forces.gravity;
  }

  /** Одиночки (документы без связей) тянутся к кольцу вокруг связной части, как в Obsidian; вдоль кольца их разводит отталкивание. */
  #ringForce = (alpha: number) => {
    const lone: SimNode[] = [];
    const linked: SimNode[] = [];
    for (const n of this.nodes) (isLone(n) ? lone : linked).push(n);
    if (!lone.length || !linked.length) return;
    let cx = 0;
    let cy = 0;
    for (const n of linked) {
      cx += n.x;
      cy += n.y;
    }
    cx /= linked.length;
    cy /= linked.length;
    const dist = linked.map((n) => Math.hypot(n.x - cx, n.y - cy)).sort((a, b) => a - b);
    const radius = dist[Math.floor((dist.length - 1) * 0.95)] + this.forces.length;
    for (const n of lone) {
      const dx = n.x - cx;
      const dy = n.y - cy;
      const d = Math.hypot(dx, dy) || 1;
      const pull = ((radius - d) / d) * RING_PULL * alpha;
      n.vx += dx * pull;
      n.vy += dy * pull;
    }
  };

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
    const base = p.kind === "chunk" ? -CHILD_BUDGET / (children.get(p.owner) ?? 1) : this.tuning.charge * (1 + this.tuning.hubCharge * Math.sqrt(p.degree));
    // Чанки делят бюджет владельца и держатся группой у него: их заряд от длины связи не зависит.
    const scale = this.tuning.chargeScaleLength && p.kind !== "chunk" ? (this.forces.length / this.tuning.chargeScaleLength) ** 2 : 1;
    return base * scale * this.forces.repel;
  }

  /** Заменяет набор узлов и пружин; координаты и закрепления приходят от окна, оно же их хранит. */
  setGraph(nodes: CoreNode[], links: CoreLink[]): void {
    const next: SimNode[] = [];
    const map = new Map<number, SimNode>();
    for (const n of nodes) {
      const existing = this.#byIndex.get(n.index);
      const p: SimNode = existing ?? { id: n.index, kind: n.kind, owner: n.owner, x: n.x, y: n.y, vx: 0, vy: 0, r: n.kind === "chunk" ? 3.6 : 6, charge: this.tuning.charge, degree: 0, pinned: false };
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
    for (const p of next) p.r = nodeRadius(p.kind, p.degree);
    // d3 кэширует радиусы при initialize: функция пересчитывается для нового набора.
    (this.sim.force("collide") as ReturnType<typeof forceCollide<SimNode>>).radius((n) => n.r + 1.5);
    this.nodes = next;
    this.#links = simLinks;
    this.#byIndex = map;
    for (const p of next) p.charge = this.#chargeOf(p, children);
    this.sim.nodes(next);
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
    (this.sim.force("charge") as ReturnType<typeof forceManyBody<SimNode>>).strength((n) => n.charge).distanceMax(this.tuning.chargeRange * this.forces.length);
    (this.sim.force("x") as ReturnType<typeof forceX<SimNode>>).strength((n) => this.#center(n));
    (this.sim.force("y") as ReturnType<typeof forceY<SimNode>>).strength((n) => this.#center(n));
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
      indices[i] = n.id;
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
