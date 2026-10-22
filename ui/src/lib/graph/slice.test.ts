import { describe, expect, it } from "vitest";
import { SliceStore, emptySlice, type ServerNode, type ServerSlice } from "./slice";

const doc = (id: string, label = id): ServerNode => ({ id, revision: "r", kind: "document", label, path: `docs/${id}.md`, docs_path: `${id}.md`, module: "m", owner: id, chunks: 3, ordinal: null, first_line: 1, last_line: 1 });
const chunk = (id: string, owner: string, ordinal: number): ServerNode => ({ ...doc(id), kind: "chunk", owner, chunks: 0, ordinal, first_line: 4 + ordinal, last_line: 8 + ordinal });

const server = (nodes: ServerNode[], edges: ServerSlice["edges"] = []): ServerSlice => ({
  schema_version: 1,
  method: "physical-v1",
  snapshot: "s1",
  scope: { modules: [], files: [] },
  nodes,
  edges,
  layers: emptySlice().layers,
  totals: { documents: nodes.filter((n) => n.kind === "document").length, chunks: 3, links: 0, document_pairs: 0 },
  shown: { documents: 0, chunks: 0, edges: edges.length },
  applied: emptySlice().applied,
  truncated: false,
  limit_reasons: [],
  warnings: [],
  used_bytes: 0,
});

describe("модель среза окна", () => {
  it("рёбра приходят индексами среза и превращаются в индексы панели с направлением и строками", () => {
    const store = new SliceStore();
    const slice = store.apply(server([doc("a"), doc("b")], [{ kind: "markdown_link", from: 1, to: 0, raw: null, strength: 1, lines: [7] }]));
    expect(slice.docCount).toBe(2);
    expect(slice.edges).toHaveLength(1);
    expect(slice.edges[0]).toMatchObject({ kind: "markdown_link", a: 1, b: 0, s: 1, lines: [7], raw: null });
  });

  it("индексы узлов устойчивы при продолжении и раскрытии: раскладка и выбор не сбрасываются", () => {
    const store = new SliceStore();
    const first = store.apply(server([doc("a"), doc("b")]));
    const a = store.indexOf("a")!;
    const b = store.indexOf("b")!;
    // «Показать ещё»: порядок документов в новом ответе другой, добавились c и чанки a.
    const second = store.apply(server([doc("c"), doc("b"), doc("a"), chunk("a1", "a", 0), chunk("a2", "a", 1)]));
    expect(store.indexOf("a")).toBe(a);
    expect(store.indexOf("b")).toBe(b);
    expect(second.visibleDocs.size).toBe(3);
    expect(second.chunksOf.get(a)).toEqual([store.indexOf("a1"), store.indexOf("a2")]);
    expect(second.nodes[store.indexOf("a1")!]).toMatchObject({ kind: "chunk", owner: a, ordinal: 0, firstLine: 4 });
    expect(first.nodes).toBe(second.nodes);
  });

  it("свёрнутые чанки остаются в модели, но не в видимых; рёбра заменяются целиком", () => {
    const store = new SliceStore();
    store.apply(server([doc("a"), chunk("a1", "a", 0)], [{ kind: "chunk_similarity", from: 0, to: 1, raw: 0.7, strength: 0.7, lines: [] }]));
    const collapsed = store.apply(server([doc("a")]));
    expect(collapsed.chunksOf.get(store.indexOf("a")!)).toBeUndefined();
    expect(collapsed.edges).toHaveLength(0);
    expect(collapsed.nodes[store.indexOf("a1")!].kind).toBe("chunk");
  });

  it("чанк чужого владельца вне среза и ребро к неизвестному узлу отбрасываются", () => {
    const store = new SliceStore();
    const slice = store.apply(server([doc("a"), chunk("x1", "ghost", 0)], [{ kind: "document_similarity", from: 0, to: 1, raw: 0.9, strength: 0.9, lines: [] }, { kind: "document_similarity", from: 0, to: 9, raw: 0.9, strength: 0.9, lines: [] }]));
    expect(slice.chunksOf.size).toBe(0);
    expect(slice.edges).toHaveLength(0);
  });
});
