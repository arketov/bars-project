import { afterEach, describe, expect, it } from "vitest";
import { Layout } from "./layout";
import type { GEdge, GNode } from "./model";
import { DEFAULT_FORCES, LEGACY_TUNING, PhysicsCore, nodeRadius, type CoreLink, type CoreNode, type Tuning } from "./physics-core";
import { PhysicsHost, type Positions } from "./physics-host";

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function chain(count: number, isolates = 0): { nodes: CoreNode[]; links: CoreLink[] } {
  const nodes: CoreNode[] = Array.from({ length: count + isolates }, (_, index) => ({ index, kind: "document", owner: index, x: Math.cos(index) * 80, y: Math.sin(index * 1.7) * 80, pinned: false }));
  const links: CoreLink[] = Array.from({ length: count - 1 }, (_, i) => ({ a: i, b: i + 1, k: 0.34 }));
  return { nodes, links };
}

describe("ядро физики", () => {
  it("ссылки держат цепочку: подавляющее большинство связей короче 1,6 длины", () => {
    const { nodes, links } = chain(24, 8);
    const core = new PhysicsCore(7);
    core.setForces({ ...DEFAULT_FORCES });
    core.setGraph(nodes, links);
    core.settle(500);
    const at = new Map(core.nodes.map((n) => [n.id, n]));
    const short = links.filter((l) => Math.hypot(at.get(l.a)!.x - at.get(l.b)!.x, at.get(l.a)!.y - at.get(l.b)!.y) < 1.6 * DEFAULT_FORCES.length);
    expect(short.length / links.length).toBeGreaterThanOrEqual(0.8);
  });

  it("раскладка детерминирована: одинаковый вход — одинаковые координаты", () => {
    const run = () => {
      const { nodes, links } = chain(10);
      const core = new PhysicsCore(7);
      core.setGraph(nodes, links);
      core.settle(200);
      return Array.from(core.snapshot().coords);
    };
    expect(run()).toEqual(run());
  });

  it("закреплённый узел остаётся на месте, остальные его не сдвигают", () => {
    const { nodes, links } = chain(6);
    nodes[2] = { ...nodes[2], x: 10, y: 20, pinned: true };
    const core = new PhysicsCore(7);
    core.setGraph(nodes, links);
    core.settle(200);
    const pinned = core.nodes.find((n) => n.id === 2)!;
    expect([pinned.x, pinned.y]).toEqual([10, 20]);
  });

  it("номера узлов не совпадают с позицией (вид запроса, локальный граф): каждый узел отталкивается своим зарядом", () => {
    // Запрос с большим номером и результаты вразброс, как подмножество большого среза; все стартуют почти в одной точке.
    const query = 900;
    const results = [3, 41, 77, 150, 260, 333, 512, 640];
    const nodes: CoreNode[] = [
      { index: query, kind: "query", owner: query, x: 0, y: 0, pinned: false },
      ...results.map((index, i): CoreNode => ({ index, kind: "document", owner: index, x: Math.cos(i) * 20, y: Math.sin(i) * 20, pinned: false })),
    ];
    const core = new PhysicsCore(7);
    core.setForces({ ...DEFAULT_FORCES });
    core.setGraph(nodes, results.map((b) => ({ a: query, b, k: 0.4 })));
    core.settle(600);
    const docs = core.nodes.filter((n) => n.kind === "document");
    const nearest = docs.map((a) => Math.min(...docs.filter((b) => b !== a).map((b) => Math.hypot(a.x - b.x, a.y - b.y))));
    expect(Math.min(...nearest)).toBeGreaterThan(DEFAULT_FORCES.length * 0.3);
    expect(Array.from(core.snapshot().indices).sort((a, b) => a - b)).toEqual([...results, query].sort((a, b) => a - b));
  });
});

