import { describe, expect, it } from "vitest";
import { loadSlice, parseFlags, queryView, shapeQuery } from "./data";
import { drawStrength, type EdgeKind } from "./model";

// Правила вида запроса (query-graph.md) на снимке стенда: порядок включения, глубина, пределы, режимы поиска, статусы результатов.
const base = loadSlice("project");
const ALL: EdgeKind[] = ["markdown_link", "document_similarity"];
const build = (flags = "", text = "физический граф") => queryView(base, text, 1, parseFlags(flags))!;
const shape = (flags = "", depth = 1, kinds: EdgeKind[] = ALL, budget = { nodes: 200, edges: 800 }, minS = 0.82) => {
  const slice = build(flags);
  return { slice, shape: shapeQuery(slice, kinds, minS, depth, budget) };
};

describe("вид запроса: построение", () => {
  it("детерминирован: тот же запрос, режим и глубина дают тот же набор и порядок", () => {
    expect(shape().shape.nodes).toEqual(shape().shape.nodes);
  });

  it("глубина 0 — запрос и результаты, глубина 2 не уменьшает набор", () => {
    const d0 = shape("", 0).shape;
    const d1 = shape("", 1).shape;
    const d2 = shape("", 2).shape;
    const results = build().queryInfo!.results.length;
    expect(d0.nodes.length).toBe(1 + results);
    expect(d1.nodes.length).toBeGreaterThan(d0.nodes.length);
    expect(d2.nodes.length).toBeGreaterThanOrEqual(d1.nodes.length);
    for (const n of d1.nodes) expect(d2.nodes).toContain(n);
  });

  it("запрос первый, результаты по месту в выдаче, затем окружение", () => {
    const { slice, shape: s } = shape("", 2);
    const info = slice.queryInfo!;
    expect(s.nodes[0]).toBe(info.queryNode);
    expect(s.nodes.slice(1, 1 + info.results.length)).toEqual(info.results.map((r) => r.node));
    expect(s.depthOf.get(s.nodes[1 + info.results.length])).toBe(1);
  });

  it("окружение берётся по кругу: первые соседи всех результатов раньше вторых", () => {
    const { slice, shape: s } = shape("", 1, ["markdown_link"]);
    const info = slice.queryInfo!;
    const firsts = new Set<number>();
    for (const r of info.results) {
      const n = (info.nbrs.get(r.node) ?? []).filter((x) => x.kind === "markdown_link").find((x) => !info.results.some((q) => q.node === x.other));
      if (n) firsts.add(n.other);
    }
    const env = s.nodes.slice(1 + info.results.length);
    const firstPositions = [...firsts].map((n) => env.indexOf(n)).filter((i) => i >= 0);
    expect(firstPositions.length).toBeGreaterThan(0);
    expect(Math.max(...firstPositions)).toBeLessThan(firsts.size + 1 + info.results.length);
  });

  it("пределы: результаты не вытесняются, причина названа, продолжение добавляет без потерь", () => {
    const tight = shape("many,limit", 2, ALL, { nodes: 36, edges: 110 });
    const info = tight.slice.queryInfo!;
    expect(tight.shape.truncated).not.toBeNull();
    expect(tight.shape.nodes.length).toBeLessThanOrEqual(36);
    for (const r of info.results) expect(tight.shape.nodes).toContain(r.node);
    expect(tight.shape.known.nodes).toBeGreaterThan(tight.shape.shown.nodes);
    const wider = shapeQuery(tight.slice, ALL, 0.82, 2, { nodes: 78, edges: 250 });
    expect(wider.nodes.slice(0, tight.shape.nodes.length)).toEqual(tight.shape.nodes);
    expect(new Set(wider.nodes).size).toBe(wider.nodes.length);
  });

  it("предел рёбер усекает связи, а не результаты", () => {
    const { slice, shape: s } = shape("many", 2, ALL, { nodes: 500, edges: 30 });
    expect(s.truncated?.edges).toBe(true);
    expect(s.edgeIds?.size).toBeLessThan(30);
    expect(s.nodes.length).toBe(s.known.nodes);
    expect(slice.queryInfo!.results.length).toBe(20);
  });

  it("режим связей меняет набор, но не порядок включения общих узлов", () => {
    const links = shape("", 1, ["markdown_link"]).shape.nodes;
    const both = shape("", 1, ALL).shape.nodes;
    for (const n of links) expect(both).toContain(n);
  });
});

