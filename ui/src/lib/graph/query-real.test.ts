import { expect, it } from "vitest";
import recordings from "../../../../docs/chats/2026-10-26-query-graph-sources/real-slices.json";
import { acceptQuerySlice, shapeQuery, type QueryInput, type ServerQuerySlice } from "./query";
import type { GraphReply } from "../graph";

for (const recording of recordings) {
  it(`настоящая выдача bars-project: ${recording.request.text}`, () => {
    const r = recording.request;
    const mode = r.score_kind === "none" ? "graph" : recording.bundle ? "mix" : "hybrid";
    const input: QueryInput = {
      id:r.id, text:r.text, scope:r.scope, requested:mode, applied:mode,
      scoreKind:r.score_kind as QueryInput["scoreKind"], minScore:0,
      bundle:recording.bundle as unknown as GraphReply | null,
      hits:r.hits.filter(h=>h.channel === "primary").map(h=>({path:h.path,module:".",title:h.title,heading_chain:h.heading,first_line:h.first,last_line:h.last,text:"",score:h.score ?? 0,score_kind:"rerank",source_hash:h.source_hash})),
    };
    const server = recording.slice as unknown as ServerQuerySlice;
    const {slice,info} = acceptQuerySlice(server,input);
    expect(info.text).toBe(r.text);
    for (const mode of ["links","meaning","all"] as const) {
      const shape=shapeQuery(slice,info,mode);
      expect(shape.nodes.every(i=>!!slice.nodes[i])).toBe(true);
      expect(shape.nodes).toContain(info.queryNode);
    }
    for (const result of info.results.filter(r=>r.channel === "related")) {
      expect(result.score).toBeNull();
      expect(slice.edges.some(e=>e.kind === "query_result" && e.b === result.node)).toBe(false);
    }
    if (mode === "graph") expect(slice.edges.filter(e=>e.kind === "query_result").every(e=>e.neutral && e.raw === null)).toBe(true);
    expect(info.bundle).toEqual(recording.bundle);
  });
}
