// Раскладка в окне: модель узлов для отрисовки и hit-testing, расчёт — в Web Worker (physics-host.ts).
// Окно хранит координаты и закрепления и принимает порции только своего поколения и запуска раскладки.
import { hash, springCoefficients, type GEdge, type GNode } from "./model";
import { DEFAULT_FORCES, nodeRadius, type CoreLink, type CoreNode, type Forces } from "./physics-core";
import { PhysicsHost, type Positions, type ToPhysics } from "./physics-host";

export { DEFAULT_FORCES };
export type { Forces };

export interface PNode {
  index: number;
  node: GNode;
  x: number;
  y: number;
  r: number;
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

interface Transport {
  post(message: ToPhysics): void;
  close(): void;
}

/** Web Worker; без него (тесты, jsdom) — тот же хозяин физики на основном потоке с асинхронной доставкой. */
function openTransport(onMessage: (message: Positions) => void): Transport {
  if (typeof Worker !== "undefined") {
    try {
      const worker = new Worker(new URL("./physics.worker.ts", import.meta.url), { type: "module" });
      worker.onmessage = (event: MessageEvent<Positions>) => onMessage(event.data);
      return { post: (message) => worker.postMessage(message), close: () => worker.terminate() };
    } catch {
      // Worker не создан (ограничение среды): расчёт идёт на основном потоке.
    }
  }
  let closed = false;
  const host = new PhysicsHost((message) => {
    setTimeout(() => {
      if (!closed) onMessage(message);
    }, 0);
  });
  return {
    post: (message) => {
      if (!closed) host.receive(message);
    },
    close: () => {
      closed = true;
      host.destroy();
    },
  };
}

export class Layout {
  nodes: PNode[] = [];
  links: PLink[] = [];
  forces: Forces = { ...DEFAULT_FORCES };
  /** Вызывается после применения порции координат: окно перерисовывает карту. */
  onPositions: (settled: boolean) => void = () => {};
  #byIndex = new Map<number, PNode>();
  #pins = new Set<number>();
  #transport: Transport;
  #gen = 0;
  #run = 0;
  #active = false;
  #destroyed = false;

  constructor() {
    this.#transport = openTransport((message) => this.#receive(message));
  }