describe("хозяин физики: порции, поколения и остановка", () => {
  const collect = () => {
    const sent: Positions[] = [];
    return { sent, host: new PhysicsHost((message) => sent.push(message)) };
  };
  const start = (host: PhysicsHost, gen = 1) => {
    const { nodes, links } = chain(12);
    host.receive({ t: "graph", gen, nodes, links, forces: { ...DEFAULT_FORCES } });
  };

  it("одна неподтверждённая порция: следующая приходит после ACK", async () => {
    const { sent, host } = collect();
    start(host);
    host.receive({ t: "heat", gen: 1, run: 0, alpha: 0.9 });
    await wait(30);
    expect(sent).toHaveLength(1);
    host.receive({ t: "ack", seq: sent[0].seq });
    await wait(30);
    expect(sent.length).toBeGreaterThanOrEqual(2);
    expect(sent[1].seq).toBe(sent[0].seq + 1);
    host.destroy();
  });

  it("остановка: после cool новых порций нет даже при ACK", async () => {
    const { sent, host } = collect();
    start(host);
    host.receive({ t: "heat", gen: 1, run: 0, alpha: 0.9 });
    await wait(20);
    host.receive({ t: "cool", gen: 1, run: 1 });
    host.receive({ t: "ack", seq: sent.at(-1)!.seq });
    const count = sent.length;
    await wait(40);
    expect(sent).toHaveLength(count);
    host.destroy();
  });

  it("поздняя команда прежнего поколения не запускает раскладку", async () => {
    const { sent, host } = collect();
    start(host, 2);
    host.receive({ t: "heat", gen: 1, run: 0, alpha: 0.9 });
    await wait(30);
    expect(sent).toHaveLength(0);
    host.destroy();
  });

  it("settle с остановкой даёт одну порцию с признаком settled и тишину после неё", async () => {
    const { sent, host } = collect();
    start(host);
    host.receive({ t: "settle", gen: 1, run: 1, ticks: 100, freeze: true });
    expect(sent).toHaveLength(1);
    expect(sent[0]).toMatchObject({ settled: true, active: false, run: 1, gen: 1 });
    host.receive({ t: "ack", seq: sent[0].seq });
    await wait(30);
    expect(sent).toHaveLength(1);
    host.destroy();
  });

  it("скрытая панель не считает, возвращение продолжает раскладку", async () => {
    const { sent, host } = collect();
    start(host);
    host.receive({ t: "hold", on: true });
    host.receive({ t: "heat", gen: 1, run: 0, alpha: 0.9 });
    await wait(30);
    expect(sent).toHaveLength(0);
    host.receive({ t: "hold", on: false });
    await wait(30);
    expect(sent.length).toBeGreaterThan(0);
    host.destroy();
  });
});

