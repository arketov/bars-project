import { expect, it } from "vitest";
import { LOCAL_LIMITS, adjacency, localGraph, mapPaths } from "./navigation";
import type { EdgeKind, GEdge } from "./model";

let next = 0;
const edge = (a: number, b: number, s = 1, kind: EdgeKind = "markdown_link"): GEdge => ({ id: next++, key: `${kind}:${a}:${b}`, kind, a, b, raw: kind === "markdown_link" ? null : s, s, lines: [] });

// 0 → 1 → 2 → 3 → 4, обходной путь 0 → 5 → 3, дальний хвост 4 → 6.
const chain = () => {
  next = 0;
  return [edge(0, 1), edge(1, 2), edge(2, 3), edge(3, 4), edge(0, 5, 0.5), edge(5, 3, 0.5), edge(4, 6)];
};

it("локальный граф: глубина считает шаги, ссылки идут в обе стороны", () => {
  const adj = adjacency(chain());
  const one = localGraph(adj, 3, 1);
  expect([...one.depthOf.keys()].sort()).toEqual([2, 3, 4, 5]);
  expect(one.depthOf.get(3)).toBe(0);
  expect(one.truncated).toBe(false);
  const two = localGraph(adj, 3, 2);
  expect([...two.depthOf.keys()].sort()).toEqual([0, 1, 2, 3, 4, 5, 6]);
  expect(two.depthOf.get(1)).toBe(2);
  expect(two.edgeIds.size).toBe(7);
});

it("локальный граф: предел узлов отбрасывает слабые связи, сообщает известное число и не режет рёбра между показанными", () => {
  const adj = adjacency(chain());
  const cut = localGraph(adj, 3, 2, { nodes: 4, edges: 100 });
  expect(cut.truncated).toBe(true);
  expect(cut.depthOf.size).toBe(4);
  // Сильные соседи (шаг 1: 2 и 4) остаются, слабый 5 теряется раньше шага 2.
  expect(cut.depthOf.has(2) && cut.depthOf.has(4)).toBe(true);
  expect(cut.known.nodes).toBe(7);
  for (const id of cut.edgeIds) {
    const e = chain().find((x) => x.id === id)!;
    expect(cut.depthOf.has(e.a) && cut.depthOf.has(e.b)).toBe(true);
  }
});

it("локальный граф: предел связей оставляет сильные и помечает усечение", () => {
  next = 0;
  const edges = [edge(0, 1, 1), edge(0, 2, 1), edge(1, 2, 0.5, "document_similarity"), edge(0, 3, 1)];
  const cut = localGraph(adjacency(edges), 0, 1, { nodes: 100, edges: 3 });
  expect(cut.truncated).toBe(true);
  expect(cut.edgeIds.size).toBe(3);
  expect(cut.edgeIds.has(edges[2].id)).toBe(false);
  expect(cut.known.edges).toBe(4);
});

it("локальный граф: у документа без связей остаётся он один", () => {
  const result = localGraph(adjacency([]), 9, 3);
  expect([...result.depthOf]).toEqual([[9, 0]]);
  expect(result.edgeIds.size).toBe(0);
});

it("ступени предела растут до 500/2000", () => {
  expect(LOCAL_LIMITS[0]).toEqual({ nodes: 200, edges: 800 });
  expect(LOCAL_LIMITS[LOCAL_LIMITS.length - 1]).toEqual({ nodes: 500, edges: 2000 });
});

it("путь по карте: кратчайший по числу шагов первым, затем остальные, всего до трёх", () => {
  const paths = mapPaths(adjacency(chain()), 0, 4);
  expect(paths.map((p) => p.nodes)).toEqual([[0, 5, 3, 4], [0, 1, 2, 3, 4]]);
  expect(paths[0].steps.length).toBeLessThan(paths[1].steps.length);
  expect(paths.length).toBeLessThanOrEqual(3);
});

it("путь по карте: при равной длине выше путь с более сильными связями", () => {
  next = 0;
  const weak = [edge(0, 1, 0.5, "document_similarity"), edge(1, 3, 0.5, "document_similarity")];
  const strong = [edge(0, 2, 0.9, "document_similarity"), edge(2, 3, 0.9, "document_similarity")];
  const paths = mapPaths(adjacency([...weak, ...strong]), 0, 3);
  expect(paths.map((p) => p.nodes)).toEqual([[0, 2, 3], [0, 1, 3]]);
});

it("путь по карте: по направлению ссылок обратный ход закрыт, без переключателя ссылки идут в обе стороны", () => {
  next = 0;
  const edges = [edge(1, 0), edge(1, 2)];
  expect(mapPaths(adjacency(edges), 0, 2)[0].nodes).toEqual([0, 1, 2]);
  expect(mapPaths(adjacency(edges, true), 0, 2)).toEqual([]);
  const forward = mapPaths(adjacency(edges, true), 1, 2)[0];
  expect(forward.steps[0].forward).toBe(true);
  expect(mapPaths(adjacency(edges), 0, 2)[0].steps[0].forward).toBe(false);
});

it("путь по карте: длиннее шести шагов не ищется; тот же узел и несвязные узлы дают пустой ответ", () => {
  next = 0;
  const long = Array.from({ length: 7 }, (_, i) => edge(i, i + 1));
  expect(mapPaths(adjacency(long), 0, 7)).toEqual([]);
  expect(mapPaths(adjacency(long), 0, 6)[0].steps.length).toBe(6);
  expect(mapPaths(adjacency(long), 3, 3)).toEqual([]);
  expect(mapPaths(adjacency([edge(0, 1), edge(2, 3)]), 0, 3)).toEqual([]);
});

it("путь по карте: взаимные ссылки — одна линия, варианты не дублируются по параллельным рёбрам", () => {
  next = 0;
  const edges = [edge(0, 1), edge(1, 0), edge(1, 2)];
  const paths = mapPaths(adjacency(edges), 0, 2);
  expect(paths.map((p) => p.nodes)).toEqual([[0, 1, 2]]);
});
