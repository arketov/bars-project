import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {metrics} from '../../scripts/analyze-quality.mjs';
import {read,write,sha} from './freeze.mjs';

export function validateRows(inputs,rows) {
  assert.equal(rows.length,inputs.length,'Complete case count');
  const byId=new Map();
  for(const row of rows) {
    assert(!byId.has(row.id),'Duplicate case');
    const input=inputs.find(i=>i.id===row.id);
    assert(input,'Unknown case');
    assert.equal(row.scores.length,input.documents.length,'Exact score count');
    assert(row.scores.every(s=>Number.isFinite(s)&&s>=0&&s<=1),'Finite normalized scores');
    byId.set(row.id,row);
  }
  return byId;
}
export function summarize(inputs,rows,threshold) {
  let positive=0,negative=0,loss=0,falseNonempty=0;
  const totals={top1:0,hit3:0,hit5:0,mrr10:0,ndcg10:0};
  for(const input of inputs) {
    const scores=rows.get(input.id).scores;
    const ranked=input.hits.map((h,i)=>({...h,score:scores[i]})).sort((a,b)=>b.score-a.score);
    const kept=ranked.filter(h=>h.score>=threshold);
    if(!input.gold.length){negative++;falseNonempty+=Number(kept.length>0);continue;}
    positive++;
    if(ranked.slice(0,3).some(h=>h.relevant.length)&&!kept.slice(0,3).some(h=>h.relevant.length))loss++;
    const m=metrics(kept,input.gold.length);
    for(const key of Object.keys(totals))totals[key]+=m[key];
  }
  return {positive,negative,...Object.fromEntries(Object.entries(totals).map(([k,v])=>[k,positive?v/positive:null])),lost_top3:loss,false_nonempty:falseNonempty};
}
export function registryDiagnostics(inputs,rows,threshold) {
  const cases=inputs.map(input=>{
    const [head,tail,filler]=rows.get(input.id).scores;
    return {id:input.id,head,tail,filler,head_kept:head>=threshold,tail_kept:tail>=threshold,filler_kept:filler>=threshold,
      tail_above_filler:tail>filler,head_above_filler:head>filler,tail_minus_head:tail-head};
  });
  const count=key=>cases.filter(c=>c[key]).length;
  return {cases,head_kept:count('head_kept'),tail_kept:count('tail_kept'),filler_kept:count('filler_kept'),tail_above_filler:count('tail_above_filler')};
}
export function analyze(out,names) {
  const inputs=read(path.join(out,'inputs.json')),protocol=read(path.join(out,'protocol.json'));
  assert.equal(sha(fs.readFileSync(path.join(out,'inputs.json'))),protocol.inputs_sha256);
  const summaries=[];
  const reference=read(path.join(out,'historical-baseline.json'));
  const historicalInputs=inputs.filter(i=>i.suite==='retrieval');
  const datasets=[{name:'historical-bge',inputs:historicalInputs,rows:validateRows(historicalInputs,reference)}];
  for(const name of names) {
    const events=fs.readFileSync(path.join(out,name+'.jsonl'),'utf8').trim().split('\n').map(JSON.parse);
    assert.equal(events.filter(e=>e.event==='complete').length,1,`Complete run ${name}`);
    assert(!events.some(e=>e.event==='error'),`No inference errors ${name}`);
    assert.equal(events.find(e=>e.event==='start')?.inputs_sha256,protocol.inputs_sha256,'Same input bytes');
    const rows=validateRows(inputs,events.filter(e=>e.event==='row'));
    datasets.push({name,inputs,rows});
  }
  for(const dataset of datasets) {
    for(const suite of ['retrieval','controlled-registry']) {
      const subset=dataset.inputs.filter(i=>i.suite===suite);
      if(!subset.length)continue;
      const tune=subset.filter(i=>i.split==='tune');
      const grid=protocol.thresholds.map(threshold=>({threshold,...summarize(tune,dataset.rows,threshold)}));
      const eligible=grid.filter(s=>s.lost_top3===0).sort((a,b)=>a.false_nonempty-b.false_nonempty||a.threshold-b.threshold);
      assert(eligible.length);
      const selected=eligible[0].threshold;
      // Threshold selection uses tune only. The test rows do not affect this value.
      const entry={model:dataset.name,suite,selected_threshold:selected,tune:grid};
      entry.test=protocol.thresholds.map(threshold=>({threshold,...summarize(subset.filter(i=>i.split==='test'),dataset.rows,threshold)}));
      entry.test_at_selected=entry.test.find(r=>r.threshold===selected);
      if(suite==='controlled-registry')entry.tail_diagnostics=Object.fromEntries(['tune','test'].map(split=>[split,
        protocol.thresholds.map(threshold=>({threshold,...registryDiagnostics(subset.filter(i=>i.split===split),dataset.rows,threshold)}))]));
      summaries.push(entry);
    }
  }
  write(path.join(out,'summary.json'),{inputs_sha256:protocol.inputs_sha256,models:names,summaries});
  console.log(JSON.stringify(summaries.filter(s=>s.suite==='retrieval').map(s=>({model:s.model,threshold:s.selected_threshold,...s.test_at_selected})),null,2));
}
if(process.argv[1]&&path.resolve(process.argv[1])===path.resolve(import.meta.filename)) {
  const [out,...names]=process.argv.slice(2);assert(out&&names.length);analyze(out,names);
}
