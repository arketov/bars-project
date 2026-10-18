import fs from 'node:fs';
import assert from 'node:assert/strict';
import {quantile} from './analyze-inference.mjs';
const [out,...dirs]=process.argv.slice(2);
const read=p=>JSON.parse(fs.readFileSync(p,'utf8').replace(/^\uFEFF/,''));
const cosine=(a,b)=>{assert.equal(a.length,b.length);let dot=0,aa=0,bb=0;for(let i=0;i<a.length;i++){assert(Number.isFinite(a[i])&&Number.isFinite(b[i]));dot+=a[i]*b[i];aa+=a[i]*a[i];bb+=b[i]*b[i];}return dot/Math.sqrt(aa*bb)};
const loaded=dirs.map(dir=>{const summary=read(`${dir}/summary.json`),monitor=read(`${dir}.monitor.json`);const rows=fs.readFileSync(`${dir}/rows.jsonl`,'utf8').trim().split(/\r?\n/).map(JSON.parse);return {dir,summary,monitor,rows,vectors:read(`${dir}/document-vectors-0.json`)}});
const base=loaded[0];
const runs=loaded.map(({dir,summary,monitor,rows,vectors})=>{
  assert.deepEqual(summary.document_ids,base.summary.document_ids);
  assert.equal(vectors.length,summary.document_ids.length);
  const docCos=vectors.map((v,i)=>cosine(base.vectors[i],v));
  const queryCos=rows.map(r=>{const before=base.rows.find(b=>b.id===r.id&&b.repeat===r.repeat);assert(before);return cosine(before.vector,r.vector)});
  return {dir,config:summary.config,n:rows.length,queryMedian:quantile(rows.map(r=>r.seconds),.5),queryP95:quantile(rows.map(r=>r.seconds),.95),load:summary.load,
    documentPasses:summary.indexing,documentBytes:summary.document_bytes,
    minQueryCosine:Math.min(...queryCos),minDocumentCosine:Math.min(...docCos),
    timingValid:monitor.exit_code===0&&!monitor.timing_contaminated,
    peakPrivate:Math.max(...monitor.samples.map(s=>s.private_bytes)),peakWorkingSet:Math.max(...monitor.samples.map(s=>s.peak_working_set)),
    limitation:'64 fixed length-stratified documents in hash order; model throughput, not full indexing or retrieval quality'};
});
fs.writeFileSync(out,JSON.stringify({baseline:base.summary.config.name,runs},null,2));
for(const r of runs)console.log(`${r.config.name} query=${r.queryMedian.toFixed(3)}s docs=${r.documentPasses[0].texts_per_second.toFixed(2)}/s cos=${r.minDocumentCosine.toFixed(6)} timingValid=${r.timingValid}`);
