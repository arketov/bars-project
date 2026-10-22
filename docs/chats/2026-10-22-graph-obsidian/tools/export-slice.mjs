// Срез physical-v1 из готового индекса проекта, только чтение: документы, настоящие ссылки и близость документов
// по centroid (вес чанка — длина текста, k=5, tau=0,5, объединение направлений, сила — максимум). Для стенда раунда 27.
// node export-slice.mjs <корень проекта> <выход.json>
import { DatabaseSync } from "node:sqlite";
import { writeFileSync } from "node:fs";
import { join } from "node:path";

const [root, out] = process.argv.slice(2);
const db = new DatabaseSync(join(root, ".bars", "index.sqlite"), { readOnly: true });

const docs = db.prepare("select id, path, module, title from documents order by path").all();
console.error("documents sample:", docs.slice(0, 2));
const chunkCount = new Map(db.prepare("select document_id, count(*) as n from chunks group by document_id").all().map((r) => [r.document_id, r.n]));

// Ссылки: graph_edges links_to между узлами, у которых есть источник-документ.
const src = new Map(db.prepare("select node_id, source_path from graph_node_sources").all().map((r) => [r.node_id, r.source_path]));
const srcSample = [...src.values()].slice(0, 2);
console.error("graph source sample:", srcSample);
const byPath = new Map(docs.map((d, i) => [d.path, i]));
const resolve = (p) => (byPath.has(p) ? byPath.get(p) : byPath.get(p.replace(/^docs\//, "")) ?? byPath.get(`docs/${p}`));
const links = new Set();
for (const r of db.prepare("select from_id, to_id from graph_edges where predicate = '\"links_to\"'").all()) {
  const a = resolve(src.get(r.from_id) ?? "");
  const b = resolve(src.get(r.to_id) ?? "");
  if (a !== undefined && b !== undefined && a !== b) links.add(`${a}:${b}`);
}

// Centroid документов.
const vecOf = db.prepare("select vector from vectors where embed_hash = ?");
const centroids = docs.map(() => null);
let dim = 0;
for (const [i, d] of docs.entries()) {
  const rows = db.prepare("select text, embed_hash from chunks where document_id = ?").all(d.id);
  let acc = null;
  let total = 0;
  for (const c of rows) {
    const v = vecOf.get(c.embed_hash);
    if (!v) continue;
    const buf = Buffer.from(v.vector);
    const f = new Float32Array(buf.buffer, buf.byteOffset, buf.byteLength / 4);
    dim = f.length;
    let norm = 0;
    for (const x of f) norm += x * x;
    norm = Math.sqrt(norm) || 1;
    const w = [...c.text].length;
    acc ??= new Float64Array(f.length);
    for (let j = 0; j < f.length; j++) acc[j] += (w * f[j]) / norm;
    total += w;
  }
  if (!acc || !total) continue;
  let norm = 0;
  for (const x of acc) norm += x * x;
  norm = Math.sqrt(norm);
  if (!norm) continue;
  centroids[i] = Float32Array.from(acc, (x) => x / norm);
}
console.error("dim", dim, "centroids", centroids.filter(Boolean).length, "of", docs.length);

const K = 5;
const TAU = 0.5;
const pairs = new Map();
for (let i = 0; i < docs.length; i++) {
  const a = centroids[i];
  if (!a) continue;
  const near = [];
  for (let j = 0; j < docs.length; j++) {
    const b = centroids[j];
    if (i === j || !b) continue;
    let dot = 0;
    for (let t = 0; t < a.length; t++) dot += a[t] * b[t];
    near.push([j, dot]);
  }
  near.sort((x, y) => y[1] - x[1] || x[0] - y[0]);
  for (const [j, cos] of near.slice(0, K)) {
    if (cos < TAU) continue;
    const key = i < j ? `${i}:${j}` : `${j}:${i}`;
    pairs.set(key, Math.max(pairs.get(key) ?? -1, cos));
  }
}

const docsRoot = (p) => p.replace(/^docs\//, "");
const nodes = docs.map((d) => ({
  id: `doc-${d.id}`, revision: "r1", kind: "document", label: d.title || d.path, path: d.path.startsWith("docs/") ? d.path : `docs/${d.path}`,
  docs_path: docsRoot(d.path), module: d.module, owner: `doc-${d.id}`, chunks: chunkCount.get(d.id) ?? 0, ordinal: null, first_line: 0, last_line: 0,
}));
const edges = [
  ...[...links].map((k) => { const [from, to] = k.split(":").map(Number); return { kind: "markdown_link", from, to, raw: null, strength: 1, lines: [] }; }),
  ...[...pairs].sort((x, y) => y[1] - x[1]).map(([k, cos]) => { const [from, to] = k.split(":").map(Number); return { kind: "document_similarity", from, to, raw: cos, strength: Math.max(0, Math.min(1, cos)), lines: [] }; }),
];
const ready = (note) => ({ state: "ready", note });
const slice = {
  schema_version: 1, method: "physical-v1", snapshot: `export-${Date.now()}`, scope: { modules: [], files: [] }, nodes, edges,
  layers: { markdown_link: ready("ссылки"), document_similarity: ready(`близость документов посчитана для ${centroids.filter(Boolean).length} из ${docs.length}`), chunk_similarity: ready("чанки") },
  totals: { documents: docs.length, chunks: [...chunkCount.values()].reduce((a, b) => a + b, 0), links: links.size, document_pairs: pairs.size },
  shown: { documents: docs.length, chunks: 0, edges: edges.length },
  applied: { max_documents: 2000, max_chunks: 2000, max_edges: 10000, max_bytes: 2 << 20 }, truncated: false, limit_reasons: [], warnings: [], used_bytes: 0,
};
writeFileSync(out, JSON.stringify(slice));
console.error(`${root}: документов ${docs.length}, ссылок ${links.size}, пар близости ${pairs.size}`);
