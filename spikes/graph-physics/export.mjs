// Research-only projection of an existing graph. No models, writes or reindex.
import { DatabaseSync } from 'node:sqlite';
import { createHash } from 'node:crypto';
import { resolve, join } from 'node:path';
import { writeFileSync } from 'node:fs';
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
export function snapshot(root, name) {
  const db = new DatabaseSync(join(resolve(root), '.bars/index.sqlite'), { readOnly: true });
  let nodes, edges, sources, meta;
  try {
    db.exec('BEGIN');
    nodes = db.prepare('SELECT data FROM graph_nodes ORDER BY id').all().map(r => JSON.parse(r.data));
    edges = db.prepare('SELECT data FROM graph_edges ORDER BY id').all().map(r => JSON.parse(r.data));
    sources = db.prepare('SELECT path,revision FROM graph_sources ORDER BY path').all();
    meta = db.prepare('SELECT key,value FROM graph_meta ORDER BY key').all();
    db.exec('COMMIT');
  } finally { db.close(); }
  const kinds = Object.fromEntries([...new Set(nodes.map(n => n.reference.kind))].map(k => [k, nodes.filter(n => n.reference.kind === k).length]));
  const idMap = new Map(nodes.map(n => [n.reference.id, n]));
  const views = ['documents', 'documents-chunks'].map(view => {
    const kept = nodes.filter(n => n.reference.kind === 'document' || (view !== 'documents' && n.reference.kind === 'chunk'));
    const ids = new Set(kept.map(n => n.reference.id));
    const links = new Map();
    for (const edge of edges) {
      // Projection preserves link/similarity provenance; contains is a research load only.
      if (!['links_to', 'similar_to', 'contains'].includes(edge.predicate)) continue;
      if (view === 'documents' && edge.predicate === 'contains') continue;
      const endpoints = id => ids.has(id) ? [id] : (idMap.get(id)?.source_ids ?? []).filter(id => ids.has(id));
      for (const source of endpoints(edge.from)) for (const target of endpoints(edge.to)) {
        if (source === target) continue;
        const [a, b] = [source, target].sort();
        const key = JSON.stringify([a, b, edge.predicate]);
        // Max is a render-load projection, not a proposed document similarity contract.
        const weight = edge.score == null ? 1 : Math.min(1, Math.max(0, edge.score));
        const previous = links.get(key);
        if (!previous || previous.weight < weight) links.set(key, { source: a, target: b, weight, predicate: edge.predicate });
      }
    }
    return { name: `${name}/${view}`, nodes: kept.map((n, i) => ({ id: n.reference.id, label: n.label, kind: n.reference.kind, x: Math.cos(i * 2.399963) * Math.sqrt(i + 1) * 12, y: Math.sin(i * 2.399963) * Math.sqrt(i + 1) * 12 })), edges: [...links.values()] };
  });
  const manifest = { name, capturedAt: new Date().toISOString(), kinds, rawEdges: edges.length, meta, sources, sourceFingerprint: sha(JSON.stringify(sources)), graphFingerprint: sha(JSON.stringify({ nodes, edges })), views: views.map(v => ({ name: v.name, nodes: v.nodes.length, edges: v.edges.length, sha256: sha(JSON.stringify(v)) })) };
  return { views, manifest };
}
if (process.argv[1]?.endsWith('export.mjs')) {
  const result = snapshot(process.argv[2], process.argv[3]);
  writeFileSync(process.argv[4], JSON.stringify(result), { flag: 'wx' });
  console.log(JSON.stringify(result.manifest.views));
}
