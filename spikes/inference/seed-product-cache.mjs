// Локальная приёмка loader без повторного скачивания; источники только читаются.
import fs from 'node:fs';
import path from 'node:path';
const source=path.join(process.env.USERPROFILE,'.bars','models');
const dest=path.resolve('spikes/inference/data/product-models');
function linkTree(src,out){
  fs.mkdirSync(out,{recursive:true});
  for(const entry of fs.readdirSync(src,{withFileTypes:true})){
    const a=path.join(src,entry.name),b=path.join(out,entry.name);
    if(entry.isDirectory())linkTree(a,b);
    else if(!fs.existsSync(b))fs.linkSync(fs.realpathSync(a),b);
  }
}
for(const repo of ['models--BAAI--bge-m3','models--rozgo--bge-reranker-v2-m3'])linkTree(path.join(source,repo),path.join(dest,repo));
const weights=path.join(dest,'models--onnx-community--bge-reranker-v2-m3-ONNX','snapshots','6f5ff65298512715a1e669753bc754d2bc8f367b','onnx','model_fp16.onnx');
fs.mkdirSync(path.dirname(weights),{recursive:true});
if(!fs.existsSync(weights))fs.linkSync(path.resolve('spikes/inference/data/models/ranged-model_fp16.onnx'),weights);
// hf-hub 0.5 требует refs/<revision> даже для revision, равной полному commit hash.
for(const [repo,revision] of [
  ['models--onnx-community--bge-reranker-v2-m3-ONNX','6f5ff65298512715a1e669753bc754d2bc8f367b'],
  ['models--rozgo--bge-reranker-v2-m3','fbd57b17b4db111a9d16813bb08b4c804fac18e9'],
]){
  const refs=path.join(dest,repo,'refs');fs.mkdirSync(refs,{recursive:true});
  const ref=path.join(refs,revision);if(!fs.existsSync(ref))fs.writeFileSync(ref,revision);
}
console.log(dest);
