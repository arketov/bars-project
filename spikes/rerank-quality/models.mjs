// Downloads only selected, hash-pinned research weights into an ignored checkout fixture.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {read,write,inside} from './freeze.mjs';

const [directory,localQ8]=process.argv.slice(2);
assert(directory&&localQ8,'new-model-directory existing-ggml-org-Q8-file');
const out=inside(directory);
assert(!fs.existsSync(out),'New model directory required');
fs.mkdirSync(out,{recursive:true});
const inventory=fs.readFileSync('docs/knowledge/rerank-gguf-2026-10-28.jsonl','utf8').trim().split('\n').map(JSON.parse);
const selected=[
  ['qwen06-q8','ggml-org/Qwen3-Reranker-0.6B-Q8_0-GGUF','qwen3-reranker-0.6b-q8_0.gguf',localQ8],
  ['qwen06-q4','Voodisss/Qwen3-Reranker-0.6B-GGUF-llama_cpp','Qwen3-Reranker-0.6B-Q4_K_M.gguf'],
  ['qwen4-q4','Voodisss/Qwen3-Reranker-4B-GGUF-llama_cpp','Qwen3-Reranker-4B-Q4_K_M.gguf'],
  ['qwen8-q4','Voodisss/Qwen3-Reranker-8B-GGUF-llama_cpp','Qwen3-Reranker-8B-Q4_K_M.gguf'],
  ['bge-q8','gpustack/bge-reranker-v2-m3-GGUF','bge-reranker-v2-m3-Q8_0.gguf'],
];
const artifacts=[];
for(const [name,repo,file,local] of selected) {
  const spec=inventory.find(r=>r.repo===repo&&r.files)?.files.find(f=>f[0]===file);
  assert(spec,`Known artifact ${name}`);
  const response=await fetch(`https://huggingface.co/api/models/${repo}`,{signal:AbortSignal.timeout(60000)});
  assert(response.ok,`Model metadata ${response.status}`);
  const revision=(await response.json()).sha;
  assert(/^[a-f0-9]{40}$/.test(revision));
  const url=`https://huggingface.co/${repo}/resolve/${revision}/${file}`;
  const target=local?path.resolve(local):path.join(out,file);
  if(!local) {
    const disk=fs.statfsSync(out);
    assert(disk.bavail*disk.bsize>spec[1]+8*2**30,'Keep 8 GiB disk reserve');
    // HF full-file streaming can stall on this host; bounded ranges fail promptly.
    const fd=fs.openSync(target,'wx');
    try {
      for(let offset=0;offset<spec[1];offset+=8*2**20) {
        const last=Math.min(spec[1]-1,offset+8*2**20-1);
        let bytes;
        for(let attempt=0;attempt<3;attempt++) {
          try {
            const result=await fetch(url+`?download=true&part=${offset}`,{headers:{Range:`bytes=${offset}-${last}`},signal:AbortSignal.timeout(60000)});
            assert.equal(result.status,206,`Download ${result.status}`);
            assert.equal(result.headers.get('content-range'),`bytes ${offset}-${last}/${spec[1]}`);
            bytes=Buffer.from(await result.arrayBuffer());
            assert.equal(bytes.length,last-offset+1);break;
          } catch(e) {if(attempt===2)throw e;}
        }
        let written=0;
        while(written<bytes.length)written+=fs.writeSync(fd,bytes,written,bytes.length-written,offset+written);
        if(offset%(128*2**20)===0)console.log(`${name}: ${Math.round(offset/2**20)} MiB`);
      }
    } finally {fs.closeSync(fd);}
  }
  assert.equal(fs.statSync(target).size,spec[1]);
  const hash=crypto.createHash('sha256');
  for await(const bytes of fs.createReadStream(target))hash.update(bytes);
  const digest=hash.digest('hex');
  assert.equal(digest,spec[2],`SHA-256 ${name}`);
  artifacts.push({name,repo,revision,file,sha256:digest,bytes:spec[1],path:target,url});
  write(path.join(out,`${name}.json`),artifacts.at(-1));
  console.log(`Verified ${name}`);
}
write(path.join(out,'models.json'),artifacts);
