import { describe, expect, it } from "vitest";
import type { SearchHit } from "../types";
import type { GEdge, GNode } from "./model";
import { buildQuerySlice, shapeQuery, type QueryInput } from "./query";
import { emptySlice, type Slice } from "./slice";

const doc = (index: number): GNode => ({ index, id: `d${index}`, kind: "document", label: `Док ${index}`, path: `docs/d${index}.md`, docsPath: `d${index}.md`, module: "m", owner: index, chunks: 2, ordinal: null, firstLine: 1, lastLine: 1 });
let edgeId = 0;
const link = (a: number, b: number): GEdge => ({ id: edgeId++, key: `l${a}-${b}`, kind: "markdown_link", a, b, raw: null, s: 1, lines: [1] });
const sim = (a: number, b: number, s: number): GEdge => ({ id: edgeId++, key: `s${a}-${b}`, kind: "document_similarity", a, b, raw: s, s, lines: [] });
const hit = (path: string, score: number, first = 1): SearchHit => ({ path, module: "m", title: path, heading_chain: "Раздел", first_line: first, last_line: first + 3, text: "", score, score_kind: "rerank" });

/** 0 — результат; 1..6 — его соседи по ссылке (1–5) и по смыслу (6); 7 — сосед соседа 1; 8 — вне связей. */
function base(): Slice {
  edgeId = 0;
  const nodes = Array.from({ length: 9 }, (_, i) => doc(i));
  const edges = [link(0, 1), link(0, 2), link(3, 0), link(0, 4), link(0, 5), sim(0, 6, 0.9), sim(0, 8, 0.4), link(1, 7)];
  return { ...emptySlice(), nodes, docCount: nodes.length, edges, visibleDocs: new Set(nodes.map((n) => n.index)) };
}

const input = (hits: SearchHit[], patch: Partial<QueryInput> = {}): QueryInput => ({ id: "q1", text: "запрос", hits, scoreKind: "rerank", requested: "hybrid", applied: "hybrid", minScore: 0.1, ...patch });
const ALL = ["markdown_link", "document_similarity"] as const;

describe("вид запроса", () => {
  it("запрос — отдельный узел, результаты в порядке выдачи, у документа с несколькими чанками одна оценка — наибольшая", () => {
    const { slice, info } = buildQuerySlice(base(), input([hit("docs/d0.md", 0.9, 1), hit("docs/d2.md", 0.7), hit("docs/d0.md", 0.95, 10)]));
    expect(slice.nodes[info.queryNode]).toMatchObject({ kind: "query", label: "запрос" });
    expect(info.results.map((r) => [r.node, r.rank, r.score])).toEqual([[0, 1, 0.95], [2, 2, 0.7]]);
    expect(info.results[0].fragments).toHaveLength(2);
    const q = slice.edges.filter((e) => e.kind === "query_result");
    expect(q.map((e) => [e.a, e.b, e.s, e.raw, e.scoreKind])).toEqual([[info.queryNode, 0, 0.95, 0.95, "rerank"], [info.queryNode, 2, 0.7, 0.7, "rerank"]]);
    expect(slice.nodes[0]).toMatchObject({ qrole: "result", rank: 1 });
  });

  it("общий массив узлов хранилища не меняется", () => {
    const b = base();
    const before = b.nodes.length;
    buildQuerySlice(b, input([hit("docs/d0.md", 0.9)]));
    expect(b.nodes).toHaveLength(before);
    expect(b.nodes[0].qrole).toBeUndefined();
    expect(b.edges.some((e) => e.kind === "query_result")).toBe(false);
  });

  it("результат без узла в графе остаётся в списке с причиной и не получает связи", () => {
    const { slice, info } = buildQuerySlice(base(), input([hit("docs/нет.md", 0.8), hit("docs/d0.md", 0.5)]));
    expect(info.results[0]).toMatchObject({ node: null, status: "no_graph_object" });
    expect(info.results[0].reason).toBeTruthy();
    expect(slice.edges.filter((e) => e.kind === "query_result")).toHaveLength(1);
  });

  it("без оценки (режим «Отношения и пути») связь нейтральна, оценка не выдумывается", () => {
    const { slice } = buildQuerySlice(base(), input([hit("docs/d0.md", 0.9)], { scoreKind: "none", applied: "graph", requested: "graph" }));
    const edge = slice.edges.find((e) => e.kind === "query_result")!;
    expect(edge).toMatchObject({ raw: null, neutral: true, scoreKind: "none" });
  });

  it("глубина 0 — запрос и результаты; 1 — окружение: ближайшие 4 по ссылкам и затем по смыслу; 2 — окружение окружения", () => {
    const { slice, info } = buildQuerySlice(base(), input([hit("docs/d0.md", 0.9)]));
    const d0 = shapeQuery(slice, info, [...ALL], 0.5, 0);
    expect(d0.nodes).toEqual([info.queryNode, 0]);
    const d1 = shapeQuery(slice, info, [...ALL], 0.5, 1);
    // Ссылки: 1, 2, 3, 4 (первые четыре; 5 — пятая, не входит); близость: 6 (0,9) — вне порога 0,4 узел 8 не входит.
    expect(d1.nodes.slice(2).sort((a, b) => a - b)).toEqual([1, 2, 3, 4, 6]);
    expect(d1.depthOf.get(1)).toBe(1);
    const d2 = shapeQuery(slice, info, [...ALL], 0.5, 2);
    expect(d2.nodes).toContain(7);
    expect(d2.depthOf.get(7)).toBe(2);
    expect(d2.anchorOf.get(7)).toBe(0);
  });

  it("режим связей и порог меняют окружение, связь запроса видна всегда", () => {
    const { slice, info } = buildQuerySlice(base(), input([hit("docs/d0.md", 0.9)]));
    const links = shapeQuery(slice, info, ["markdown_link"], 0.5, 1);
    expect(links.nodes).not.toContain(6);
    const meaning = shapeQuery(slice, info, ["document_similarity"], 0.95, 1);
    expect(meaning.nodes).toEqual([info.queryNode, 0]);
    expect(meaning.shown.edges).toBe(1);
  });

  it("при нехватке места результаты не вытесняются окружением, усечение называет число и сохраняет рёбра запроса", () => {
    const hits = [hit("docs/d0.md", 0.9), hit("docs/d2.md", 0.8), hit("docs/d1.md", 0.7)];
    const { slice, info } = buildQuerySlice(base(), input(hits));
    const cut = shapeQuery(slice, info, [...ALL], 0.5, 2, { nodes: 2, edges: 3 });
    expect(cut.nodes).toHaveLength(4);
    expect(cut.nodes.slice(0, 4)).toEqual([info.queryNode, 0, 2, 1]);
    expect(cut.truncated?.nodes).toBe(true);
    expect(cut.known.nodes).toBeGreaterThan(cut.shown.nodes);
    const kept = slice.edges.filter((e) => e.kind === "query_result").length;
    expect(cut.shown.edges).toBeGreaterThanOrEqual(kept);
  });

  it("порядок соседей не зависит от степени узла: сначала другие результаты", () => {
    const { slice, info } = buildQuerySlice(base(), input([hit("docs/d0.md", 0.9), hit("docs/d5.md", 0.8)]));
    const list = info.nbrs.get(0)!;
    expect(list[0].other).toBe(5);
  });
});
