// Reconstruct only frozen questions and archived product references; no model execution.
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
const [bench, snapshots, destination]=process.argv.slice(2);
assert(destination,'bench snapshots-directory new-output-directory');
assert(!fs.existsSync(destination),'new output required');
fs.mkdirSync(destination,{recursive:true});
const read=p=>JSON.parse(fs.readFileSync(p,'utf8').replace(/^\uFEFF/,''));
const digest=p=>execFileSync(bench,['digest',path.resolve(p)],{encoding:'utf8',windowsHide:true}).trim();
const archive='docs/knowledge/inference-2026-10-18/product-rows.jsonl.gz';
const baseline=zlib.gunzipSync(fs.readFileSync(archive)).toString().trim().split('\n').map(JSON.parse).filter(r=>r.kind==='core');
const expected={bars:24,overdrive:100,holdout:14};
const hashes={bars:'4cfd5c6856958f9a0f3ab2916f7900f9943cb0e450b15e0591def3d78be3f2b6',overdrive:'00708396a6cb872f6427d08df983d8776d5f491ada467a052602b20b07cac513',holdout:'b019ea990fa156f5b2f4072259a9f7e5921ea2437ec818b267584e02b2bcb31d'};
for(const [corpus,sets] of Object.entries({bars:['bars'],overdrive:['overdrive','holdout']})){
  const out=path.join(destination,corpus);fs.mkdirSync(out);
  const source=path.resolve(snapshots,corpus==='bars'?'bars-2026-10-18':'overdrive-2026-10-18-v2');
  execFileSync(bench,['verify',source],{stdio:'inherit',windowsHide:true});
  const cases=sets.flatMap(dataset=>{
    const file=`crates/bars-core/examples/quality/${dataset==='holdout'?'inference-holdout':dataset}.json`;
    assert.equal(digest(file),hashes[dataset],`frozen questions drift: ${dataset}`);
    const list=read(file);assert.equal(list.length,expected[dataset]);
    return list.map(c=>({...c,dataset}));
  });
  const inputs=cases.map(c=>{
    const rows=baseline.filter(r=>r.dataset===c.dataset&&r.id===c.id);assert.equal(rows.length,1);
    return {id:c.id,query:c.query,split:c.split,gold_count:c.gold.length,hits:rows[0].hits,documents:[],dataset:c.dataset};
  });
  assert.equal(new Set(cases.map(c=>c.id)).size,cases.length);
  fs.writeFileSync(path.join(out,'cases.json'),JSON.stringify(cases));
  fs.writeFileSync(path.join(out,'inputs.json'),JSON.stringify(inputs));
  fs.writeFileSync(path.join(out,'manifest.json'),JSON.stringify({source,count:cases.length,index_blake3:digest(path.join(source,'index.sqlite')),cases_blake3:digest(path.join(out,'cases.json')),inputs_blake3:digest(path.join(out,'inputs.json')),candidate_reference:'archived product FP32 embed + FP16 rerank hits; not stage9 fusion',baseline_archive:archive,baseline_archive_blake3:digest(archive),sets},null,2));
}
console.log('Frozen questions recovered: bars24 + overdrive100 + holdout14; two source corpora');
