import fs from 'node:fs';
import crypto from 'node:crypto';
async function request(url,options={}) {
  for(let attempt=1;attempt<=4;attempt++) {
    try {
      const response=await fetch(url,{...options,signal:AbortSignal.timeout(45000)});
      if(response.status===429||response.status>=500)throw Error(`HTTP ${response.status}`);
      return response;
    } catch(error) {
      if(attempt===4)throw error;
      console.log(`Retry ${attempt}: ${error.cause?.code??error.message}`);
      await new Promise(resolve=>setTimeout(resolve,attempt*1000));
    }
  }
}
const root=process.argv[2]??'spikes/inference/data/models';
const source=JSON.parse(fs.readFileSync(`${root}/source.json`,'utf8'));
const metadata=await (await request(`https://huggingface.co/api/models/${source.repository}/tree/${source.revision}/onnx`)).json();
fs.writeFileSync(`${root}/remote-files.json`,JSON.stringify(metadata,null,2));
for(const name of source.files.map(p=>p.split('/').at(-1))){
  const item=metadata.find(f=>f.path===`onnx/${name}`);
  if(!item?.size||!item.lfs?.oid)throw Error('missing pinned size/hash');
  const dest=`${root}/ranged-${name}`;
  if(fs.existsSync(`${dest}.verified.json`)){console.log(`${name}: already verified`);continue;}
  const existing=fs.existsSync(dest)?fs.statSync(dest).size:0;
  console.log(`${name}: resume ${existing}/${item.size}`);
  if(existing>item.size||(existing!==item.size&&existing%(4*1024*1024)!==0))throw Error('partial range boundary; preserve file for inspection');
  const hash=crypto.createHash('sha256');
  if(existing){for await(const bytes of fs.createReadStream(dest)){hash.update(bytes);}}
  const fd=fs.openSync(dest,existing?'a':'wx');
  try{
    const chunk=4*1024*1024;
    for(let base=existing;base<item.size;base+=chunk*4){
      const starts=Array.from({length:4},(_,i)=>base+i*chunk).filter(start=>start<item.size);
      const parts=await Promise.allSettled(starts.map(async start=>{
        const end=Math.min(start+chunk,item.size)-1;
        const response=await request(`https://huggingface.co/${source.repository}/resolve/${source.revision}/onnx/${name}`,{headers:{Range:`bytes=${start}-${end}`}});
        if(response.status!==206||response.headers.get('content-range')!==`bytes ${start}-${end}/${item.size}`)throw Error(`invalid range response: ${response.status}`);
        const bytes=Buffer.from(await response.arrayBuffer());
        if(bytes.length!==end-start+1)throw Error('incomplete range');
        return {start,end,bytes};
      }));
      for(const part of parts){
        if(part.status==='rejected')throw part.reason;
        const {start,end,bytes}=part.value;
        let offset=0;while(offset<bytes.length)offset+=fs.writeSync(fd,bytes,offset,bytes.length-offset);
        hash.update(bytes);
        if(start%(32*1024*1024)===0)console.log(`${name} ${Math.round((end+1)/item.size*100)}%`);
      }
    }
  }finally{fs.closeSync(fd);}
  const digest=hash.digest('hex');
  if(digest!==item.lfs.oid)throw Error('SHA256 mismatch');
  fs.writeFileSync(`${dest}.verified.json`,JSON.stringify({source,bytes:item.size,sha256:digest},null,2));
  console.log(`${name}: verified ${digest}`);
}
