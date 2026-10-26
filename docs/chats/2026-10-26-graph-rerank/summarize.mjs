import fs from 'node:fs';
import zlib from 'node:zlib';
import assert from 'node:assert/strict';
const [analysisFile, rawFile, output] = process.argv.slice(2);
const a = JSON.parse(zlib.gunzipSync(fs.readFileSync(analysisFile)));
const raw = JSON.parse(fs.readFileSync(rawFile));
assert.equal(raw.rows.length, 36);
for (const r of raw.rows) {
  assert.equal(r.variants.length, 10);
  for (const v of r.variants.filter(v => v.name.endsWith('_rerank'))) {
    assert.equal(v.score_kind, 'Rerank');
    assert.equal(v.rerank_error, null);
  }
}
const names = Object.keys(a.rows[0].variants).filter(n => n !== 'graph');
const summaries = [];
const percentile = (xs, q) => xs.sort((a, b) => a - b)[Math.ceil(q * xs.length) - 1];
for (const split of ['all', 'dev', 'holdout']) for (const scenario of ['all', ...Array.from({length:8}, (_, i) => `S${i+1}`)]) {
  const rows = a.rows.filter(r => (split === 'all' || r.split === split) && (scenario === 'all' || r.scenario === scenario));
  for (const name of names) {
    const vs = rows.map(r => r.variants[name]);
    const total = key => vs.reduce((n, v) => n + v[key], 0);
    summaries.push({split, scenario, name, expected:total('expected'), found:total('found'), complete:vs.filter(v=>v.complete === true).length, bytes:total('bytes'), packets:vs.reduce((n,v)=>n+v.packets.length,0)});
  }
}
const pairs = [];
for (const [left, right] of [['hybrid', 'mix'], ['hybrid_rerank', 'mix_rerank'], ['hybrid_ui_rerank', 'mix_ui_rerank'], ['hybrid_wide_rerank', 'mix_rerank']]) {
  for (const suffix of ['', '_hits']) {
    const changes = a.rows.map(r => ({id:r.id, scenario:r.scenario, left:r.variants[left+suffix].found, right:r.variants[right+suffix].found}));
    pairs.push({left:left+suffix, right:right+suffix, wins:changes.filter(c=>c.right>c.left), losses:changes.filter(c=>c.right<c.left), ties:changes.filter(c=>c.right===c.left).length});
  }
}
const controls = a.rows.filter(r=>!/^S[1-8]$/.test(r.scenario));
const hit3 = Object.fromEntries(raw.rows[0].variants.map(v=>[v.name, {found:controls.filter(r=>r.hit3[v.name]).length,total:controls.length}]));
const timings = Object.fromEntries(raw.rows[0].variants.map(v => {
  const xs = raw.rows.map(r=>r.variants.find(x=>x.name===v.name).elapsed_ms);
  return [v.name, {p50:percentile([...xs], .5),p95:percentile([...xs], .95)}];
}));
const result = {snapshot:a.snapshot, device:raw.rerank_device, summaries, pairs, hit3, timings};
fs.writeFileSync(output, JSON.stringify(result, null, 2), {flag:'wx'});
console.log(JSON.stringify({device:result.device, totals:summaries.filter(s=>s.scenario==='all'&&s.split!=='dev'&&!s.name.endsWith('_hits')), pairs, hit3, timings}, null, 2));
