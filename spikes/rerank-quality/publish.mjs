// Publish only complete runs. Weights, DLLs and temporary DBs remain outside Git.
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import assert from 'node:assert/strict';
import {read,write,inside,sha} from './freeze.mjs';
import {validateEvents,analyze} from './analyze.mjs';

const [configFile,destination]=process.argv.slice(2);
assert(configFile&&destination,'publish-config.json new-evidence-directory');
const cfg=read(configFile),out=inside(destination);
assert(!fs.existsSync(out),'New destination required');
const inputsFile=path.join(cfg.frozen,'inputs.json'),inputs=read(inputsFile),protocol=read(path.join(cfg.frozen,'protocol.json'));
assert.equal(sha(fs.readFileSync(inputsFile)),protocol.inputs_sha256);
assert.equal(cfg.runs.length,7,'Predeclared model matrix must be complete');
assert.deepEqual(cfg.runs.map(r=>r.name).sort(),['onnx-directml-fp16','bge-q8','qwen06-q8','qwen06-q4','qwen06-q8-voodisss','qwen4-q4','qwen8-q4'].sort());
for(const run of cfg.runs) {
  const events=fs.readFileSync(run.file,'utf8').trim().split('\n').map(JSON.parse);
  validateEvents(inputs,events,run.name,protocol.inputs_sha256);
}
fs.mkdirSync(out,{recursive:true});
const copy=(source,name)=>fs.copyFileSync(source,path.join(out,name),fs.constants.COPYFILE_EXCL);
const zip=(source,name)=>fs.writeFileSync(path.join(out,name+'.gz'),zlib.gzipSync(fs.readFileSync(source)),{flag:'wx'});
zip(inputsFile,'inputs.json');
for(const name of ['protocol.json','historical-baseline.json','input-token-diagnostics.json']) {
  if(name.startsWith('input-token'))zip(path.join(cfg.frozen,name),name);
  else copy(path.join(cfg.frozen,name),name);
}
for(const run of cfg.runs) {
  zip(run.file,run.name+'.jsonl');
  if(run.log)zip(run.log,run.name+'-server.log');
}
for(const extra of cfg.extras??[])zip(extra.file,extra.name);
copy(cfg.hardware,'hardware.json');
const code=fs.readdirSync('spikes/rerank-quality').filter(f=>f.endsWith('.mjs')).map(file=>({file:'spikes/rerank-quality/'+file,sha256:sha(fs.readFileSync('spikes/rerank-quality/'+file))}));
write(path.join(out,'provenance.json'),{code,source_config:cfg,runs:cfg.runs.map(r=>({name:r.name,source_sha256:sha(fs.readFileSync(r.file))}))});
analyze(out,cfg.runs.map(r=>r.name));
write(path.join(out,'checksums.json'),Object.fromEntries(fs.readdirSync(out).sort().map(file=>[file,sha(fs.readFileSync(path.join(out,file)))])));
