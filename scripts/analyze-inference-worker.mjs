import fs from 'node:fs';
import zlib from 'node:zlib';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {metric,quantile} from './analyze-inference.mjs';
const [root,data,dest,workerExe]=process.argv.slice(2);
const read=p=>JSON.parse(fs.readFileSync(p,'utf8').replace(/^\uFEFF/,''));
const cases=read(`${data}/cases.json`);
const names=['production','final-fp16-b16'];
const counts={serial:8,'concurrent-2':2,'concurrent-4':4,'during-index':8};
const all=[];const metadata={binarySha256:crypto.createHash('sha256').update(fs.readFileSync(workerExe)).digest('hex'),runs:[]};
const summary=names.map(name=>{
  const dir=`${root}/${name}`,bytes=fs.readFileSync(`${dir}/rows.jsonl`);
  const rows=bytes.toString().trim().split(/\r?\n/).map(JSON.parse);
  assert.equal(rows.length,22);
  const monitor=read(`${dir}.monitor.json`),complete=read(`${dir}/complete.json`);
  assert.equal(monitor.exit_code,0);
  const keys=new Set();
  for(const row of rows){
    assert(!keys.has(`${row.mode}:${row.query}`));keys.add(`${row.mode}:${row.query}`);
    assert(row.response.applied.score_kind==='rerank');assert(!row.response.rerank_error);
    assert(row.response.results.every(h=>h.score_kind==='rerank'&&Number.isFinite(h.score)));
    if(row.mode==='during-index')assert(row.active_at_start);
    const matches=cases.filter(c=>c.query===row.query);assert.equal(matches.length,1);const c=matches[0];
    const hits=row.response.results.map(h=>({...h,relevant:c.gold.flatMap((g,i)=>g.path===h.path&&h.text.includes(g.evidence)?[i]:[]),document_relevant:c.gold.some(g=>g.path===h.path)}));
    row.case_id=c.id;row.metrics=[0,.1].map(threshold=>({threshold,...metric(hits,c.gold.length,threshold)}));
    const redacted={...row,response:{...row.response,results:row.response.results.map(h=>{const {text,...rest}=h;return {...rest,text_sha256:crypto.createHash('sha256').update(text).digest('hex')};})}};
    all.push({run:name,...redacted});
  }
  metadata.runs.push({name,complete,monitor,originalRowsSha256:crypto.createHash('sha256').update(bytes).digest('hex')});
  return {name,n:rows.length,timingValid:!monitor.timing_contaminated,modes:Object.entries(counts).map(([mode,count])=>{const a=rows.filter(r=>r.mode===mode);assert.equal(a.length,count);return {mode,n:a.length,median:quantile(a.map(r=>r.seconds),.5),max:Math.max(...a.map(r=>r.seconds))}}),rows};
});
const losses=[];
for(const row of summary[1].rows){
  const old=summary[0].rows.find(r=>r.mode===row.mode&&r.case_id===row.case_id);assert(old);
  for(let i=0;i<2;i++){
    const lost=['top1','hit3','mrr10','ndcg10','docTop1','docHit3','docMrr10'].filter(k=>row.metrics[i][k]<old.metrics[i][k]);
    if(lost.length)losses.push({mode:row.mode,id:row.case_id,threshold:row.metrics[i].threshold,metrics:lost});
  }
}
fs.writeFileSync(`${dest}/worker.json`,JSON.stringify({kind:'real-worker-ipc',limitation:'8 serial, 2/4 concurrent and 8 index-overlap requests per variant; not a general latency SLA',runs:summary.map(({rows,...rest})=>rest),losses},null,2));
fs.writeFileSync(`${dest}/worker-metadata.json`,JSON.stringify(metadata,null,2));
fs.writeFileSync(`${dest}/worker-rows.jsonl.gz`,zlib.gzipSync(all.map(r=>JSON.stringify(r)).join('\n')+'\n'));
console.log(`Worker IPC: 44 responses checked, quality losses=${losses.length}; document bodies replaced with SHA256 in published evidence`);
