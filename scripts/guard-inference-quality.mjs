// Stop a slow exploratory variant on the first predeclared quality regression.
import fs from 'node:fs';
import crypto from 'node:crypto';
import {metric} from './analyze-inference.mjs';
const [inputsFile,referenceFile,rowsFile,rejectionFile]=process.argv.slice(2);
const inputs=JSON.parse(fs.readFileSync(inputsFile));
const parse=p=>{
  const bytes=fs.readFileSync(p);const end=bytes.lastIndexOf(10);
  return end<0?[]:bytes.subarray(0,end).toString().split(/\r?\n/).filter(Boolean).map(JSON.parse);
};
const reference=parse(referenceFile),rows=parse(rowsFile);
for(const row of rows){
  const old=reference.find(r=>r.id===row.id),input=inputs.find(c=>c.id===row.id);
  if(!old||!input)throw Error('Missing frozen reference/input');
  for(const threshold of [0,.1]){
    const before=metric(old.hits,input.gold_count,threshold),after=metric(row.hits,input.gold_count,threshold);
    const lost=['top1','hit3','mrr10'].filter(k=>after[k]<before[k]);
    const secondaryLost=['ndcg10','docTop1','docHit3','docMrr10'].filter(k=>after[k]<before[k]);
    if(!input.gold_count&&after.falseNonempty>before.falseNonempty)lost.push('falseNonempty');
    if(lost.length){
      fs.writeFileSync(rejectionFile,JSON.stringify({status:'rejected-primary-quality',id:row.id,threshold,lost,secondaryLost,before,after,completedObservations:rows.length,
        referenceSha256:crypto.createHash('sha256').update(fs.readFileSync(referenceFile)).digest('hex'),limitation:'counterexample only; not complete quality metrics or clean timing'},null,2));
      console.log(`Quality regression: ${row.id}, ${lost.join(',')}`);process.exit(2);
    }
  }
}
