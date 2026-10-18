// P0/OQ-91: read-only snapshot → existing-vector graph; no model or profile writes.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import { DatabaseSync } from 'node:sqlite';

const sha = data => crypto.createHash('sha256').update(data).digest('hex');
const read = p => JSON.parse(fs.readFileSync(p, 'utf8'));
const write = (p, value) => fs.writeFileSync(p, JSON.stringify(value, null, 2) + '\n', { flag: 'wx' });
const [command, ...args] = process.argv.slice(2);

if (command === 'export') {
  assert.equal(args.length, 2, 'export <project-root> <new-output-directory>');
  const [root, output] = args.map(p => path.resolve(p));
  assert(!fs.existsSync(output), 'output must be new');
  const db = new DatabaseSync(path.join(root, '.bars', 'index.sqlite'), { readOnly: true });
  let rows, meta;
  try {
    db.exec('BEGIN');
    meta = Object.fromEntries(db.prepare(`SELECT key,value FROM meta WHERE key IN (
      'schema_version','model_source','model_id','model_dim','model_doc_prefix','docs_root')`).all().map(r => [r.key, r.value]));
    assert.equal(meta.schema_version, '1', 'check query against new SQLite schema');
    rows = db.prepare(`SELECT d.path,d.module,d.title,d.hash,c.ordinal,c.first_line,c.last_line,
      c.heading_chain,c.text,c.embed_hash,v.vector
      FROM chunks c JOIN documents d ON d.id=c.document_id
      LEFT JOIN vectors v ON v.embed_hash=c.embed_hash ORDER BY d.path,c.ordinal`).all();
    db.exec('COMMIT');
  } finally { db.close(); }
  const excluded = p => /^(?:docs\/chats\/|docs\/features\/graphrag\/|docs\/knowledge\/(?:graph-|local-documentation-tools|agent-tool-usage))/.test(p);
  const selected = rows.filter(r => !excluded(r.path));
  const dim = Number(meta.model_dim);
  assert(Number.isInteger(dim) && dim > 0 && dim <= 65536);
  const header = Buffer.alloc(12);
  header.write('BGV1'); header.writeUInt32LE(selected.length, 4); header.writeUInt32LE(dim, 8);
  const blobs = [];
  const chunks = selected.map((row, index) => {
    assert(row.vector && row.vector.byteLength === dim * 4, `missing/invalid vector: ${row.path}:${row.ordinal}`);
    const { vector, ...chunk } = row;
    blobs.push(Buffer.from(vector));
    return { index, ...chunk };
  });
  const snapshot = { format: 1, created_at: new Date().toISOString(), meta, chunks };
  const snapshotBytes = JSON.stringify(snapshot) + '\n';
  const vectorBytes = Buffer.concat([header, ...blobs]);
  const documents = [...new Map(chunks.map(c => [c.path, { path: c.path, hash: c.hash, module: c.module }])).values()];
  const manifest = {
    format: 1, created_at: snapshot.created_at, meta, counts: { documents: documents.length, chunks: chunks.length, dimension: dim },
    excluded_documents: [...new Set(rows.filter(r => excluded(r.path)).map(r => r.path))],
    files: documents.map(d => d.path.startsWith(meta.docs_root + '/') ? d.path.slice(meta.docs_root.length + 1) : d.path),
    documents, snapshot_sha256: sha(snapshotBytes), vectors_sha256: sha(vectorBytes),
    limits: 'Development feasibility probe; no independent holdout or extraction acceptance.',
  };
  fs.mkdirSync(output);
  fs.writeFileSync(path.join(output, 'snapshot.json'), snapshotBytes, { flag: 'wx' });
  fs.writeFileSync(path.join(output, 'vectors.bin'), vectorBytes, { flag: 'wx' });
  write(path.join(output, 'manifest.json'), manifest);
  console.log(JSON.stringify({ output, ...manifest.counts, files: manifest.files }));
} else if (command === 'analyze') {
  assert(args.length >= 3, 'analyze <snapshot-dir> <baselines.json> <knn.json> [...]');
  const [dir, baselineFile, ...graphFiles] = args;
  const manifest = read(path.join(dir, 'manifest.json'));
  const snapshotBytes = fs.readFileSync(path.join(dir, 'snapshot.json'));
  assert.equal(sha(snapshotBytes), manifest.snapshot_sha256);
  assert.equal(sha(fs.readFileSync(path.join(dir, 'vectors.bin'))), manifest.vectors_sha256);
  const { chunks } = JSON.parse(snapshotBytes);
  const baselines = read(baselineFile);
  const hitKey = h => JSON.stringify([h.path, h.first_line, h.last_line, h.text]);
  const byHit = new Map(chunks.map(c => [hitKey(c), c]));
  const graphs = graphFiles.map(file => ({ file: path.basename(file), ...read(file) }));
  for (const graph of graphs) {
    assert.equal(graph.total_nodes, chunks.length);
    assert.equal(graph.partial, false, 'partial graph is not accepted as full kNN');
    assert.equal(graph.completed_nodes, chunks.length);
    assert.equal(graph.graph.length, chunks.length);
    const seen = new Set();
    for (const row of graph.graph) {
      assert(Number.isInteger(row.source) && row.source >= 0 && row.source < chunks.length);
      assert(!seen.has(row.source), 'duplicate graph source');
      seen.add(row.source);
      const targets = new Set();
      for (const [target, cosine] of row.neighbors) {
        assert(Number.isInteger(target) && target >= 0 && target < chunks.length && target !== row.source);
        assert(!targets.has(target), 'duplicate graph target');
        targets.add(target);
        assert(Number.isFinite(cosine) && cosine >= -1 && cosine <= 1);
      }
    }
  }
  const byteBudget = 16 * 1024;
  const pack = ids => {
    const unique = [...new Set(ids)], kept = [];
    let bytes = 0;
    for (const i of unique) {
      const size = Buffer.byteLength(chunks[i].text, 'utf8');
      if (bytes + size > byteBudget) continue;
      kept.push(i); bytes += size;
    }
    return { chunk_ids: kept, documents: [...new Set(kept.map(i => chunks[i].path))], text_bytes: bytes };
  };
  const cases = baselines.map(({ id, query, output }) => {
    const hits = output.results;
    const mapped = hits.map(h => byHit.get(hitKey(h))?.index);
    const invalid = mapped.some(x => x === undefined);
    if (invalid) return { id, query, invalid: true, reason: 'live MCP hits differ from frozen scope/revision/text', unmapped: hits.filter((h, i) => mapped[i] === undefined).map(h => ({ path: h.path, first_line: h.first_line })) };
    const ordinary = pack(mapped);
    const variants = graphs.map(graph => {
      const adjacency = new Map(graph.graph.map(r => [r.source, r.neighbors]));
      const seeds = mapped.slice(0, 3);
      const related = seeds.flatMap((s, rank) => (adjacency.get(s) ?? []).map(([target, cosine]) => ({ target, cosine, rank, source: s })));
      related.sort((a, b) => (b.cosine / (b.rank + 1)) - (a.cosine / (a.rank + 1)) || a.target - b.target);
      const expanded = pack([...seeds, ...related.map(r => r.target), ...mapped]);
      return {
        graph: graph.file, expanded,
        added_documents: expanded.documents.filter(p => !ordinary.documents.includes(p)),
        removed_documents: ordinary.documents.filter(p => !expanded.documents.includes(p)),
        related: related.slice(0, 10).map(r => ({ source: chunks[r.source].path, target: chunks[r.target].path, first_line: chunks[r.target].first_line, cosine: r.cosine })),
      };
    });
    return { id, query, invalid: false, applied: output.applied ?? output.params ?? null, baseline: ordinary, variants };
  });
  const result = {
    format: 1, snapshot_sha256: manifest.snapshot_sha256, vectors_sha256: manifest.vectors_sha256,
    meta: manifest.meta, counts: manifest.counts, byte_budget: byteBudget,
    budget_unit: 'UTF-8 source text bytes only; not the product response/envelope/token budget',
    graphs: graphs.map(({ file, seconds, comparisons, total_nodes, completed_nodes, partial }) => ({ file, seconds, comparisons, total_nodes, completed_nodes, partial })),
    cases, limits: 'Development probe; no gold labels, extraction scores, independent holdout, or end-to-end speed claims.',
  };
  console.log(JSON.stringify(result, null, 2));
} else {
  throw Error('command: export | analyze');
}
