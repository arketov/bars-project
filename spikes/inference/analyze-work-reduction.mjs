// Offline ranking simulation on frozen Stage 9 scores; no new latency measurement.
import fs from 'node:fs';
import zlib from 'node:zlib';
import crypto from 'node:crypto';
const inputs=JSON.parse(fs.readFileSync('spikes/inference/data/overdrive/inputs.json'));
const archive=name=>zlib.gunzipSync(fs.readFileSync(`docs/knowledge/quality-2026-10-18/${name}-rows.jsonl.gz`)).toString().trim().split('\n').map(JSON.parse);
const rows=[...archive('train'),...archive('test')].filter(r=>r.mode==='search'&&r.candidates===30&&r.k===15&&r.repeat===0);
if(rows.length!==100)throw Error('Expected 100 frozen K15 rows');
function metrics(hits,threshold) {
  const kept=hits.filter(h=>h.score>=threshold);
  const rank=kept.slice(0,10).findIndex(h=>h.relevant.length);
  return {top1:Number(!!kept[0]?.relevant.length),hit3:Number(kept.slice(0,3).some(h=>h.relevant.length)),mrr10:rank<0?0:1/(rank+1),nonempty:Number(!!kept.length)};
}
const variants=[];
for(const spec of [...[3,5,8,10,15].map(k=>({k})),...[.1,.2,.3,.4,.5].map(cutoff=>({cutoff}))]) {
  let pairs=0;const losses=[];let positive=0;const sums={top1:0,hit3:0,mrr10:0};
  const negativeFalse={0:0,0.1:0};
  for(const row of rows) {
    const input=inputs.find(c=>c.id===row.id);
    if(!input)throw Error('Missing input');
    const ids=new Set(input.hits.slice(0,spec.k??15).filter(h=>spec.cutoff===undefined||h.score>=spec.cutoff).map(h=>h.id));
    const selected=row.hits.filter(h=>ids.has(h.id));pairs+=selected.length;
    for(const threshold of [0,.1]) {
      const before=metrics(row.hits,threshold),after=metrics(selected,threshold);
      if(!input.gold_count){negativeFalse[threshold]+=after.nonempty;continue;}
      const lost=['top1','hit3','mrr10'].filter(k=>after[k]<before[k]);
      if(lost.length)losses.push({id:row.id,threshold,metrics:lost});
      if(threshold===0){positive++;for(const k of Object.keys(sums))sums[k]+=after[k];}
    }
  }
  variants.push({spec,meanPairs:pairs/rows.length,positive,qualityAtZero:Object.fromEntries(Object.entries(sums).map(([k,v])=>[k,v/positive])),negativeFalse,losses});
}
const result={method:'Offline subset simulation from frozen Stage9 C30 K15 scores; batching numerical changes and latency unmeasured',questions:rows.length,inputSha256:crypto.createHash('sha256').update(fs.readFileSync('spikes/inference/data/overdrive/inputs.json')).digest('hex'),variants};
fs.writeFileSync('spikes/inference/runs/work-reduction.json',JSON.stringify(result,null,2));
for(const r of variants)console.log(JSON.stringify({spec:r.spec,pairs:r.meanPairs,losses:r.losses.length}));
