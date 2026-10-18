import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import {pathToFileURL} from 'node:url';

const read = p => JSON.parse(fs.readFileSync(p, 'utf8').replace(/^\uFEFF/, ''));
const mean = a => a.reduce((s, x) => s + x, 0) / a.length;
const quantile = (a, p) => [...a].sort((a, b) => a - b)[Math.max(0, Math.ceil(a.length * p) - 1)];
function metric(hits, goldCount, threshold = 0) {
  const kept = hits.filter(h => h.score >= threshold);
  const rank = kept.slice(0, 10).findIndex(h => h.relevant.length);
  const gain = new Set();
  let dcg = 0;
  kept.slice(0, 10).forEach((h, i) => {
    if (h.relevant.some(g => !gain.has(g))) dcg += 1 / Math.log2(i + 2);
    h.relevant.forEach(g => gain.add(g));
  });
  const ideal = Array.from({length: Math.min(goldCount, 10)}, (_, i) => 1 / Math.log2(i + 2)).reduce((a, b) => a + b, 0);
  return {top1: Number(!!kept[0]?.relevant.length), hit3: Number(kept.slice(0, 3).some(h => h.relevant.length)),
    mrr10: rank < 0 ? 0 : 1 / (rank + 1), ndcg10: ideal ? dcg / ideal : 0,
    docTop1: Number(!!kept[0]?.document_relevant), docHit3: Number(kept.slice(0, 3).some(h => h.document_relevant)),
    docMrr10: (() => { const r = kept.slice(0, 10).findIndex(h => h.document_relevant); return r < 0 ? 0 : 1/(r+1); })(),
    falseNonempty: Number(kept.length > 0)};
}
function loadRun(dir, inputs) {
  const {config: cfg, manifest} = read(path.join(dir, 'config.json'));
  const monitorFile = `${dir}.monitor.json`;
  const monitor = fs.existsSync(monitorFile) ? read(monitorFile) : null;
  const gpuSamples=monitor?.samples.filter(s=>s.gpu?.length)??[];
  const complete = read(path.join(dir, 'complete.json'));
  const rows = fs.readFileSync(path.join(dir, 'rows.jsonl'), 'utf8').trim().split('\n').filter(Boolean).map(JSON.parse);
  const expected = inputs.filter(c => !cfg.ids.length || cfg.ids.includes(c.id));
  assert.equal(expected.length * cfg.repeats, complete.observations);
  assert.equal(rows.length, complete.observations, 'incomplete run');
  const keys = new Set();
  for (const row of rows) {
    const c = expected.find(c => c.id === row.id);
    assert(c && row.repeat >= 0 && row.repeat < cfg.repeats && Number.isInteger(row.repeat));
    assert(!keys.has(`${row.id}:${row.repeat}`), 'duplicate observation'); keys.add(`${row.id}:${row.repeat}`);
    assert.equal(row.hits.length, c.hits.length);
    assert.equal(row.logits.length, c.hits.length);
    assert.deepEqual([...row.hits.map(h => h.id)].sort((a,b)=>a-b), [...c.hits.map(h=>h.id)].sort((a,b)=>a-b));
    assert(row.seconds > 0 && Number.isFinite(row.seconds));
    assert(row.logits.every(Number.isFinite));
    assert(row.hits.every(h => Number.isFinite(h.score) && h.score >= 0 && h.score <= 1));
  }
  const first = rows.filter(r => r.repeat === 0);
  const steady = rows.filter(r => r.repeat > 0);
  const positives = first.filter(r => inputs.find(c => c.id === r.id).gold_count > 0);
  const negatives = first.filter(r => !inputs.find(c => c.id === r.id).gold_count);
  const quality = [0, .1].map(threshold => {
    const pos = positives.map(r => metric(r.hits, inputs.find(c => c.id === r.id).gold_count, threshold));
    return {threshold, positive: pos.length, negative: negatives.length,
      ...Object.fromEntries(['top1','hit3','mrr10','ndcg10','docTop1','docHit3','docMrr10'].map(k => [k, pos.length ? mean(pos.map(m=>m[k])) : null])),
      falseNonemptyCount: negatives.reduce((n,r)=>n+metric(r.hits,0,threshold).falseNonempty,0)};
  });
  return {dir, config:cfg, manifest, rows, first, quality,
    median:quantile(rows.map(r=>r.seconds),.5), p95:quantile(rows.map(r=>r.seconds),.95), n:rows.length,
    steady:steady.length?{n:steady.length,median:quantile(steady.map(r=>r.seconds),.5),p95:quantile(steady.map(r=>r.seconds),.95)}:null,
    paddingRatio:mean(rows.map(r=>r.padded/r.tokens)), session:read(path.join(dir,'session.json')),
    timingValid: monitor ? !monitor.timing_contaminated : null,
    memory: monitor ? {peakWorkingSet:Math.max(...monitor.samples.map(s=>s.peak_working_set)),sampledPeakPrivate:Math.max(...monitor.samples.map(s=>s.private_bytes)),
      peakDedicatedGpu:gpuSamples.length?Math.max(...gpuSamples.map(s=>s.gpu.reduce((n,g)=>n+g.DedicatedUsage,0))):null,
      peakSharedGpu:gpuSamples.length?Math.max(...gpuSamples.map(s=>s.gpu.reduce((n,g)=>n+g.SharedUsage,0))):null} : null};
}
function compare(base, variant) {
  const deltas = [], losses = [];
  for (const row of variant.rows ?? variant.first) {
    const old = base.rows?.find(r=>r.id===row.id && r.repeat===row.repeat) ?? base.first.find(r=>r.id===row.id); if (!old) continue;
    assert.equal(row.logits.length, old.logits.length);
    deltas.push(...row.logits.map((v,i)=>Math.abs(v-old.logits[i])));
    for (const threshold of [0,.1]) {
      const before = metric(old.hits,1,threshold), after=metric(row.hits,1,threshold);
      const lost = ['top1','hit3','mrr10','ndcg10','docTop1','docHit3','docMrr10'].filter(k=>after[k]<before[k]);
      if (lost.length) losses.push({id:row.id,repeat:row.repeat??0,threshold,metrics:lost});
    }
  }
  const paired=[];
  for(const id of new Set((variant.rows??variant.first).map(r=>r.id))) {
    const b=(base.rows??base.first).filter(r=>r.id===id).map(r=>r.seconds);
    const v=(variant.rows??variant.first).filter(r=>r.id===id).map(r=>r.seconds);
    if(b.every(Number.isFinite)&&v.every(Number.isFinite)&&b.length&&v.length)paired.push(quantile(b,.5)/quantile(v,.5));
  }
  return {baseline:base.config.name, medianSpeedup:base.median/variant.median,pairedMedianSpeedup:paired.length?quantile(paired,.5):null,
    maxLogitDelta:deltas.length?Math.max(...deltas):null, losses};
}
export {metric,quantile};
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){
if (process.argv[2] === '--self-test') {
  const hit=(id,relevant,score=.9)=>({id,relevant,score,document_relevant:!!relevant.length});
  assert.equal(metric([hit(1,[]),hit(2,[0])],1).mrr10,.5);
  assert.equal(metric([hit(1,[0],.01)],1,.1).hit3,0);
  assert.equal(metric([hit(1,[0]),hit(2,[0])],1).ndcg10,1);
  assert.equal(quantile([1,2,3,4],.95),4);
  assert.equal(compare({config:{name:'b'},median:2,first:[{id:'x',logits:[1],hits:[hit(1,[0])]}]},
    {median:1,first:[{id:'x',logits:[1],hits:[hit(1,[])]}]}).losses.length,2);
  console.log('inference analysis: all assertions passed');
} else {
  const [inputFile,output,...dirs]=process.argv.slice(2);
  assert(dirs.length, 'inputs.json summary.json run-dir [...]');
  const inputs=read(inputFile), runs=dirs.map(dir=>loadRun(dir,inputs));
  const result=runs.map(r=>{
    const base=runs.find(b=>b.config.name==='cpu-t16-b4') ?? runs[0];
    const {rows,first,...summary}=r;
    return {...summary,...compare(base,r)};
  });
  fs.writeFileSync(output,JSON.stringify({inputSha256:crypto.createHash('sha256').update(fs.readFileSync(inputFile)).digest('hex'),runs:result},null,2)+'\n');
  for(const r of result)console.log(`${r.config.name} n=${r.n} med=${r.median.toFixed(3)} p95=${r.p95.toFixed(3)} pad=${r.paddingRatio.toFixed(2)} logitDelta=${r.maxLogitDelta?.toExponential(2)} losses=${r.losses.length} timingValid=${r.timingValid}`);
}
}
