// Демонстрационный ответ query-v1 для браузерного макета и компонентных тестов.
import { physicalFixture } from "./mock-physical";
import type { TreeDocument } from "./types";
import type { QueryRequest, ServerQuerySlice } from "./graph/query";
export function queryFixture(params: Record<string, unknown>, documents: TreeDocument[]): ServerQuerySlice {
  const r = params.request as QueryRequest;
  const physical = physicalFixture({ request: { scope: r.scope, snapshot: r.snapshot } }, documents);
  const results: ServerQuerySlice["results"] = [];
  for (const h of r.hits) {
    const known = results.find(x => x.path === h.path);
    if (known) { known.fragments.push(h); known.score = Math.max(known.score ?? 0, h.score ?? 0); continue; }
    const index = physical.nodes.findIndex(n => n.path === h.path);
    results.push({ channel: h.channel, status: "ready", path: h.path, title: h.title, rank: results.length + 1, node: index < 0 ? null : index, score: h.channel === "related" ? null : h.score, fragments: [h] });
  }
  const roots = results.flatMap(x => x.node === null ? [] : [x.node]);
  const views = {} as ServerQuerySlice["views"];
  for (const mode of ["links", "meaning", "all", "chunks"] as const) {
    const seen = new Set(roots);
    let frontier = roots;
    for (let d = 0; d < r.depth; d++) {
      const next: number[] = [];
      for (const id of frontier) {
        const edges = physical.edges.filter(e => (e.from === id || e.to === id) && ((mode === "links" || mode === "all") && e.kind === "markdown_link" || (mode === "meaning" || mode === "all") && e.kind === "document_similarity" && e.strength >= r.min_strength));
        for (const e of edges) { const n = e.from === id ? e.to : e.from; if (!seen.has(n)) { seen.add(n); next.push(n); } }
      }
      frontier = next;
    }
    views[mode] = [...seen];
  }
  return { physical, fingerprint: "mock-query", query_node:{id:`query:${r.id}`,label:r.text}, query_edges:results.flatMap(x=>x.node===null || x.channel === "related"?[]:[{to:x.node,raw_score:x.score,score_kind:r.score_kind,strength:x.score===null?.5:Math.min(1,Math.max(0,x.score)),neutral:x.score===null}]), id: r.id, text: r.text, score_kind: r.score_kind, results, views, depth_of: physical.nodes.map(() => 0), anchor_of: physical.nodes.map((_,i) => i), known_nodes: views.all.length + 1, known_edges: physical.edges.length + roots.length, used_bytes: 0 };
}
