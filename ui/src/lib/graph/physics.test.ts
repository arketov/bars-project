import { afterEach, describe, expect, it } from "vitest";
import { Layout } from "./layout";
import type { GEdge, GNode } from "./model";
import { DEFAULT_FORCES, PhysicsCore, type CoreLink, type CoreNode } from "./physics-core";
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
    const at = new Map(core.nodes.map((n) => [n.index, n]));
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
    const pinned = core.nodes.find((n) => n.index === 2)!;
    expect([pinned.x, pinned.y]).toEqual([10, 20]);
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