  #receive(message: Positions): void {
    if (this.#destroyed) return;
    // Поздняя порция прежнего набора или остановленного запуска не сдвигает карту.
    if (message.gen !== this.#gen || message.run !== this.#run) return;
    for (let i = 0; i < message.indices.length; i++) {
      const p = this.#byIndex.get(message.indices[i]);
      if (!p || p.pinned) continue;
      p.x = message.coords[i * 2];
      p.y = message.coords[i * 2 + 1];
    }
    this.#active = message.active;
    this.#transport.post({ t: "ack", seq: message.seq });
    this.onPositions(message.settled);
  }

  /** Идёт ли движение (по последней порции). */
  get active(): boolean {
    return this.#active;
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
        r: node.kind === "chunk" ? 3.6 : node.kind === "query" ? 8 : 6,
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
    for (const p of next) {
      p.degree = 0;
      p.pinned = this.#pins.has(p.index);
    }
    for (const l of links) {
      l.source.degree++;
      l.target.degree++;
    }
    for (const p of next) p.r = nodeRadius(p.node.kind, p.degree);
    this.nodes = next;
    this.links = links;
    this.#byIndex = nextMap;
    this.#gen++;
    this.#active = false;
    const core: CoreNode[] = next.map((p) => ({ index: p.index, kind: p.node.kind, owner: p.node.owner, x: p.x, y: p.y, pinned: p.pinned }));
    const coreLinks: CoreLink[] = links.map((l) => ({ a: l.source.index, b: l.target.index, k: l.k }));
    this.#transport.post({ t: "graph", gen: this.#gen, nodes: core, links: coreLinks, forces: this.forces });
  }

  /** Применяет настройки пользователя к построенному набору. */
  setForces(forces: Forces): void {
    this.forces = { ...forces };
    this.#transport.post({ t: "forces", gen: this.#gen, forces: this.forces });
  }

  /**
   * Раскладка с нуля: позиции незакреплённых узлов берутся из детерминированного рассева, а не из прошлого режима.
   * Нужна при смене режима связей, чтобы расположение определяли только связи текущего режима.
   */
  reseed(): void {
    // Вид запроса: запрос в центре, результаты по кругу в порядке выдачи, окружение — обычным рассевом.
    const results = this.nodes.filter((p) => p.node.qrole === "result").sort((a, b) => (a.node.rank ?? 0) - (b.node.rank ?? 0));
    results.forEach((p, i) => {
      if (p.pinned) return;
      const angle = (i / Math.max(1, results.length)) * Math.PI * 2 - Math.PI / 2;
      p.x = Math.cos(angle) * 110;
      p.y = Math.sin(angle) * 110;
    });
    for (const p of this.nodes) {
      if (p.node.kind === "query" && !p.pinned) {
        p.x = 0;
        p.y = 0;
        continue;
      }
      if (p.pinned || p.node.kind === "chunk" || p.node.qrole === "result") continue;
      const angle = hash(p.node.index, 11) * Math.PI * 2;
      const radius = Math.sqrt(hash(p.node.index, 12)) * 260;
      p.x = Math.cos(angle) * radius;
      p.y = Math.sin(angle) * radius;
    }
    for (const p of this.nodes) {
      if (p.pinned || p.node.kind !== "chunk") continue;
      const owner = this.#byIndex.get(p.node.owner);
      const angle = hash(p.node.index, 13) * Math.PI * 2;
      p.x = (owner?.x ?? 0) + Math.cos(angle) * 20;
      p.y = (owner?.y ?? 0) + Math.sin(angle) * 20;
    }
    this.#transport.post({ t: "place", gen: this.#gen, positions: this.nodes.map((p) => ({ index: p.index, x: p.x, y: p.y })) });
  }

  get(index: number): PNode | undefined {
    return this.#byIndex.get(index);
  }

  reheat(alpha: number): void {
    this.#active = true;
    this.#transport.post({ t: "heat", gen: this.#gen, run: this.#run, alpha });
  }

  /** Детерминированная раскладка без анимации (reduced motion, остановка, первый кадр); `freeze` — остановить после неё. */
  settle(ticks: number, freeze = false): void {
    if (freeze) this.#run++;
    this.#transport.post({ t: "settle", gen: this.#gen, run: this.#run, ticks, freeze });
  }

  /** Остановка: новый запуск раскладки, поздние порции прежнего отбрасываются до обращения к worker. */
  cool(): void {
    this.#run++;
    this.#active = false;
    this.#transport.post({ t: "cool", gen: this.#gen, run: this.#run });
  }

  /** Скрытая панель не считает; возвращение продолжает прежнюю раскладку. */
  hold(on: boolean): void {
    this.#transport.post({ t: "hold", on });
  }

  togglePin(index: number): boolean {
    const p = this.#byIndex.get(index);
    if (!p) return false;
    const on = !this.#pins.has(index);
    if (on) this.#pins.add(index);
    else this.#pins.delete(index);
    p.pinned = on;
    this.#transport.post({ t: "place", gen: this.#gen, positions: [{ index, x: p.x, y: p.y }] });
    this.#transport.post({ t: "pin", gen: this.#gen, index, on });
    return on;
  }

  /** Временная фиксация на время перетаскивания; постоянным закреплением она не становится. */
  dragTo(index: number, x: number, y: number): void {
    const p = this.#byIndex.get(index);
    if (!p) return;
    p.x = x;
    p.y = y;
    this.#transport.post({ t: "drag", gen: this.#gen, index, x, y });
  }

  dragEnd(index: number): void {
    this.#transport.post({ t: "dragEnd", gen: this.#gen, index });
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
    this.#destroyed = true;
    this.#transport.close();
    this.nodes = [];
    this.links = [];
    this.#byIndex.clear();
  }
}
