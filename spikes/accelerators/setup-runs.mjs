// Reconstruct actual run names/configs in a new fixture; never overwrite frozen evidence.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
const root=process.cwd(),source=path.join(root,'docs/knowledge/accelerators-2026-11-01');
const models=path.resolve(process.argv[2]??path.join(os.homedir(),'.bars/models'));
const fixture=path.resolve(process.argv[3]??path.join(root,`docs/chats/2026-11-01-rtx-accelerators/fixture-replay-${Date.now()}`));
if(!fixture.toLowerCase().startsWith((root+path.sep).toLowerCase())||fs.existsSync(fixture))throw new Error('A new fixture directory inside this checkout is required');
const runs=path.join(fixture,'runs'),evidence=path.join(fixture,'evidence');
const protocol=JSON.parse(fs.readFileSync(path.join(source,'protocol.json')));
const targets={
  gguf:path.join(models,'models--Qwen--Qwen3-Embedding-4B-GGUF/blobs/2b0cf8f17b4c723c27303015383c27ec4bf2d8314bb677d05e920dd70bb0f16b'),
  onnx:path.join(models,'models--onnx-community--bge-reranker-v2-m3-ONNX/snapshots/6f5ff65298512715a1e669753bc754d2bc8f367b/onnx/model_fp16.onnx'),
  tokenizer:path.join(models,'models--rozgo--bge-reranker-v2-m3/snapshots/fbd57b17b4db111a9d16813bb08b4c804fac18e9/tokenizer.json'),
};
const artifacts=JSON.parse(fs.readFileSync(path.join(source,'artifacts.json')));
for(const artifact of artifacts){
  const file=targets[artifact.role];if(!file)throw new Error('Unknown model role');
  const hash=crypto.createHash('sha256');for await(const chunk of fs.createReadStream(file))hash.update(chunk);
  if(hash.digest('hex')!==artifact.sha256)throw new Error(`Model digest mismatch: ${artifact.role}`);
}
fs.mkdirSync(runs,{recursive:true});fs.mkdirSync(evidence);
for(const file of ['inputs.json','gguf-inputs.json','fixed-inputs.json'])fs.copyFileSync(path.join(source,file),path.join(evidence,file),fs.constants.COPYFILE_EXCL);
fs.writeFileSync(path.join(evidence,'artifacts.json'),JSON.stringify(artifacts.map(a=>({...a,file:targets[a.role]})),null,2),{flag:'wx'});
for(const {name,config} of protocol.configs){
  if(!/^[A-Za-z0-9-]+$/.test(name))throw new Error('Invalid saved config name');
  const cfg={...config,name,model:targets[config.kind],tokenizer:config.kind==='onnx'?targets.tokenizer:null,
    inputs:path.join(evidence,path.basename(config.inputs)),profile:config.profile?path.join(runs,name+'.profile'):null,
    cache:config.cache?path.join(runs,'trt-cache'):null};
  fs.writeFileSync(path.join(runs,name+'.json'),JSON.stringify(cfg,null,2),{flag:'wx'});
}
const names=new Set(protocol.configs.map(c=>c.name));
const gguf=[];
for(const suffix of ['serial','controlled'])for(const backend of ['Vulkan','CUDA'])
  for(const mode of ['off-f16','on-f16','on-q8','on-q4','on-b128','on-b2048','off-f16-repeat']){
    const name=`gguf-${backend}-${mode}-${suffix}`;if(names.has(name))gguf.push(name);
  }
const directml=['onnx-directml-fixed-v2','onnx-directml-b16','onnx-directml-b4','onnx-directml-b1','onnx-directml-binding','onnx-directml-fixed-binding','onnx-directml-b16-repeat'];
const cuda=['fixed','b16','b4','b1','binding','fixed-binding','graph','b16-repeat','profile'].map(s=>`onnx-cuda-${s}-v2`).concat('onnx-cuda-graph-profile');
fs.writeFileSync(path.join(fixture,'order.json'),JSON.stringify({gguf,directml:directml.filter(n=>names.has(n)),cuda:cuda.filter(n=>names.has(n)),
  optional_failures:['onnx-tensorrt-v3'].filter(n=>names.has(n))},null,2),{flag:'wx'});
const quote=s=>"'"+s.replaceAll("'","''")+"'";
console.log(`$env:BARS_PROBE_RUNS_DIR = ${quote(runs)}\n$env:BARS_PROBE_EVIDENCE_DIR = ${quote(evidence)}\n$order = Get-Content -Raw ${quote(path.join(fixture,'order.json'))} | ConvertFrom-Json`);
console.log('Run one group at a time; no simultaneous GGUF/ONNX GPU work. Then node spikes/accelerators/analyze.mjs.');
