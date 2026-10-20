import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import assert from 'node:assert/strict';
import {metric} from './analyze-inference.mjs';
const root=process.argv[2]??'docs/knowledge/memory-acceptance-2026-10-20';
const unpack=p=>zlib.gunzipSync(fs.readFileSync(p)).toString('utf8').replace(/^\uFEFF/,'');
const read=name=>JSON.parse(unpack(path.join(root,name+'.gz')));
const rows=name=>unpack(path.join(root,name+'.gz')).trim().split(/\r?\n/).map(JSON.parse);
const baseline=rows('../inference-2026-10-18/product-rows.jsonl').filter(r=>r.kind==='core');
const proof=JSON.parse(unpack('docs/knowledge/inference-2026-10-18/product-acceptance.json.gz'));
assert.equal(baseline.length,138);assert.equal(proof.core.length,3);
assert(proof.core.every(c=>c.monitor.exit_code===0&&!c.monitor.quality_rejected));
const metrics=['top1','hit3','mrr10','ndcg10','docTop1','docHit3','docMrr10'];
const results=[];
for(const corpus of ['bars','overdrive']){
  const index=read(`index-${corpus}-core-fp16-complete.json`);
  const indexMonitor=read(`index-${corpus}-core-fp16.monitor.json`);
  const indexEnd=rows(`index-${corpus}-core-fp16-native-memory.jsonl`).at(-1);
  assert.equal(indexMonitor.exit_code,0);assert(!indexMonitor.quality_rejected);
  assert.equal(indexEnd.kind,'end');assert.equal(indexEnd.detail.ok,true);
  assert(index.serial_documents);assert.equal(index.identity.precision,'fp16');
  assert(/direct.?ml/i.test(index.identity.actual));
  assert(index.identity.fingerprint.model.includes('@fp16:4de13258303883538bd53b696b452bf8099f0858'));
  const full=index.rows.find(r=>r.mode==='full');assert(full&&full.chunks>0);
  assert.equal(full.chunks,corpus==='bars'?482:946);
  const inputs=read(`data-${corpus}-inputs.json`);
  assert.equal(inputs.length,corpus==='bars'?24:114);
  const fresh=rows(`quality-${corpus}-production-rows.jsonl`);
  const monitor=read(`quality-${corpus}-production.monitor.json`);
  const complete=read(`quality-${corpus}-production-complete.json`);
  const session=read(`quality-${corpus}-production-session.json`);
  const qualityEnd=rows(`quality-${corpus}-production-native-memory.jsonl`).at(-1);
  assert.equal(qualityEnd.kind,'end');assert.equal(qualityEnd.detail.ok,true);
  assert(complete.rows_blake3);assert.equal(monitor.exit_code,0);assert(!monitor.quality_rejected);
  assert(monitor.quality_only&&monitor.timing_contaminated);
  assert.equal(session.embedding_runtime.precision,'fp16');assert(/direct.?ml/i.test(session.rerank_device));
  assert(session.staged_quality);assert(/direct.?ml/i.test(session.embedding_runtime.actual));
  assert(!session.embedding_runtime.fallback_reason);
  assert.equal(fresh.length,inputs.length);assert.equal(new Set(fresh.map(r=>r.id)).size,inputs.length);
  for(const dataset of [...new Set(inputs.map(c=>c.dataset))]){
    const selected=inputs.filter(c=>c.dataset===dataset),losses=[],scores=[];
    for(const input of selected){
      const before=baseline.filter(r=>r.dataset===dataset&&r.id===input.id);
      const after=fresh.filter(r=>r.id===input.id);assert.equal(before.length,1);assert.equal(after.length,1);
      assert.equal(after[0].repeat,0);assert(after[0].hits.every(h=>Number.isFinite(h.score)&&h.score>=0&&h.score<=1));
      for(const threshold of [0,.1]){
        const old=metric(before[0].hits,input.gold_count,threshold),now=metric(after[0].hits,input.gold_count,threshold);
        const lost=metrics.filter(k=>now[k]<old[k]-1e-12);
        if(!input.gold_count&&now.falseNonempty>old.falseNonempty)lost.push('falseNonempty');
        if(lost.length)losses.push({id:input.id,threshold,metrics:lost});
        scores.push({id:input.id,threshold,before:old,after:now});
      }
    }
    results.push({dataset,questions:selected.length,fresh_index_chunks:full.chunks,losses,scores});
  }
}
const result={kind:'fresh-fp16-index-quality',timing_valid:false,reference:'archived product FP32 embed + FP16 rerank',candidate_delta_is_stage9_comparison:false,metric_roundoff_tolerance:1e-12,accepted:results.every(r=>!r.losses.length),results};
fs.writeFileSync(path.join(root,'precision-summary.json'),JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify({accepted:result.accepted,datasets:results.map(({dataset,questions,losses})=>({dataset,questions,losses:losses.length}))}));
assert(result.accepted,'fresh FP16 retrieval lost a selected quality metric');