describe("раскладка в окне (расчёт вне основного потока)", () => {
  let layout: Layout | null = null;
  afterEach(() => {
    layout?.destroy();
    layout = null;
  });

  const gnode = (index: number): GNode => ({ index, id: `n${index}`, kind: "document", label: `n${index}`, path: "", docsPath: "", module: "m", owner: index, chunks: 0, ordinal: null, firstLine: 1, lastLine: 1 });
  const gedge = (id: number, a: number, b: number): GEdge => ({ id, key: `${a}-${b}`, kind: "markdown_link", a, b, raw: null, s: 1, lines: [1] });

  async function until(condition: () => boolean, ms = 1500) {
    const end = Date.now() + ms;
    while (!condition() && Date.now() < end) await wait(10);
    expect(condition()).toBe(true);
  }

  it("settle с остановкой сообщает о завершённой раскладке и оставляет конечные координаты", async () => {
    layout = new Layout();
    let settled = 0;
    layout.onPositions = (s) => {
      if (s) settled++;
    };
    const nodes = Array.from({ length: 8 }, (_, i) => gnode(i));
    layout.setGraph(nodes, nodes.slice(1).map((_, i) => gedge(i, i, i + 1)), (i) => i);
    layout.reseed();
    layout.settle(300, true);
    await until(() => settled > 0);
    expect(layout.active).toBe(false);
    expect(layout.nodes.every((n) => Number.isFinite(n.x) && Number.isFinite(n.y))).toBe(true);
    const before = layout.nodes.map((n) => [n.x, n.y]);
    await wait(60);
    expect(layout.nodes.map((n) => [n.x, n.y])).toEqual(before);
  });

  it("окно с отрисовкой: следующий шаг физики только после показанного кадра", async () => {
    layout = new Layout();
    layout.paced = true;
    let batches = 0;
    layout.onPositions = () => batches++;
    const nodes = Array.from({ length: 10 }, (_, i) => gnode(i));
    layout.setGraph(nodes, nodes.slice(1).map((_, i) => gedge(i, i, i + 1)), (i) => i);
    layout.reheat(0.9);
    await until(() => batches === 1);
    await wait(60);
    expect(batches).toBe(1);
    layout.frameDone();
    await until(() => batches === 2);
    await wait(60);
    expect(batches).toBe(2);
  });

  it("после остановки поздние порции не двигают карту", async () => {
    layout = new Layout();
    const nodes = Array.from({ length: 10 }, (_, i) => gnode(i));
    layout.setGraph(nodes, nodes.slice(1).map((_, i) => gedge(i, i, i + 1)), (i) => i);
    const start = layout.nodes.map((n) => [n.x, n.y]);
    layout.reheat(0.9);
    await until(() => layout!.nodes.some((n, i) => n.x !== start[i][0] || n.y !== start[i][1]));
    layout.cool();
    const before = layout.nodes.map((n) => [n.x, n.y]);
    await wait(80);
    expect(layout.nodes.map((n) => [n.x, n.y])).toEqual(before);
    expect(layout.active).toBe(false);
  });

  it("новый набор узлов, пока порция прежнего в пути, не останавливает раскладку (раунд 27)", async () => {
    layout = new Layout();
    let moves = 0;
    layout.onPositions = () => moves++;
    const nodes = Array.from({ length: 10 }, (_, i) => gnode(i));
    layout.setGraph(nodes, nodes.slice(1).map((_, i) => gedge(i, i, i + 1)), (i) => i);
    layout.reheat(0.9);
    // Срез перезагружен до прихода первой порции: она устарела, но worker должен продолжить с новым набором.
    layout.setGraph(nodes, nodes.slice(1).map((_, i) => gedge(i, i, i + 1)), (i) => i);
    layout.reheat(0.6);
    await until(() => moves > 3);
    expect(moves).toBeGreaterThan(3);
  });

  it("закрепление сохраняется при смене набора узлов", async () => {
    layout = new Layout();
    const nodes = Array.from({ length: 4 }, (_, i) => gnode(i));
    layout.setGraph(nodes, [gedge(0, 0, 1)], (i) => i);
    expect(layout.togglePin(1)).toBe(true);
    layout.setGraph(nodes.slice(0, 3), [gedge(0, 0, 1)], (i) => i);
    expect(layout.get(1)?.pinned).toBe(true);
    expect(layout.togglePin(1)).toBe(false);
  });
});

