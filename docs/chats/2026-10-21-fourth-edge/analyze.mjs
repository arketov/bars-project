// Research only. Node 24; read-only SQLite, no model calls or product imports.
import { DatabaseSync } from 'node:sqlite';
import { readFileSync, writeFileSync } from 'node:fs';
import { gzipSync, gunzipSync } from 'node:zlib';
import { createHash } from 'node:crypto';
import { resolve, join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

const here = dirname(fileURLToPath(import.meta.url));
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const semantics = new Set(['depends_on', 'uses', 'implements', 'verifies', 'supersedes', 'requires']);
const pairKey = (a, b) => JSON.stringify([a, b].sort());
function capture(root, name) {
  const db = new DatabaseSync(join(root, '.bars/index.sqlite'), { readOnly: true });
  try {
    db.exec('BEGIN');
    const nodes = db.prepare('SELECT data FROM graph_nodes ORDER BY id').all().map(r => JSON.parse(r.data));
    const edges = db.prepare('SELECT data FROM graph_edges ORDER BY id').all().map(r => JSON.parse(r.data));
    const meta = db.prepare('SELECT key,value FROM graph_meta ORDER BY key').all();
    const sources = db.prepare('SELECT path,revision FROM graph_sources ORDER BY path').all();
    db.exec('COMMIT');
    // Preserve identities/provenance for a repeat; no source texts or embeddings.
    return { name, capturedAt: new Date().toISOString(), nodes, edges, meta, sources };
  } finally { db.close(); }
}
function analyze(data) {
  const nodes = new Map(data.nodes.map(n => [n.reference.id, n]));
  const kind = id => nodes.get(id)?.reference.kind;
  const documents = id => kind(id) === 'document' ? [id] : (nodes.get(id)?.source_ids ?? []).filter(x => kind(x) === 'document');
  const chunks = data.nodes.filter(n => n.reference.kind === 'chunk');
  const ownership = data.edges.filter(e => e.predicate === 'contains' && kind(e.from) === 'document' && kind(e.to) === 'chunk');
  const ownerPairs = new Set(ownership.map(e => pairKey(e.from, e.to)));
  const wrongOwner = ownership.filter(e => nodes.get(e.to).parent !== e.from || !nodes.get(e.to).source_ids.includes(e.from));
  const ownerCounts = new Map();
  for (const e of ownership) {
    if (!ownerCounts.has(e.to)) ownerCounts.set(e.to, new Set());
    ownerCounts.get(e.to).add(e.from);
  }
  const completeOwnership = chunks.every(n => ownerCounts.get(n.reference.id)?.size === 1);
  assert.equal(wrongOwner.length, 0, 'contains must agree with source ownership');
  assert.equal(completeOwnership, true, 'each indexed chunk must have exactly one document owner');
  const links = new Set();
  const incoming = new Map();
  for (const e of data.edges.filter(e => e.predicate === 'links_to')) {
    for (const a of documents(e.from)) for (const b of documents(e.to)) {
      if (a === b) continue;
      links.add(pairKey(a, b));
      if (!incoming.has(b)) incoming.set(b, new Set());
      incoming.get(b).add(a);
    }
  }
  function shared(groups) {
    const pairs = new Map();
    let generated = 0;
    for (const [via, ids] of groups) {
      const list = [...ids].sort();
      for (let i = 0; i < list.length; i++) for (let j = i + 1; j < list.length; j++) {
        generated++;
        const key = pairKey(list[i], list[j]);
        if (!pairs.has(key)) pairs.set(key, []);
        pairs.get(key).push(via);
      }
    }
    const sorted = [...pairs].sort((a, b) => b[1].length - a[1].length || a[0].localeCompare(b[0]));
    return { generated, pairs: pairs.size, alreadyDirect: [...pairs.keys()].filter(k => links.has(k)).length,
      maxSupport: sorted[0]?.[1].length ?? 0,
      samples: sorted.slice(0, 4).map(([key, via]) => ({ documents: JSON.parse(key).map(id => nodes.get(id).label), via: via.slice(0, 5).map(id => nodes.get(id)?.label ?? id), support: via.length })) };
  }
  const entityDocs = new Map(data.nodes.filter(n => n.reference.kind === 'entity').map(n => [n.reference.id, new Set(documents(n.reference.id))]));
  const semantic = data.edges.filter(e => semantics.has(e.predicate));
  const crossFile = semantic.filter(e => documents(e.from).some(a => documents(e.to).some(b => a !== b)));
  return { name: data.name, capturedAt: data.capturedAt, meta: data.meta,
    sources: data.sources.length,
    kinds: Object.fromEntries([...new Set(data.nodes.map(n => n.reference.kind))].map(k => [k, data.nodes.filter(n => n.reference.kind === k).length])),
    predicates: Object.fromEntries([...new Set(data.edges.map(e => e.predicate))].sort().map(k => [k, data.edges.filter(e => e.predicate === k).length])),
    ownership: { rawEdges: ownership.length, uniquePairs: ownerPairs.size, chunks: chunks.length, completeOwnership, wrongOwner: wrongOwner.length },
    directDocumentPairs: links.size,
    commonTargets: { ...shared(incoming), largestHub: Math.max(0, ...[...incoming.values()].map(x => x.size)) },
    sharedEntities: { ...shared(entityDocs), crossFileEntities: [...entityDocs.values()].filter(x => x.size > 1).length },
    semantic: { edges: semantic.length, crossFile: crossFile.length, samples: semantic.slice(0, 4).map(e => ({ subject: nodes.get(e.from)?.label, predicate: e.predicate, object: nodes.get(e.to)?.label, polarity: e.polarity, qualifiers: e.qualifiers, origin: e.origin, evidence: e.evidence })) } };
}

const snapshotPath = join(here, 'snapshots.json.gz');
if (process.argv.includes('--capture')) {
  const inputs = [capture(resolve('.'), 'bars-project'), capture(resolve('../cyberProject/ai-mutiplayer-bots'), 'overdrive-bots')];
  writeFileSync(snapshotPath, gzipSync(JSON.stringify(inputs)), { flag: 'wx' });
}
const bytes = readFileSync(snapshotPath);
const results = JSON.parse(gunzipSync(bytes)).map(analyze);
const output = { snapshotSha256: sha(bytes), method: 'read-only existing graph; inventory, not extraction precision or UI acceptance', results };
writeFileSync(join(here, 'results.json'), JSON.stringify(output, null, 2) + '\n');
console.log(JSON.stringify(results.map(({ name, kinds, ownership, directDocumentPairs, commonTargets, sharedEntities, semantic }) => ({ name, kinds, ownership, directDocumentPairs, commonTargets, sharedEntities, semantic })), null, 2));
