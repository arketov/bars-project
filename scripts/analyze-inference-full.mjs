import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {metric,quantile} from './analyze-inference.mjs';
const read=p=>JSON.parse(fs.readFileSync(p,'utf8').replace(/^\uFEFF/,''));
const [data,out,...dirs]=process.argv.slice(2);
assert(dirs.length,'data-dir summary.json full-run-dir [...]');
const inputs=read(path.join(data,'inputs.json'));
const mean=a=>a.reduce((n,x)=>n+x,0)/a.length;
const runs=dirs.map(dir=>{
  const config=read(path.join(dir,'config.json'));
  const complete=read(path.join(dir,'complete.json'));
  assert(complete.rows_blake3);
  const rows=fs.readFileSync(path.join(dir,'rows.jsonl'),'utf8').trim().split(/\r?\n/).map(JSON.parse);
  const expected=inputs.filter(c=>!config.ids.length||config.ids.includes(c.id));
  assert.equal(rows.length,expected.length*config.repeats,'incomplete full run');
  const keys=new Set();
  for(const row of rows){
    assert(expected.some(c=>c.id===row.id));
    assert(Number.isInteger(row.repeat)&&row.repeat>=0&&row.repeat<config.repeats);
    const key=`${row.id}:${row.repeat}`;assert(!keys.has(key));keys.add(key);
    assert(row.seconds>0&&Number.isFinite(row.seconds));
    assert(row.hits.every(h=>Number.isFinite(h.score)&&h.score>=0&&h.score<=1));
  }
  const quality=[];
  for(const split of ['all',...new Set(expected.map(c=>c.split))])for(const threshold of [0,.1]){
    const first=rows.filter(r=>r.repeat===0&&(split==='all'||expected.find(c=>c.id===r.id).split===split));
    const positive=first.filter(r=>expected.find(c=>c.id===r.id).gold_count);
    const negative=first.filter(r=>!expected.find(c=>c.id===r.id).gold_count);
    const metrics=positive.map(r=>metric(r.hits,expected.find(c=>c.id===r.id).gold_count,threshold));
    quality.push({split,threshold,positive:positive.length,negative:negative.length,
      ...Object.fromEntries(['top1','hit3','mrr10','ndcg10','docTop1','docHit3','docMrr10'].map(k=>[k,metrics.length?mean(metrics.map(m=>m[k])):null])),
      falseNonemptyCount:negative.reduce((n,r)=>n+metric(r.hits,0,threshold).falseNonempty,0)});
  }
  const monitor=read(`${dir}.monitor.json`);
  assert.equal(monitor.exit_code,0,'process failed; completed rows alone are not acceptance');
  assert(!monitor.quality_rejected,'quality-rejected run');
  assert(!fs.existsSync(path.join(dir,'invalidated.json')),'invalidated run');
  return {dir,config,n:rows.length,median:quantile(rows.map(r=>r.seconds),.5),p95:quantile(rows.map(r=>r.seconds),.95),
    embedMedian:quantile(rows.map(r=>r.embed),.5),rerankMedian:quantile(rows.map(r=>r.rerank),.5),otherMedian:quantile(rows.map(r=>r.other),.5),
    timingValid:monitor.exit_code===0&&!monitor.timing_contaminated,session:read(path.join(dir,'session.json')),quality,rows};
});
const baseline=runs[0];
const result=runs.map(run=>{
  const losses=[];
  for(const row of run.rows){
    const old=baseline.rows.find(r=>r.id===row.id&&r.repeat===row.repeat)??baseline.rows.find(r=>r.id===row.id);
    assert(old,'missing baseline case');
    const count=inputs.find(c=>c.id===row.id).gold_count;
    for(const threshold of [0,.1]){
      const before=metric(old.hits,count,threshold),after=metric(row.hits,count,threshold);
      const lost=['top1','hit3','mrr10','ndcg10','docTop1','docHit3','docMrr10'].filter(k=>after[k]<before[k]);
      if(lost.length)losses.push({id:row.id,repeat:row.repeat,threshold,metrics:lost});
      if(!count&&after.falseNonempty>before.falseNonempty)losses.push({id:row.id,repeat:row.repeat,threshold,metrics:['falseNonempty']});
    }
  }
  const {rows,...summary}=run;
  return {...summary,baseline:baseline.config.name,medianSpeedup:baseline.timingValid&&run.timingValid?baseline.median/run.median:null,losses,
    changedCandidateQueries:[...new Set(rows.filter(r=>r.candidate_delta).map(r=>r.id))]};
});
fs.writeFileSync(out,JSON.stringify({kind:'full-retrieval',data,runs:result},null,2));
for(const r of result)console.log(`${r.config.name} n=${r.n} median=${r.median.toFixed(3)} p95=${r.p95.toFixed(3)} losses=${r.losses.length} timingValid=${r.timingValid}`);
