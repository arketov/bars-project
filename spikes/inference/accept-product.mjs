import fs from 'node:fs';
import crypto from 'node:crypto';
import zlib from 'node:zlib';
import assert from 'node:assert/strict';
import {metric} from '../../scripts/analyze-inference.mjs';
const dest='docs/knowledge/inference-2026-10-18';
const runPrefix=process.argv[2]??'product2';
const read=p=>JSON.parse(fs.readFileSync(p,'utf8').replace(/^\uFEFF/,''));
const rows=p=>fs.readFileSync(p,'utf8').trim().split(/\r?\n/).map(JSON.parse);
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const keys=['top1','hit3','mrr10','ndcg10','docTop1','docHit3','docMrr10'];
function compare(before,after,negative){
  for(const key of keys)assert(after[key]>=before[key],`quality loss ${key}`);
  if(negative)assert(after.falseNonempty<=before.falseNonempty,'new false nonempty');
}
function metadata(dir){
  const monitor=read(`${dir}.monitor.json`),complete=read(`${dir}/complete.json`);
  assert.equal(monitor.exit_code,0);assert(monitor.quality_only);
  const stderr=fs.readFileSync(`${dir}.stderr.log`,'utf8');
  assert(stderr.includes('загружен на DirectMl; precision=fp16; batch=16'));
  assert(!stderr.includes('GPU unavailable'));
  return {complete,monitor,rowsSha256:hash(`${dir}/rows.jsonl`),loadLog:stderr.split(/\r?\n/).filter(s=>s.includes('загружен'))};
}
const core=[],evidence=[];
for(const [dataset,n] of [['overdrive',100],['bars',24],['holdout',14]]){
  const dir=`spikes/inference/runs/${runPrefix}-${dataset}/production`;
  const input=read(`spikes/inference/data/${dataset}/inputs.json`);
  const baseline=rows(`spikes/inference/runs/full-${dataset}/production/rows.jsonl`);
  const actual=rows(`${dir}/rows.jsonl`);assert.equal(actual.length,n);
  const seen=new Set();
  for(const row of actual){
    assert(!seen.has(row.id));seen.add(row.id);assert.equal(row.repeat,0);
    const old=baseline.find(r=>r.id===row.id),c=input.find(c=>c.id===row.id);assert(old&&c);
    const metrics=[];
    for(const threshold of [0,.1]){
      const a=metric(old.hits,c.gold_count,threshold),b=metric(row.hits,c.gold_count,threshold);
      compare(a,b,!c.gold_count);metrics.push({threshold,...b});
    }
    evidence.push({kind:'core',dataset,...row,metrics});
  }
  const meta=metadata(dir),session=read(`${dir}/session.json`);assert.equal(session.rerank_device,'DirectMl');
  core.push({dataset,n,losses:0,session,...meta});
}
const baseline=zlib.gunzipSync(fs.readFileSync(`${dest}/worker-rows.jsonl.gz`)).toString().trim().split('\n').map(JSON.parse).filter(r=>r.run==='production');
const dir=`spikes/inference/runs/${runPrefix}-worker/production`,actual=rows(`${dir}/rows.jsonl`);
assert.equal(actual.length,22);const cases=read('spikes/inference/data/overdrive/cases.json');
const counts={serial:8,'concurrent-2':2,'concurrent-4':4,'during-index':8};
for(const [mode,n] of Object.entries(counts))assert.equal(actual.filter(r=>r.mode===mode).length,n);
const seenWorker=new Set();
for(const row of actual){
  assert.equal(row.response.applied.score_kind,'rerank');assert(!row.response.rerank_error);
  if(row.mode==='during-index')assert(row.active_at_start);
  const old=baseline.find(r=>r.mode===row.mode&&r.query===row.query),c=cases.find(c=>c.query===row.query);assert(old&&c);
  const key=`${row.mode}:${c.id}`;assert(!seenWorker.has(key),'duplicate worker case');seenWorker.add(key);
  assert(row.response.results.every(h=>h.score_kind==='rerank'&&Number.isFinite(h.score)));
  const hits=row.response.results.map(h=>({...h,relevant:c.gold.flatMap((g,i)=>g.path===h.path&&h.text.includes(g.evidence)?[i]:[]),document_relevant:c.gold.some(g=>g.path===h.path)}));
  const metrics=[0,.1].map(threshold=>({threshold,...metric(hits,c.gold.length,threshold)}));
  metrics.forEach((m,i)=>compare(old.metrics[i],m,!c.gold.length));
  const response={...row.response,results:row.response.results.map(({text,...rest})=>({...rest,text_sha256:crypto.createHash('sha256').update(text).digest('hex')}))};
  evidence.push({kind:'worker',...row,response,case_id:c.id,metrics});
}
const sourceFiles=['Cargo.toml','Cargo.lock','crates/bars-core/Cargo.toml','crates/bars-core/src/models/builtin_rerank.rs','crates/bars-core/src/models/rerank_artifact.rs','crates/bars-core/src/models/mod.rs','crates/bars-worker/src/worker/models.rs','crates/bars-worker/src/handler/mod.rs'];
const binaries=['bench_inference.exe','bench_worker_inference.exe'].map(name=>`${process.env.LOCALAPPDATA}/bars/target/release/examples/${name}`);
const result={kind:'product-loader-acceptance',runPrefix,limitation:'quality-only; timings excluded from speed conclusion; cached artifacts, not fresh network download acceptance; allocation backoff unit-tested, not real small-VRAM hardware acceptance; shared source drift recorded separately, results apply to built snapshot',core,worker:{n:22,losses:0,...metadata(dir)},currentFileHashes:Object.fromEntries([...sourceFiles,...binaries].map(p=>[p,hash(p)])),buildSources:read('spikes/inference/provenance/product-build-sources.json'),sourceDrift:read('spikes/inference/provenance/product-source-drift.json')};
fs.writeFileSync(`${dest}/product-acceptance.json.gz`,zlib.gzipSync(JSON.stringify(result)));
fs.writeFileSync(`${dest}/product-rows.jsonl.gz`,zlib.gzipSync(evidence.map(r=>JSON.stringify(r)).join('\n')+'\n'));
console.log('Product FP16 loader: 138 full core + 22 real IPC responses; no metric losses, no CPU fallback; document bodies redacted');
