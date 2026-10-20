import assert from 'node:assert/strict';
import { readFileSync, existsSync, writeFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import { createHash } from 'node:crypto';
import { resolve, join } from 'node:path';
const out = resolve(process.argv[2]);
const read = name => JSON.parse(readFileSync(join(out,name)));
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const captured = JSON.parse(gunzipSync(readFileSync(join(out,'snapshots.json.gz'))));
const report = read('measurement.json');
assert.equal(report.success, true);
assert.equal(report.physics.length, 36); assert.equal(report.render.length, 72);
assert.deepEqual(report.checks.pageErrors, []);
assert.deepEqual(captured.map(c=>c.manifest), read('manifests.json'));
for (const c of captured) for (const g of c.views) {
  const entry = c.manifest.views.find(x=>x.name===g.name);
  assert.equal(hash(JSON.stringify(g)),entry.sha256);
  const ids = new Set(g.nodes.map(n=>n.id)); assert.equal(ids.size,g.nodes.length);
  assert.equal(entry.nodes,g.nodes.length); assert.equal(entry.edges,g.edges.length);
  assert(g.edges.every(e=>ids.has(e.source)&&ids.has(e.target)&&e.source!==e.target&&Number.isFinite(e.weight)&&e.weight>=0&&e.weight<=1));
  assert.equal(report.physics.filter(x=>x.graph===g.name).length,9);
  assert.equal(report.render.filter(x=>x.graph===g.name).length,18);
  const check = report.checks[g.name]; assert.equal(check.late,0); assert.equal(check.updates,3); assert(check.heartbeat>2);
}
assert(report.physics.every(x=>x.finite&&x.elapsedMs>0));
assert(report.render.every(x=>x.remainingCanvas===0&&x.firstFrameMs>0&&x.frameP95Ms>0));
for (const distances of Object.values(report.checks.weights)) assert(distances[1]<distances[0]);
for (const c of captured) { const check = read(`${c.manifest.name}-pin-stop.json`); assert(check.pinned&&check.stopped&&check.released); }
assert(read('tuning.json').results.every(x=>x.finite&&x.elapsedMs>0));
const captures = read('captures.json'); assert.equal(captures.length,8); assert(captures.every(name=>existsSync(join(out,name))));
const summary = { success:true, physics:36, render:72, projections:4, captures:8, snapshotHashesVerified:true, danglingEndpoints:0, mainThreadHeartbeat:true, lateAfterStop:0, remainingCanvas:0, sourceTest:'node --test export.test.mjs', limits:'Research only; no product/native, memory leak, physical interaction or semantic-quality acceptance' };
writeFileSync(join(out,'verification.json'),JSON.stringify(summary,null,2)+'\n');
console.log(JSON.stringify(summary));