describe("одиночки кольцом, размер узла и размер карты (раунд 27)", () => {
  const withHub = (isolates: number, count = 30) => {
    const nodes: CoreNode[] = Array.from({ length: count + isolates }, (_, index) => ({ index, kind: "document", owner: index, x: Math.cos(index) * 80, y: Math.sin(index * 1.7) * 80, pinned: false }));
    const links: CoreLink[] = Array.from({ length: count - 1 }, (_, i) => ({ a: 0, b: i + 1, k: 0.5 }));
    return { nodes, links };
  };
  const settle = (nodes: CoreNode[], links: CoreLink[], tuning?: Partial<Tuning>) => {
    const core = new PhysicsCore(7, tuning);
    core.setForces({ ...DEFAULT_FORCES });
    core.setGraph(nodes, links);
    core.settle(900);
    const { indices, coords } = core.snapshot();
    return new Map(Array.from(indices).map((index, i) => [index, [coords[i * 2], coords[i * 2 + 1]] as const]));
  };

  it("документы без связей встают кольцом вокруг связной части, как в Obsidian", () => {
    const { nodes, links } = withHub(24);
    const at = settle(nodes, links);
    const core = Array.from({ length: 30 }, (_, i) => at.get(i)!);
    const cx = core.reduce((s, p) => s + p[0], 0) / core.length;
    const cy = core.reduce((s, p) => s + p[1], 0) / core.length;
    const coreRadius = Math.max(...core.map((p) => Math.hypot(p[0] - cx, p[1] - cy)));
    const loose = Array.from({ length: 24 }, (_, i) => at.get(30 + i)!);
    const dist = loose.map((p) => Math.hypot(p[0] - cx, p[1] - cy)).sort((a, b) => a - b);
    // Большинство одиночек за пределами связной части, и они окружают её со всех сторон.
    expect(dist[Math.floor(dist.length / 4)]).toBeGreaterThan(coreRadius * 0.8);
    const quadrants = new Set(loose.map((p) => `${Math.sign(p[0] - cx)}${Math.sign(p[1] - cy)}`));
    expect(quadrants.size).toBe(4);
  });

  it("раскладка детерминирована; прежнее поведение включается настройкой", () => {
    const run = (tuning?: Partial<Tuning>) => {
      const { nodes, links } = withHub(10);
      return [...settle(nodes, links, tuning).values()].flat();
    };
    expect(run()).toEqual(run());
    expect(run()).not.toEqual(run(LEGACY_TUNING));
  });

  it("радиус узла растёт с числом связей и ограничен сверху", () => {
    expect(nodeRadius("document", 0)).toBe(4);
    expect(nodeRadius("document", 9)).toBeGreaterThan(nodeRadius("document", 1));
    expect(nodeRadius("document", 10_000)).toBe(18);
    expect(nodeRadius("chunk", 50)).toBe(3.6);
  });

  it("отталкивание действует на любом расстоянии: карта растёт с числом узлов, а не сжимается в шар одного размера", () => {
    const span = (count: number) => {
      const nodes: CoreNode[] = Array.from({ length: count }, (_, index) => ({ index, kind: "document", owner: index, x: Math.cos(index) * 50, y: Math.sin(index * 1.3) * 50, pinned: false }));
      const links: CoreLink[] = Array.from({ length: count - 1 }, (_, i) => ({ a: i + 1, b: Math.floor(i / 3), k: 0.5 }));
      const xs = [...settle(nodes, links).values()].map((p) => p[0]);
      return Math.max(...xs) - Math.min(...xs);
    };
    expect(span(240)).toBeGreaterThan(span(60) * 1.4);
  });

  it("чанки раскрытого документа держатся у владельца, закреплённый документ остаётся на месте", () => {
    const nodes: CoreNode[] = [
      { index: 0, kind: "document", owner: 0, x: 0, y: 0, pinned: false },
      { index: 1, kind: "document", owner: 1, x: 40, y: 0, pinned: false },
      { index: 2, kind: "chunk", owner: 0, x: 5, y: 5, pinned: false },
      { index: 3, kind: "document", owner: 3, x: 99, y: 77, pinned: true },
    ];
    const core = new PhysicsCore(7);
    core.setGraph(nodes, [{ a: 0, b: 1, k: 0.5 }]);
    core.settle(200);
    const { indices, coords } = core.snapshot();
    const at = new Map(Array.from(indices).map((index, i) => [index, [coords[i * 2], coords[i * 2 + 1]]]));
    expect(at.get(3)).toEqual([99, 77]);
    expect(Math.hypot(at.get(2)![0] - at.get(0)![0], at.get(2)![1] - at.get(0)![1])).toBeLessThan(60);
  });
});
