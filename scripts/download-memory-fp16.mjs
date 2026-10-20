// One pinned artifact, bounded 4 MiB ranges; no copied model variants.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const [root]=process.argv.slice(2);
assert(root&&/^bars-memory-stage10-[a-f0-9]{32}$/.test(path.basename(root)));
const repository='Xenova/bge-m3',revision='4de13258303883538bd53b696b452bf8099f0858';
async function request(url,options={}){
  for(let attempt=0;attempt<3;attempt++)try{
    const response=await fetch(url,{...options,signal:AbortSignal.timeout(45000)});
    if(response.status===429||response.status>=500)throw Error(`HTTP ${response.status}`);
    return response;
  }catch(error){if(attempt===2)throw error;await new Promise(r=>setTimeout(r,1000*(attempt+1)));}
}
const metadata=await(await request(`https://huggingface.co/api/models/${repository}/tree/${revision}/onnx`)).json();
const item=metadata.find(f=>f.path==='onnx/model_fp16.onnx');
assert.equal(item?.size,1133992936);assert(/^[a-f0-9]{64}$/.test(item?.lfs?.oid));
const repo=path.join(root,'models','models--Xenova--bge-m3');
const snapshot=path.join(repo,'snapshots',revision);
fs.mkdirSync(path.join(snapshot,'onnx'),{recursive:true});
const filename=path.join(snapshot,'onnx','model_fp16.onnx');
const fd=fs.openSync(filename,'wx'),hash=crypto.createHash('sha256');
try{
  const chunk=4*1024*1024;
  for(let base=0;base<item.size;base+=chunk*4){
    const starts=Array.from({length:4},(_,i)=>base+i*chunk).filter(n=>n<item.size);
    const parts=await Promise.allSettled(starts.map(async start=>{
      const end=Math.min(start+chunk,item.size)-1;
      const response=await request(`https://huggingface.co/${repository}/resolve/${revision}/onnx/model_fp16.onnx`,{headers:{Range:`bytes=${start}-${end}`}});
      assert.equal(response.status,206);assert.equal(response.headers.get('content-range'),`bytes ${start}-${end}/${item.size}`);
      const bytes=Buffer.from(await response.arrayBuffer());assert.equal(bytes.length,end-start+1);
      return bytes;
    }));
    for(const part of parts){
      if(part.status==='rejected')throw part.reason;
      let offset=0;while(offset<part.value.length)offset+=fs.writeSync(fd,part.value,offset,part.value.length-offset);
      hash.update(part.value);
    }
    if(base%(32*1024*1024)===0)console.log(`FP16 ${Math.round(Math.min(base+chunk*4,item.size)/item.size*100)}%`);
  }
}finally{fs.closeSync(fd);}
const digest=hash.digest('hex');assert.equal(digest,item.lfs.oid);
for(const name of ['config.json','tokenizer.json','special_tokens_map.json','tokenizer_config.json']){
  const response=await request(`https://huggingface.co/${repository}/resolve/${revision}/${name}`);
  assert(response.ok,`${name}: ${response.status}`);
  const bytes=Buffer.from(await response.arrayBuffer());assert(bytes.length<100*1024*1024);
  JSON.parse(bytes);fs.writeFileSync(path.join(snapshot,name),bytes);
}
fs.mkdirSync(path.join(repo,'refs'),{recursive:true});fs.writeFileSync(path.join(repo,'refs',revision),revision);
fs.writeFileSync(path.join(root,'download-source.json'),JSON.stringify({repository,revision,bytes:item.size,sha256:digest,transport:'four bounded 4MiB ranges; SHA256 checked; native BLAKE3 validation follows'},null,2));
console.log('One FP16 artifact verified; no readiness marker fabricated');
