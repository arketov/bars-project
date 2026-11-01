import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import crypto from 'node:crypto';
import {execFileSync,execFile} from 'node:child_process';
import {promisify} from 'node:util';
const execute=promisify(execFile);
const root=path.join(process.cwd(),'docs/chats/2026-11-01-rtx-accelerators/fixture-prebuilt/tensorrt');
const url='https://developer.download.nvidia.com/compute/machine-learning/tensorrt/10.16.1/zip/TensorRT-10.16.1.11.Windows.amd64.cuda-12.9.zip';
console.error('TensorRT: archive metadata');
const head=await fetch(url,{method:'HEAD',signal:AbortSignal.timeout(30000)});
if(!head.ok)throw new Error(`TensorRT HEAD ${head.status}`);
const size=Number(head.headers.get('content-length'));
if(!Number.isSafeInteger(size)||size<65536)throw new Error('Invalid TensorRT archive size');
async function range(start,end){
  fs.mkdirSync(root,{recursive:true});
  const output=path.join(root,`range-${start}-${end}.bin`),headers=output+'.headers';
  if(end-start>32*2**20){
    const prefix=fs.existsSync(output)?fs.readFileSync(output):Buffer.alloc(0);
    if(prefix.length>end-start+1)throw new Error('Oversized partial range');
    const pieces=[];
    for(let at=start+prefix.length;at<=end;at+=8*2**20)pieces.push({start:at,end:Math.min(end,at+8*2**20-1)});
    for(let at=0;at<pieces.length;at+=8)await Promise.all(pieces.slice(at,at+8).map(async piece=>{
      const part=output+`.part-${piece.start}`,h=part+'.headers';
      await execute('curl.exe',['--fail','--location','--silent','--show-error','--retry','2','--max-time','90',
        '--range',`${piece.start}-${piece.end}`,'--dump-header',h,'--output',part,'--url',url],{windowsHide:true,timeout:280000});
      const header=fs.readFileSync(h,'utf8').toLowerCase();
      if(!header.includes(`content-range: bytes ${piece.start}-${piece.end}/${size}`)||fs.statSync(part).size!==piece.end-piece.start+1)throw new Error('Invalid range part');
      piece.file=part;
    }));
    const data=Buffer.concat([prefix,...pieces.map(p=>fs.readFileSync(p.file))]);
    if(data.length!==end-start+1)throw new Error('Truncated combined range');
    fs.writeFileSync(output,data);return data;
  }
  if(fs.existsSync(output)&&fs.statSync(output).size===end-start+1&&fs.existsSync(headers)&&fs.readFileSync(headers,'utf8').toLowerCase().includes(`content-range: bytes ${start}-${end}/${size}`))return fs.readFileSync(output);
  execFileSync('curl.exe',['--fail','--location','--silent','--show-error','--max-time','300','--range',`${start}-${end}`,
    '--dump-header',headers,'--output',output,'--url',url],{windowsHide:true,timeout:305000});
  const header=fs.readFileSync(headers,'utf8');
  if(!header.toLowerCase().includes(`content-range: bytes ${start}-${end}/${size}`))throw new Error('Unexpected archive range');
  const b=fs.readFileSync(output);if(b.length!==end-start+1)throw new Error('Truncated archive range');return b;
}
console.error('TensorRT: ZIP directory');
const tail=await range(size-65536,size-1),eocd=tail.lastIndexOf(Buffer.from([0x50,0x4b,0x05,0x06]));
if(eocd<0)throw new Error('Missing ZIP central directory');
const cdSize=tail.readUInt32LE(eocd+12),cdOffset=tail.readUInt32LE(eocd+16);
if(cdSize>16*2**20||cdOffset+cdSize>size)throw new Error('Invalid central directory');
const cd=await range(cdOffset,cdOffset+cdSize-1),entries=[];
const selected=process.argv.slice(2).filter(a=>/^nvinfer_builder_resource_sm86_10\.dll$/.test(a));
const wanted=new Set(selected.length?selected:['nvinfer_10.dll','nvinfer_plugin_10.dll','nvonnxparser_10.dll']);
for(let at=0;at<cd.length;){
  if(cd.readUInt32LE(at)!==0x02014b50)throw new Error('Invalid central directory record');
  const len=cd.readUInt16LE(at+28),extra=cd.readUInt16LE(at+30),comment=cd.readUInt16LE(at+32);
  const name=cd.toString('utf8',at+46,at+46+len).replaceAll('\\','/');
  if(wanted.has(name.split('/').at(-1))){
    let offset=cd.readUInt32LE(at+42);
    if(offset===0xffffffff){
      for(let e=at+46+len;e<at+46+len+extra;){
        const kind=cd.readUInt16LE(e),size=cd.readUInt16LE(e+2);
        if(kind===1){const skip=(cd.readUInt32LE(at+24)===0xffffffff?8:0)+(cd.readUInt32LE(at+20)===0xffffffff?8:0);offset=Number(cd.readBigUInt64LE(e+4+skip));break;}
        e+=4+size;
      }
    }
    if(!Number.isSafeInteger(offset)||offset>=size)throw new Error('Invalid ZIP64 offset');
    entries.push({name,method:cd.readUInt16LE(at+10),compressed:cd.readUInt32LE(at+20),bytes:cd.readUInt32LE(at+24),offset});
  }
  at+=46+len+extra+comment;
}
console.log(JSON.stringify({url,size,entries},null,2));
if(process.argv.includes('--list'))process.exit(0);
if(entries.length!==wanted.size||entries.some(e=>e.compressed>1024*2**20||e.bytes>2*2**30))throw new Error('Unexpected TensorRT entries');
fs.mkdirSync(root,{recursive:true});
for(const entry of entries){
  const output=path.join(root,entry.name.split('/').at(-1));
  if(fs.existsSync(output))throw new Error(`Refusing overwrite ${output}`);
  const local=await range(entry.offset,entry.offset+29);
  if(local.readUInt32LE(0)!==0x04034b50)throw new Error('Invalid local ZIP header');
  const offset=entry.offset+30+local.readUInt16LE(26)+local.readUInt16LE(28);
  const compressed=await range(offset,offset+entry.compressed-1);
  const data=entry.method===8?zlib.inflateRawSync(compressed):entry.method===0?compressed:null;
  if(!data||data.length!==entry.bytes)throw new Error('Invalid extracted size');
  fs.writeFileSync(output,data,{flag:'wx'});
  fs.writeFileSync(output+'.artifact.json',JSON.stringify({url,archive_bytes:size,whole_archive_verified:false,entry,
    sha256:crypto.createHash('sha256').update(data).digest('hex'),note:'Range extraction; verify NVIDIA Authenticode signature before loading'},null,2));
  console.log(`Extracted ${path.basename(output)}`);
}
