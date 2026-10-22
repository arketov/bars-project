// Только браузерный макет и тесты: детерминированный срез `graph_physical` на синтетическом проекте.
// Настоящий срез строит Rust (crates/bars-graph/src/physical.rs); здесь повторены его правила пределов и раскрытия.
import { hash } from "./graph/model";
import type { Limits, PhysicalRequest, ServerEdge, ServerNode, ServerSlice } from "./graph/slice";

const MODULES = ["adr", "features", "manual"];
const DOCS = 36;
const SNAPSHOT = "mock-snapshot-0001";

const chunkCount = (doc: number) => (doc === 7 ? 120 : 2 + (doc % 5) * 2);
const docPath = (doc: number) => `${MODULES[doc % MODULES.length]}/doc-${String(doc).padStart(2, "0")}.md`;
const docId = (doc: number) => `doc-${doc}`;
const chunkId = (doc: number, ordinal: number) => `chunk-${doc}-${ordinal}`;

/** Документы по кругу модулей, внутри — по пути (то же правило, что в Rust). */
function roundRobin(limit: number): number[] {
  const groups = MODULES.map((m) => Array.from({ length: DOCS }, (_, i) => i).filter((i) => MODULES[i % MODULES.length] === m).sort((a, b) => docPath(a).localeCompare(docPath(b))));
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

export function physicalFixture(params: Record<string, unknown>): ServerSlice {
  const request = (params.request ?? {}) as Partial<PhysicalRequest>;
  const limits = (request.limits ?? { max_documents: 500, max_chunks: 500, max_edges: 4000, max_bytes: 1 << 20 }) as Limits;
  if (request.snapshot && request.snapshot !== SNAPSHOT) throw Object.assign(new Error("снимок графа устарел"), { data: { kind: "snapshot_expired" } });
  const shown = roundRobin(limits.max_documents);
  const nodes: ServerNode[] = shown.map((doc) => ({
    id: docId(doc), revision: "r1", kind: "document", label: `Документ ${doc}: ${MODULES[doc % MODULES.length]}`, path: `docs/${docPath(doc)}`, docs_path: docPath(doc), module: MODULES[doc % MODULES.length],
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
      nodes.push({ id: chunkId(doc, o), revision: "r1", kind: "chunk", label: `Раздел ${o + 1}`, path: `docs/${docPath(doc)}`, docs_path: docPath(doc), module: MODULES[doc % MODULES.length], owner: docId(doc), chunks: 0, ordinal: o, first_line: 4 + o * 6, last_line: 9 + o * 6 });
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
  const ready = { state: "ready" as const, note: "Макет: синтетические данные" };
  const reasons: string[] = [];
  if (shown.length < DOCS) reasons.push(`документов показано ${shown.length} из ${DOCS}`);
  if (kept.length < all.length) reasons.push(`не показано связей: ${all.length - kept.length}`);
  return {
    schema_version: 1,
    method: "physical-v1",
    snapshot: SNAPSHOT,
    scope: (request.scope ?? { modules: [], files: [] }) as ServerSlice["scope"],
    nodes,
    edges: kept,
    layers: { markdown_link: ready, document_similarity: ready, chunk_similarity: ready },
    totals: { documents: DOCS, chunks: Array.from({ length: DOCS }, (_, i) => chunkCount(i)).reduce((a, b) => a + b, 0), links: edges.length, document_pairs: sims.length },
    shown: { documents: shown.length, chunks: chunksShown, edges: kept.length },
    applied: limits,
    truncated: reasons.length > 0,
    limit_reasons: reasons,
    warnings: [],
    used_bytes: 0,
  };
}
