import { beforeEach, describe, expect, it, vi } from "vitest";
import type { GraphNode, GraphReply } from "./graph";

const call = vi.hoisted(() => vi.fn());
vi.mock("./api", () => ({ call }));
import { docsRelative, loadBacklinks } from "./backlinks";

const ref = (kind: "document" | "section", id: string) => ({ kind, id, revision: "r" });
const node = (kind: "document" | "section", id: string, owner: string): GraphNode => ({ reference: ref(kind, id), label: id, namespace: "", aliases: [], source_ids: [owner], evidence: [], roles: [] });
const source = (id: string, path: string, title: string) => ({ reference: ref("document", id), path, docs_path: path.replace(/^docs\//, ""), module: "m", title, hash: "h" });
const reply = (patch: Partial<GraphReply>): GraphReply => ({ schema_version: 1, operation: "graph", snapshot: "s", scope: { modules: [], files: [] }, layer_state: "ready", layer_states: {}, coverage: {} as GraphReply["coverage"], nodes: [], sources: [], assertions: [], evidence: [], items: [], truncated: false, limit_reasons: [], next_cursor: null, warnings: [], resolution: "", candidates: [], missing_roles: [], budget_mode: "", used_bytes: 0, elapsed_ms: 0, method: "", ...patch });
const edge = (id: string, from: string, to: string, evidence: string[]) => ({ type: "edge" as const, edge: { id, from, to, predicate: "links_to", polarity: "positive" as const, qualifiers: {}, origin: "structural", evidence } });
const proof = (id: string, line: number) => ({ id, document: ref("document", "B"), span: { start: 0, end: 1, first_line: line, last_line: line }, quote: "", source_hash: "", method: "" });

describe("входящие ссылки", () => {
  beforeEach(() => call.mockReset());

  it("путь от корня документации", () => {
    expect(docsRelative("docs/manual/a.md", "docs")).toBe("manual/a.md");
    expect(docsRelative("manual/a.md", ".")).toBe("manual/a.md");
  });

  it("ссылки из разделов и на разделы сводятся к документам с номерами строк", async () => {
    call.mockResolvedValueOnce(reply({ sources: [source("A", "docs/a.md", "А")], nodes: [node("document", "A", "A"), node("section", "A#1", "A")] }));
    call.mockResolvedValueOnce(
      reply({
        sources: [source("B", "docs/b.md", "Б"), source("C", "docs/c.md", "В")],
        nodes: [node("section", "B#1", "B"), node("section", "C#1", "C")],
        evidence: [proof("e1", 7), proof("e2", 3), proof("e3", 9)],
        items: [edge("1", "B#1", "A", ["e1", "e2"]), edge("2", "C#1", "A#1", ["e3"]), edge("3", "A#1", "A", ["e3"])],
      }),
    );
    const result = await loadBacklinks("/p", "docs/a.md", "docs");
    expect(call.mock.calls[0][1].request.scope.files).toEqual(["a.md"]);
    expect(call.mock.calls[1][1].request.roots.map((r: { id: string }) => r.id)).toEqual(["A", "A#1"]);
    expect(result.items).toEqual([
      { path: "docs/b.md", title: "Б", lines: [3, 7] },
      { path: "docs/c.md", title: "В", lines: [9] },
    ]);
    expect(result.note).toBe("");
  });

  it("документа нет в графе — пояснение вместо пустого списка", async () => {
    call.mockResolvedValueOnce(reply({ layer_state: "not_built" }));
    const result = await loadBacklinks("/p", "docs/a.md", "docs");
    expect(result.items).toEqual([]);
    expect(result.note).toContain("не построен");
  });
});
