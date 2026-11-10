// Downloads only selected, hash-pinned research weights into an ignored checkout fixture.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {read,write,inside} from './freeze.mjs';

const [directory,localQ8,seedDirectory,namesCsv,resumeFlag]=process.argv.slice(2);
assert(directory&&localQ8,'new-model-directory existing-ggml-org-Q8-file');
const out=inside(directory);
assert(!fs.existsSync(out)||resumeFlag==='resume','New model directory or explicit resume required');
assert(!fs.existsSync(path.join(out,'models.json')),'Completed model directory is immutable');
fs.mkdirSync(out,{recursive:true});
const inventory=fs.readFileSync('docs/knowledge/rerank-gguf-2026-10-28.jsonl','utf8').trim().split('\n').map(JSON.parse);
const selected=[
  ['qwen06-q8','ggml-org/Qwen3-Reranker-0.6B-Q8_0-GGUF','qwen3-reranker-0.6b-q8_0.gguf',localQ8],
  ['qwen06-q4','Voodisss/Qwen3-Reranker-0.6B-GGUF-llama_cpp','Qwen3-Reranker-0.6B-Q4_K_M.gguf'],
  ['qwen06-q8-voodisss','Voodisss/Qwen3-Reranker-0.6B-GGUF-llama_cpp','Qwen3-Reranker-0.6B.Q8_0.gguf'],
  ['qwen4-q4','Voodisss/Qwen3-Reranker-4B-GGUF-llama_cpp','Qwen3-Reranker-4B-Q4_K_M.gguf'],
  ['qwen8-q4','Voodisss/Qwen3-Reranker-8B-GGUF-llama_cpp','Qwen3-Reranker-8B-Q4_K_M.gguf'],
  ['bge-q8','gpustack/bge-reranker-v2-m3-GGUF','bge-reranker-v2-m3-Q8_0.gguf'],
];
const wanted=namesCsv?namesCsv.split(','):selected.map(s=>s[0]);
assert(wanted.length&&wanted.every(name=>selected.some(s=>s[0]===name)),'Known explicit model names');
const artifacts=[];
for(const [name,repo,file,local] of selected.filter(s=>wanted.includes(s[0]))) {
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
    const seed=seedDirectory&&path.join(seedDirectory,file);
    const progressFile=path.join(out,`${name}.progress.json`);
    if(!fs.existsSync(target)&&seed&&fs.existsSync(seed))fs.copyFileSync(seed,target,fs.constants.COPYFILE_EXCL);
    let seedBytes=fs.existsSync(target)?fs.statSync(target).size:0;
    if(resumeFlag==='resume'&&fs.existsSync(target)) {
      const checkpoint=read(progressFile);
      assert.equal(checkpoint.sha256,spec[2],'Resume the same pinned artifact');
      assert(Number.isInteger(checkpoint.bytes)&&checkpoint.bytes>=0&&checkpoint.bytes<=seedBytes);
      seedBytes=checkpoint.bytes;
    }
    assert((seedBytes% (8*2**20)===0||seedBytes===spec[1])&&seedBytes<=spec[1],'Contiguous completed prefix required');
    fs.writeFileSync(progressFile,JSON.stringify({bytes:seedBytes,sha256:spec[2]}));
    const fd=fs.openSync(target,fs.existsSync(target)?'r+':'wx');
    try {
      const offsets=Array.from({length:Math.ceil((spec[1]-seedBytes)/(8*2**20))},(_,i)=>seedBytes+i*8*2**20);
      for(let group=0;group<offsets.length;group+=16) {
        const results=await Promise.allSettled(offsets.slice(group,group+16).map(async offset=>{
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
        }));
        const failure=results.find(result=>result.status==='rejected');
        if(failure)throw failure.reason;
        fs.fsyncSync(fd);
        const completed=Math.min(spec[1],offsets[Math.min(group+15,offsets.length-1)]+8*2**20);
        fs.writeFileSync(progressFile,JSON.stringify({bytes:completed,sha256:spec[2]}));
      }
    } finally {fs.closeSync(fd);}
  }
  assert.equal(fs.statSync(target).size,spec[1]);
  const hash=crypto.createHash('sha256');
  for await(const bytes of fs.createReadStream(target))hash.update(bytes);
  const digest=hash.digest('hex');
  assert.equal(digest,spec[2],`SHA-256 ${name}`);
  const verified={name,repo,revision,file,sha256:digest,bytes:spec[1],path:target,url};
  const manifestFile=path.join(out,`${name}.json`);
  if(fs.existsSync(manifestFile)) {
    assert.equal(resumeFlag,'resume','Existing manifest requires resume');
    const previous=read(manifestFile);
    for(const key of ['name','repo','file','sha256','bytes','path'])assert.equal(previous[key],verified[key],`Existing manifest ${key}`);
    artifacts.push(previous);
  } else { artifacts.push(verified);write(manifestFile,verified); }
  console.log(`Verified ${name}`);
}
write(path.join(out,'models.json'),artifacts);
