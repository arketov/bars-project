// Design probe only: saved snapshots, no database/model/product calls.
import { readFileSync, writeFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';

const input = readFileSync(new URL('../2026-10-21-fourth-edge/snapshots.json.gz', import.meta.url));
const snapshots = JSON.parse(gunzipSync(input));
const pair = (a, b) => JSON.stringify([a, b].sort());
const q = s => 0.05 + 0.45 * s;
const width = s => 0.6 + 1.8 * s;
const brightness = s => 0.25 + 0.65 * s;
const springs = values => {
  const raw = values.map(q), total = raw.reduce((a, b) => a + b, 0);
  const cap = Math.max(...raw);
  return raw.map(v => cap * v / total);
};
const close = (a, b) => assert.ok(Math.abs(a - b) < 1e-10, `${a} != ${b}`);
let monotonicCases = 0;
for (let i = 50; i < 100; i++) {
  const a = i / 100, b = (i + 1) / 100;
  assert.ok(width(b) > width(a) && brightness(b) > brightness(a));
  // Increasing one edge, fixed competing links and geometry.
  assert.ok(springs([b, .6, 1])[0] > springs([a, .6, 1])[0]);
  monotonicCases++;
}
for (const count of [1, 2, 50, 500]) {
  const coefficients = springs(Array(count).fill(.8));
  close(coefficients.reduce((a, b) => a + b, 0), q(.8));
}
close(springs([1, 1, 1]).reduce((a, b) => a + b, 0), q(1));

// Fractional coverage on Unicode characters, independent of UTF-8 byte length.
const coverageWeights = (text, spans) => {
  const chars = [...text], multiplicity = chars.map(() => 0);
  const masks = spans.map(ranges => chars.map((_, i) => ranges.some(([a, b]) => a <= i && i < b)));
  masks.forEach(mask => mask.forEach((covered, i) => { if (covered) multiplicity[i]++; }));
  return masks.map(mask => mask.reduce((sum, covered, i) => sum + (covered ? 1 / multiplicity[i] : 0), 0));
};
assert.deepEqual(coverageWeights('абвгде', [[[0, 4]], [[2, 6]]]), [3, 3]);
assert.deepEqual(coverageWeights('абвгде', [[[0, 6]], [[0, 6]]]), [3, 3]);
assert.deepEqual(coverageWeights('абвгде', [[[0, 6], [0, 6]]]), [6]);
const weights = coverageWeights('абвгде', [[[0, 2]], [[2, 6]]]);
assert.deepEqual(weights, [2, 4]);
const centroid = vectors => {
  const sum = [0, 0];
  vectors.forEach(([v, weight]) => {
    const norm = Math.hypot(...v);
    assert.ok(norm > 0 && v.every(Number.isFinite));
    v.forEach((value, i) => sum[i] += weight * value / norm);
  });
  const norm = Math.hypot(...sum);
  assert.ok(norm > 0);
  return sum.map(v => v / norm);
};
const original = centroid([[[1, 0], 6], [[0, 1], 2]]);
const repeated = centroid([[[1, 0], 3], [[1, 0], 3], [[0, 1], 2]]);
original.forEach((v, i) => close(v, repeated[i]));
assert.throws(() => centroid([[[0, 0], 1]]));

const inventory = snapshots.map(snapshot => {
  const nodes = new Map(snapshot.nodes.map(n => [n.reference.id, n]));
  const documents = snapshot.nodes.filter(n => n.reference.kind === 'document');
  const chunks = snapshot.nodes.filter(n => n.reference.kind === 'chunk');
  const owner = id => {
    const n = nodes.get(id);
    assert.ok(n, `missing endpoint ${id}`);
    const owners = n.reference.kind === 'document' ? [id] : n.source_ids.filter(x => nodes.get(x)?.reference.kind === 'document');
    assert.equal(owners.length, 1);
    return owners[0];
  };
  chunks.forEach(n => assert.equal(owner(n.reference.id), n.parent));
  const links = new Map(), similar = new Set(), kinds = new Map();
  const mark = (key, kind) => {
    if (!kinds.has(key)) kinds.set(key, new Set());
    kinds.get(key).add(kind);
  };
  for (const e of snapshot.edges) {
    if (e.predicate === 'links_to') {
      const a = owner(e.from), b = owner(e.to);
      if (a === b) continue;
      const directed = JSON.stringify([a, b]);
      links.set(directed, (links.get(directed) ?? 0) + 1);
      mark(pair(a, b), 'markdown_link');
    }
    if (e.predicate === 'similar_to') {
      assert.equal(nodes.get(e.from)?.reference.kind, 'chunk');
      assert.equal(nodes.get(e.to)?.reference.kind, 'chunk');
      assert.ok(Number.isFinite(e.score) && e.score >= -1 && e.score <= 1);
      if (e.from !== e.to) similar.add(pair(e.from, e.to));
      const a = owner(e.from), b = owner(e.to);
      if (a !== b) mark(pair(a, b), 'chunk_similarity');
    }
  }
  const reciprocal = [...links.keys()].filter(key => {
    const [a, b] = JSON.parse(key);
    return links.has(JSON.stringify([b, a]));
  }).length / 2;
  return {
    name: snapshot.name, state: snapshot.meta.find(x => x.key === 'state')?.value,
    documents: documents.length, chunks: chunks.length,
    allNodes: documents.length + chunks.length,
    directedDocumentLinkPairs: links.size, reciprocalDocumentPairs: reciprocal,
    repeatedLinkPairs: [...links.values()].filter(n => n > 1).length,
    undirectedChunkSimilarityPairs: similar.size,
    ownerPairsWithLinksAndChunkSimilarity: [...kinds.values()].filter(s => s.size === 2).length,
    defaultNodeLimitFitsDocuments: documents.length <= 500,
    maxNodeLimitFitsAll: documents.length + chunks.length <= 2000,
  };
});
const result = {
  inputSha256: createHash('sha256').update(input).digest('hex'),
  scope: 'design formulas and saved partial graph inventory; no vectors, extraction quality, product UI or native acceptance',
  checks: { monotonicCases, groupSizes: [1, 2, 50, 500], coverageAndCentroid: 'passed' },
  inventory,
};
writeFileSync(new URL('./verification.json', import.meta.url), JSON.stringify(result, null, 2) + '\n');
console.log(JSON.stringify(result, null, 2));
