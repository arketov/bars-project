// IDEA-06: анализ сырых строк. Node.js, без дополнительных пакетов.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';

export function metrics(hits, goldCount) {
  const seen = new Set();
  let dcg = 0, mrr = 0;
  for (const [i, hit] of hits.slice(0, 10).entries()) {
    if (hit.relevant.length && !mrr) mrr = 1 / (i + 1);
    if (hit.relevant.some(g => !seen.has(g))) dcg += 1 / Math.log2(i + 2);
    hit.relevant.forEach(g => seen.add(g));
  }
  const ideal = Array.from({length: Math.min(10, goldCount)}, (_, i) => 1 / Math.log2(i + 2)).reduce((a,b) => a+b, 0);
  const hit = k => Number(hits.slice(0,k).some(h => h.relevant.length));
  return {top1: hit(1), hit3: hit(3), hit5: hit(5), mrr10: mrr, ndcg10: ideal ? dcg/ideal : 0};
}
export function recall(hits, k, goldCount) {
  return goldCount ? new Set(hits.slice(0,k).flatMap(h=>h.relevant)).size / goldCount : 0;
}
const quantile = (xs, q) => [...xs].sort((a,b)=>a-b)[Math.max(0, Math.ceil(q*xs.length)-1)] ?? 0;
const mean = xs => xs.length ? xs.reduce((a,b)=>a+b,0)/xs.length : 0;
function summarize(rows, cases, threshold, cosine = false) {
  const positive = [], negative = [], timings = [];
  let loss = 0;
  for (const row of rows) {
    timings.push(row);
    if (row.repeat !== 0) continue;
    const c = cases.get(row.id);
    const sorted = cosine ? [...row.hits].sort((a,b)=>b.score-a.score) : row.hits;
    const kept = sorted.filter(h=>h.score>=threshold);
    const limit = row.k ? 3 : 5;
    if (!c.gold.length) { negative.push(Number(kept.slice(0,limit).length>0)); continue; }
    const m = metrics(kept,c.gold.length);
    const documentRank=kept.slice(0,10).findIndex(h=>h.document_relevant);
    positive.push({...m,docTop1:Number(kept.slice(0,1).some(h=>h.document_relevant)),docHit3:Number(kept.slice(0,3).some(h=>h.document_relevant)),docMrr10:documentRank<0?0:1/(documentRank+1)});
    if (sorted.slice(0,limit).some(h=>h.relevant.length) && !kept.slice(0,limit).some(h=>h.relevant.length)) loss++;
  }
  const metricsKeys = ['top1','hit3','hit5','mrr10','ndcg10','docTop1','docHit3','docMrr10'];
  const first=new Map(rows.filter(r=>r.repeat===0).map(r=>[r.id,JSON.stringify(r.hits.map(h=>h.id))]));
  const repeatOrderChanges=rows.filter(r=>r.repeat>0&&JSON.stringify(r.hits.map(h=>h.id))!==first.get(r.id)).length;
  return {positive:positive.length,negative:negative.length,
    ...Object.fromEntries(metricsKeys.map(k=>[k,mean(positive.map(m=>m[k]))])),
    falseNonempty:mean(negative),falseNonemptyCount:negative.reduce((a,b)=>a+b,0),lostAnswers:loss,repeatOrderChanges,
    median:quantile(timings.map(r=>r.total),.5),p95:quantile(timings.map(r=>r.total),.95),
    embedMedian:quantile(timings.map(r=>r.embed),.5),rerankMedian:quantile(timings.map(r=>r.rerank),.5),
    restMedian:quantile(timings.map(r=>r.total-r.embed-r.rerank),.5)};
}
function analyze(dataset, dirs) {
  const cases = new Map(dataset.map(c=>[c.id,c])), rows = [];
  const sources = [];
  for (const dir of dirs) {
    const config=JSON.parse(fs.readFileSync(`${dir}/config.json`,'utf8'));
    const part=fs.readFileSync(`${dir}/rows.jsonl`,'utf8').trim().split('\n').filter(Boolean).map(l=>JSON.parse(l));
    validateRun(part,config,dataset);
    rows.push(...part);sources.push({dir,config});
  }
  const groups=new Map();
  for(const row of rows){const split=cases.get(row.id).split;const key=`${split}/C${row.candidates}/${row.mode}/K${row.k}`;if(!groups.has(key))groups.set(key,[]);groups.get(key).push(row);}
  const quality=[],retrieval=[];
  for(const [name,part] of groups){
    if(part[0].mode!=='search'){
      const positive=part.filter(r=>cases.get(r.id).gold.length);
      retrieval.push({name,positive:positive.length,
        curve:[3,5,8,10,15,20,30,50].map(k=>({k,recall:mean(positive.map(r=>recall(r.hits,k,cases.get(r.id).gold.length))),hit:mean(positive.map(r=>Number(r.hits.slice(0,k).some(h=>h.relevant.length))))})),
        ranks:positive.map(r=>({id:r.id,rank:r.hits.findIndex(h=>h.relevant.length)+1||null}))});
      continue;
    }
    for(const cosine of (part[0].k ? [false] : [false,true])) {
      for(const threshold of (part[0].k ? [0,.02,.05,.1,.3,.5] : [0,.4,.5,.6])) {
        quality.push({name:name+(cosine?'/cosine':''),threshold,...summarize(part,cases,threshold,cosine),
          categories:Object.fromEntries([...new Set(part.map(r=>cases.get(r.id).category))].map(cat=>[cat,summarize(part.filter(r=>cases.get(r.id).category===cat),cases,threshold,cosine)]))});
      }
    }
  }
  return {quality_unit:dataset.every(c=>c.category==='historical-document')?'document (use docTop1/docHit3/docMrr10)':'canonical section',sources,quality,retrieval};
}
function validateRun(rows,config,dataset){
  const expected=new Set();
  for(const c of dataset.filter(c=>c.split===config.split))for(const count of config.candidates){
    for(let repeat=0;repeat<config.repeats;repeat++)for(const k of [0,...config.depths])expected.add(`${c.id}:${count}:${k}:${repeat}:search`);
    for(const mode of ['fts','vector','fused'])expected.add(`${c.id}:${count}:0:0:${mode}`);
  }
  const keys=new Set();
  for(const r of rows){const key=`${r.id}:${r.candidates}:${r.k}:${r.repeat}:${r.mode}`;assert(expected.has(key),`лишняя строка ${key}`);assert(!keys.has(key),`дубль ${key}`);keys.add(key);}
  assert.equal(keys.size,expected.size,'неполный прогон');
}
function selfTest(){
  const hit=(relevant,score=.9)=>({relevant,score,document_relevant:true});
  const list=[hit([]),hit([0]),hit([0]),hit([1])];
  assert.equal(recall(list,3,2),.5);assert.equal(recall(list,4,2),1);
  const m=metrics(list,2);assert.equal(m.mrr10,.5);assert.equal(m.top1,0);assert.equal(m.hit3,1);
  assert(Math.abs(m.ndcg10-(1/Math.log2(3)+1/Math.log2(5))/(1+1/Math.log2(3)))<1e-12);
  assert.equal(metrics([],0).mrr10,0);assert.equal(metrics([],0).ndcg10,0);
  const cases=new Map([['p',{gold:[{}]}],['n',{gold:[]}]]);
  const base={repeat:0,total:1,embed:.1,rerank:.8,k:15};
  const rows=[{...base,id:'p',hits:[hit([0],.2)]},{...base,id:'n',hits:[hit([],.1)]}];
  assert.equal(summarize(rows,cases,0).falseNonempty,1);
  const rejected=summarize(rows,cases,.3);assert.equal(rejected.lostAnswers,1);assert.equal(rejected.falseNonempty,0);
  assert.equal(quantile([1,2,3,4,5],.95),5);
  const config={split:'tune',candidates:[30],depths:[15],repeats:1};
  const data=[{id:'p',split:'tune'}];
  const complete=[{id:'p',candidates:30,k:15,repeat:0,mode:'search'},...['search','fts','vector','fused'].map(mode=>({id:'p',candidates:30,k:0,repeat:0,mode}))];
  validateRun(complete,config,data);
  assert.throws(()=>validateRun(complete.slice(1),config,data),/неполный/);
  assert.throws(()=>validateRun([...complete,complete[0]],config,data),/дубль/);
  assert.throws(()=>validateRun([{...complete[0],repeat:2}],config,data),/лишняя/);
  console.log('quality analysis: all assertions passed');
}
// Импорт метрик из других анализаторов не запускает этот.
const main=process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href;
if(main&&process.argv[2]==='--self-test')selfTest();
else if(main){
  const [datasetPath,output,...dirs]=process.argv.slice(2);
  if(!dirs.length)throw Error('node scripts/analyze-quality.mjs <cases.json> <summary.json> <run-dir> [...]');
  const dataset=JSON.parse(fs.readFileSync(datasetPath,'utf8').replace(/^\uFEFF/,''));
  const result=analyze(dataset,dirs);fs.writeFileSync(output,JSON.stringify(result,null,2)+'\n');
  const documentOnly=dataset.every(c=>c.category==='historical-document');
  for(const row of result.quality.filter(r=>r.threshold===0))console.log(`${row.name} unit=${documentOnly?'document':'section'} top1=${(documentOnly?row.docTop1:row.top1).toFixed(3)} hit3=${(documentOnly?row.docHit3:row.hit3).toFixed(3)} MRR=${(documentOnly?row.docMrr10:row.mrr10).toFixed(3)} med=${row.median.toFixed(3)} p95=${row.p95.toFixed(3)}`);
}