describe("вид запроса: оценки и каналы", () => {
  it("rerank и косинус подписаны видом; rerank не масштабируется окном модели", () => {
    const rerank = build();
    expect(rerank.queryInfo!.scoreKind).toBe("rerank");
    const edge = rerank.edges.find((e) => e.kind === "query_result")!;
    expect(drawStrength(edge, "window")).toBe(edge.s);
    const cos = build("cosine");
    expect(cos.queryInfo!.scoreKind).toBe("cosine");
    const c = cos.edges.find((e) => e.kind === "query_result")!;
    expect(drawStrength(c, "window")).not.toBe(c.s);
  });

  it("«Отношения и пути»: оценки нет, связь нейтральна", () => {
    const g = build("graph");
    expect(g.queryInfo!.scoreKind).toBe("none");
    for (const r of g.queryInfo!.results) expect(r.score).toBeNull();
    for (const e of g.edges.filter((x) => x.kind === "query_result")) expect(e.neutral).toBe(true);
  });

  it("«Со связями»: связанные источники без оценки и без связи запроса, основание названо", () => {
    const m = build("mix");
    const related = m.queryInfo!.results.filter((r) => r.channel === "related");
    expect(related.length).toBe(2);
    for (const r of related) {
      expect(r.score).toBeNull();
      expect(r.reason).toBeTruthy();
      expect(m.edges.some((e) => e.kind === "query_result" && e.b === r.node)).toBe(false);
    }
    expect(related.some((r) => r.onMap)).toBe(true);
    expect(related.some((r) => !r.onMap)).toBe(true);
  });

  it("откат «Со связями» на «Обычный» называет причину", () => {
    const f = build("mixfallback");
    expect(f.queryInfo!.requested).toBe("mix");
    expect(f.queryInfo!.applied).toBe("hybrid");
    expect(f.queryInfo!.fallbackNote).toBeTruthy();
    expect(f.queryInfo!.results.every((r) => r.channel === "primary")).toBe(true);
  });
});

describe("вид запроса: результаты без объекта графа и недоступный слой", () => {
  it("результат без объекта графа остаётся узлом с причиной и без окружения", () => {
    const { slice, shape: s } = shape("nograph", 2);
    const ghosts = slice.queryInfo!.results.filter((r) => r.status !== "ready");
    expect(ghosts.map((r) => r.status)).toEqual(["no_graph_object", "changed_since_search", "deleted_since_search"]);
    for (const g of ghosts) {
      expect(s.nodes).toContain(g.node);
      expect(slice.nodes[g.node].qrole).toBe("ghost");
      expect(g.reason).toBeTruthy();
      expect((slice.queryInfo!.nbrs.get(g.node) ?? []).length).toBe(0);
    }
  });

  it("слой связей недоступен: только запрос и результаты", () => {
    const { slice, shape: s } = shape("graphnr", 2);
    expect(s.nodes.length).toBe(1 + slice.queryInfo!.results.length);
    expect(slice.layers.markdown_link.state).toBe("failed");
  });

  it("пустая выдача: узел запроса без результатов", () => {
    const { slice, shape: s } = shape("empty", 1);
    expect(slice.queryInfo!.results.length).toBe(0);
    expect(s.nodes).toEqual([slice.queryInfo!.queryNode]);
  });

  it("неизвестное общее число не влияет на показанное", () => {
    const { slice } = shape("many,limitu", 2, ALL, { nodes: 36, edges: 110 });
    expect(slice.queryInfo!.totalKnown).toBe(false);
  });
});
