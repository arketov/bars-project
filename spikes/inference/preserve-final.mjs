// Компактные результаты без SQLite и текстов чужого проекта.
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import crypto from 'node:crypto';
const dest='docs/knowledge/inference-2026-10-18';
const read=p=>JSON.parse(fs.readFileSync(p,'utf8').replace(/^\uFEFF/,''));
const index=[];
for(const name of ['embed-dml-b4','embed-fp16-dml-b8','embed-int8-cpu16-b1','embed-cpu16-b4']){
  const dir=`spikes/inference/runs/index-validation/${name}`;
  if(!fs.existsSync(`${dir}/complete.json`))continue;
  const complete=read(`${dir}/complete.json`),monitor=read(`${dir}.monitor.json`);
  if(complete.rows.length!==3||complete.rows.some(r=>!r.report.includes('problems: []')||r.documents!==176||r.chunks!==946))throw Error(`Bad index ${name}`);
  index.push({complete,monitor,timingValid:monitor.exit_code===0&&!monitor.timing_contaminated});
}
fs.writeFileSync(`${dest}/indexing.json.gz`,zlib.gzipSync(JSON.stringify({kind:'actual-update-index',limitation:'one full/unchanged/incremental pass per variant; model load/warm excluded; embedding retrieval quality not validated',runs:index})));
fs.copyFileSync('spikes/inference/runs/embed-screen-summary.json',`${dest}/embedding.json`);
fs.writeFileSync(`${dest}/embedding-artifacts.json`,JSON.stringify({
  fp16:read('spikes/inference/data/embed-models/ranged-model_fp16.onnx.verified.json'),
  int8:read('spikes/inference/data/embed-models/ranged-model_quantized.onnx.verified.json'),
},null,2));
const proof={};
for(const root of ['int8-quality','int8-primary-quality','cpu-reference','quiet-cpu-more']){
  for(const entry of fs.readdirSync(`spikes/inference/runs/${root}`,{withFileTypes:true})){
    if(!entry.isDirectory())continue;
    const dir=`spikes/inference/runs/${root}/${entry.name}`;
    for(const file of ['config.json','session.json','rows.jsonl','complete.json','rejected.json']){
      if(fs.existsSync(`${dir}/${file}`))proof[`${root}/${entry.name}/${file}`]=fs.readFileSync(`${dir}/${file}`,'utf8');
    }
    if(fs.existsSync(`${dir}.monitor.json`))proof[`${root}/${entry.name}.monitor.json`]=fs.readFileSync(`${dir}.monitor.json`,'utf8');
  }
}
fs.writeFileSync(`${dest}/cpu-int8-proof.json.gz`,zlib.gzipSync(JSON.stringify({limitation:'primary loss od-010 at threshold .1; early od-001 stopped on secondary metric; quality-only timings invalid',files:proof})));
fs.copyFileSync('spikes/inference/provenance/base-models.json',`${dest}/base-models.json`);
const checks=[
  ['cargo test -p bars-core --lib --quiet','final-core-tests.log',197],
  ['cargo test -p bars-worker --lib --quiet','final-worker-tests.log',142],
  ['cargo test -p bars-core --test builtin_models builtin_rerank_scores_are_probabilities_in_input_order -- --nocapture','final-cpu-rerank.log',1],
];
if(checks.every(([,log])=>fs.existsSync(`spikes/inference/provenance/${log}`))){
  const tests=checks.map(([command,log,expected])=>{
    const bytes=fs.readFileSync(`spikes/inference/provenance/${log}`),text=bytes.toString();
    const match=text.match(/test result: ok\. (\d+) passed; 0 failed/);
    if(!match||Number(match[1])!==expected)throw Error(`Unexpected final tests: ${log}`);
    if(expected===1&&!text.includes('загружен на Cpu; precision=fp32; batch=4'))throw Error('CPU test skipped or wrong precision');
    return {command,passed:expected,failed:0,logSha256:crypto.createHash('sha256').update(bytes).digest('hex')};
  });
  fs.writeFileSync(`${dest}/verification.json`,JSON.stringify({date:'2026-10-18',tests,
    review:'Manual review: pinned artifacts, GPU/CPU cache and fallback, bounded allocation backoff, unchanged K/length/threshold, output order/finite checks; parallel source drift retained explicitly',
    notAccepted:['fresh whole-artifact network download','small-VRAM physical backoff','laptop/ISS-03','natural 5000-chunk corpus','FP16 embedding retrieval','later lifecycle edits from other session'],
  },null,2));
}
const sums={};
for(const name of fs.readdirSync(dest).sort())if(name!=='checksums.json'&&!name.endsWith('.md')){
  sums[name]=crypto.createHash('sha256').update(fs.readFileSync(path.join(dest,name))).digest('hex');
}
fs.writeFileSync(`${dest}/checksums.json`,JSON.stringify(sums,null,2)+'\n');
console.log(`Preserved ${index.length} complete indexing variants; ${Object.keys(proof).length} CPU proof files`);
