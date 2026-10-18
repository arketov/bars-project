// Проверка DirectML против CPU на тех же query/chunk парах; не подбор параметров.
import fs from 'node:fs';
import assert from 'node:assert/strict';
const [autoDir,cpuDir,output]=process.argv.slice(2);
if(!output)throw Error('node scripts/compare-quality-devices.mjs <auto-dir> <cpu-dir> <output.json>');
const read=dir=>fs.readFileSync(`${dir}/rows.jsonl`,'utf8').trim().split('\n').map(JSON.parse).filter(r=>r.mode==='search'&&r.candidates===30&&r.k===15&&r.repeat===0);
const gpu=new Map(read(autoDir).map(r=>[r.id,r])),cpu=read(cpuDir);
const pairs=cpu.map(row=>{
  const other=gpu.get(row.id);assert(other,`нет GPU-вопроса ${row.id}`);
  const scores=new Map(other.hits.map(h=>[h.id,h.score]));
  const differences=row.hits.filter(h=>scores.has(h.id)).map(h=>Math.abs(h.score-scores.get(h.id)));
  assert(differences.length>=10,`мало общих кандидатов ${row.id}`);
  return {id:row.id,commonPairs:differences.length,maxAbs:Math.max(...differences),meanAbs:differences.reduce((a,b)=>a+b,0)/differences.length,
    sameLeader:row.hits[0].id===other.hits[0].id,cpuTop:row.hits[0].score,directmlTop:other.hits[0].score,cpuTotal:row.total,directmlTotal:other.total};
});
assert.equal(pairs.length,4,'ожидались четыре контрольных запроса');
const maxAbs=Math.max(...pairs.map(p=>p.maxAbs)),meanAbs=pairs.reduce((a,p)=>a+p.meanAbs,0)/pairs.length;
const result={purpose:'provider sanity, not a CPU quality sweep',pairs,maxAbs,meanAbs,passed:maxAbs<.05&&meanAbs<.01};
fs.writeFileSync(output,JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify(result,null,2));
assert(result.passed,'DirectML существенно отличается от CPU: проверочную часть не запускать');
