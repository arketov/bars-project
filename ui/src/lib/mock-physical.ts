// Только браузерный макет и тесты: документы дерева, демонстрационные связи.
// Настоящий срез строит Rust (crates/bars-graph/src/physical.rs); здесь повторены его правила пределов и раскрытия.
import { hash } from "./graph/model";
import type { TreeDocument } from "./types";
import type { Limits, PhysicalRequest, ServerEdge, ServerNode, ServerSlice } from "./graph/slice";

const SNAPSHOT = "mock-snapshot-0001";

const docId = (doc: number) => `doc-${doc}`;
const chunkId = (doc: number, ordinal: number) => `chunk-${doc}-${ordinal}`;

/** Документы по кругу модулей, внутри — по пути (то же правило, что в Rust). */
function roundRobin(documents: TreeDocument[], selected: number[], limit: number): number[] {
  const modules = [...new Set(documents.map((doc) => doc.module))].sort();
  const groups = modules.map((m) => selected.filter((i) => documents[i].module === m).sort((a, b) => documents[a].path.localeCompare(documents[b].path)));
  const picked: number[] = [];
  for (let round = 0; picked.length < limit; round++) {
    let any = false;
    for (const list of groups) {
      if (round < list.length && picked.length < limit) {
        picked.push(list[round]);
        any = true;
      }
    }
    if (!any) break;
  }
  return picked;
}

export function physicalFixture(params: Record<string, unknown>, documents: TreeDocument[]): ServerSlice {
  const DOCS = documents.length;
  const chunkCount = (doc: number) => documents[doc].chunks;
  const docPath = (doc: number) => documents[doc].path.replace(/^docs\//, "");
  const request = (params.request ?? {}) as Partial<PhysicalRequest>;
  const limits = (request.limits ?? { max_documents: 500, max_chunks: 500, max_edges: 4000, max_bytes: 1 << 20 }) as Limits;
  if (request.snapshot && request.snapshot !== SNAPSHOT) throw Object.assign(new Error("снимок графа устарел"), { data: { kind: "snapshot_expired" } });
  const scope = request.scope ?? { modules: [], files: [] };
  const selected = documents.flatMap((doc, i) => (!scope.modules.length && !scope.files.length) || scope.modules.includes(doc.module) || scope.files.includes(docPath(i)) || scope.files.includes(doc.path) ? [i] : []);
  const shown = roundRobin(documents, selected, limits.max_documents);
  const nodes: ServerNode[] = shown.map((doc) => ({
    id: docId(doc), revision: "r1", kind: "document", label: documents[doc].title, path: documents[doc].path, docs_path: docPath(doc), module: documents[doc].module,
    owner: docId(doc), chunks: chunkCount(doc), ordinal: null, first_line: 1, last_line: 1,
  }));
  const at = new Map(shown.map((doc, i) => [doc, i]));
  const edges: ServerEdge[] = [];
  const link = (a: number, b: number, line: number) => {
    if (at.has(a) && at.has(b)) edges.push({ kind: "markdown_link", from: at.get(a)!, to: at.get(b)!, raw: null, strength: 1, lines: [line] });
  };
  for (let i = 0; i < DOCS; i++) {
    link(i, (i + 1) % DOCS, 3 + (i % 5));
    if (i % 4 === 0) link((i + 1) % DOCS, i, 9);
    if (i % 6 === 0) link(i, (i + 7) % DOCS, 12);
  }
  const sims: ServerEdge[] = [];
  for (let i = 0; i < DOCS; i++)
    for (const step of [2, 5]) {
      const j = (i + step) % DOCS;
      const cosine = Math.round((0.6 + hash(i * 31 + j, 4) * 0.33) * 1000) / 1000;
      if (at.has(i) && at.has(j)) sims.push({ kind: "document_similarity", from: at.get(i)!, to: at.get(j)!, raw: cosine, strength: cosine, lines: [] });
    }
  sims.sort((a, b) => (b.raw ?? 0) - (a.raw ?? 0));
  let chunksShown = 0;
  const chunkAt = new Map<string, number>();
  for (const { document, shown: count } of request.expanded ?? []) {
    const doc = Number(document.replace("doc-", ""));
    if (!at.has(doc)) continue;
    for (let o = 0; o < Math.min(count, chunkCount(doc)) && chunksShown < limits.max_chunks; o++) {
      chunkAt.set(chunkId(doc, o), nodes.length);
      nodes.push({ id: chunkId(doc, o), revision: "r1", kind: "chunk", label: `Раздел ${o + 1}`, path: documents[doc].path, docs_path: docPath(doc), module: documents[doc].module, owner: docId(doc), chunks: 0, ordinal: o, first_line: 4 + o * 6, last_line: 9 + o * 6 });
      chunksShown++;
    }
  }
  const chunkSims: ServerEdge[] = [];
  for (const [id, index] of chunkAt) {
    const [, d, o] = id.split("-").map(Number);
    for (const other of [chunkId(d, o + 1), chunkId((d + 1) % DOCS, o)]) {
      const j = chunkAt.get(other);
      if (j === undefined) continue;
      const cosine = Math.round((0.62 + hash(o * 13 + d, 8) * 0.3) * 1000) / 1000;
      chunkSims.push({ kind: "chunk_similarity", from: index, to: j, raw: cosine, strength: cosine, lines: [] });
    }
  }
  const all = [...edges, ...sims, ...chunkSims];
  const kept = all.slice(0, limits.max_edges);
  const ready = { state: "ready" as const, note: "Макет: документы дерева, демонстрационные связи" };
  const reasons: string[] = [];
  if (shown.length < selected.length) reasons.push(`документов показано ${shown.length} из ${selected.length}`);
  if (kept.length < all.length) reasons.push(`не показано связей: ${all.length - kept.length}`);
  return {
    schema_version: 1,
    method: "physical-v1",
    snapshot: SNAPSHOT,
    scope: (request.scope ?? { modules: [], files: [] }) as ServerSlice["scope"],
    nodes,
    edges: kept,
    layers: { markdown_link: ready, document_similarity: ready, chunk_similarity: ready },
    totals: { documents: selected.length, chunks: selected.reduce((sum, i) => sum + chunkCount(i), 0), links: edges.length, document_pairs: sims.length },
    shown: { documents: shown.length, chunks: chunksShown, edges: kept.length },
    applied: limits,
    truncated: reasons.length > 0,
    limit_reasons: reasons,
    warnings: [],
    used_bytes: 0,
  };
}
