// Диагностика vector-only retrieval по сохранённым векторам; без загрузки моделей.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {DatabaseSync} from 'node:sqlite';

const [root, output] = process.argv.slice(2);
assert(root && output, 'analyze-embed-retrieval.mjs <inference-runs> <new-output.json>');
assert(!fs.existsSync(output), 'output exists');
const names=['embed-dml-b4','embed-fp16-dml-b8','embed-int8-cpu16-b1','embed-cpu16-b4'];
const hash = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const read = file => JSON.parse(fs.readFileSync(file,'utf8').replace(/^\uFEFF/,''));
const sources=[];
function source(file) { sources.push({file:file.replaceAll('\\','/'),sha256:hash(file)}); return file; }
function normalize(v) {
  assert.equal(v.length,1024);
  assert(v.every(Number.isFinite));
  const norm=Math.fround(Math.sqrt(v.reduce((sum,x)=>Math.fround(sum+Math.fround(x*x)),0)));
  assert(norm>0);
  return Float32Array.from(v,x=>Math.fround(x/norm));
}
const variants=names.map(name=>{
  const data=path.join(root,'index-validation',name,'full-data');
  const cases=read(source(path.join(data,'cases.json')));
  const rows=fs.readFileSync(source(path.join(root,'embed-screen',name,'rows.jsonl')),'utf8').trim().split(/\r?\n/).map(JSON.parse);
  const queries=rows.filter(r=>r.repeat===0).map(r=>({id:r.id,vector:normalize(r.vector)}));
  assert.equal(queries.length,10);
  assert.equal(new Set(queries.map(q=>q.id)).size,10);
  const file=source(path.join(data,'index.sqlite'));
  const db=new DatabaseSync(file,{readOnly:true});
  const docs=db.prepare('SELECT c.id,d.path,c.text,v.vector FROM chunks c JOIN documents d ON d.id=c.document_id JOIN vectors v ON v.embed_hash=c.embed_hash').all().map(r=>{
    const bytes=Buffer.from(r.vector);
    assert.equal(bytes.length,4096);
    const vector=Array.from({length:1024},(_,i)=>bytes.readFloatLE(i*4));
    assert(vector.every(Number.isFinite));
    return {id:r.id,path:r.path,text:r.text,vector};
  });
  db.close();
  assert.equal(docs.length,946);
  return {name,cases,queries,docs};
});
const caseDigest=c=>crypto.createHash('sha256').update(JSON.stringify(c)).digest('hex');
for(const v of variants) {
  assert.deepEqual(v.queries.map(q=>q.id),variants[0].queries.map(q=>q.id));
  assert.equal(caseDigest(v.cases),caseDigest(variants[0].cases));
  assert.equal(caseDigest(v.docs.map(d=>[d.id,d.path,d.text])),caseDigest(variants[0].docs.map(d=>[d.id,d.path,d.text])));
}
const results=[];
for(const index of variants) for(const query of variants) {
  const rows=query.queries.map(q=>{
    const c=index.cases.find(c=>c.id===q.id); assert(c);
    const hits=index.docs.map(d=>{
      let score=0; for(let i=0;i<1024;i++) score=Math.fround(score+Math.fround(d.vector[i]*q.vector[i]));
      return {id:d.id,path:d.path,score,relevant:c.gold.flatMap((g,i)=>g.path===d.path&&d.text.includes(g.evidence)?[i]:[])};
    }).sort((a,b)=>b.score-a.score||a.id-b.id).slice(0,15);
    const recalled=new Set(hits.flatMap(h=>h.relevant)).size;
    return {id:q.id,gold_count:c.gold.length,recall15:c.gold.length?recalled/c.gold.length:null,
      hit3:c.gold.length?Number(hits.slice(0,3).some(h=>h.relevant.length)):null,
      hit15:c.gold.length?Number(recalled>0):null,hits};
  });
  const positive=rows.filter(r=>r.gold_count);
  const average=key=>positive.reduce((s,r)=>s+r[key],0)/positive.length;
  results.push({index:index.name,query:query.name,questions:rows.length,positive:positive.length,
    recall15:average('recall15'),hit3:average('hit3'),hit15:average('hit15'),rows});
}
const baseline=results[0];
for(const r of results) r.losses= r.rows.filter(row=>row.gold_count&&row.recall15<baseline.rows.find(b=>b.id===row.id).recall15).map(row=>row.id);
const report={schema:1,method:'vector-only; saved 10 queries and 946 frozen chunks; float32 dot; all 16 index/query precision combinations; not full hybrid/rerank or acceptance',sources,results};
fs.writeFileSync(output,JSON.stringify(report,null,2)+'\n',{flag:'wx'});
for(const {rows,...r} of results) console.log(JSON.stringify(r));
