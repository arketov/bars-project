// Fetch only the signed cuDNN loader from NVIDIA's large redistributable ZIP.
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import crypto from 'node:crypto';
const root=path.join(process.cwd(),'docs/chats/2026-11-01-rtx-accelerators/fixture-prebuilt/cudnn-loader');
fs.mkdirSync(root,{recursive:true});
const manifestUrl='https://developer.download.nvidia.com/compute/cudnn/redist/redistrib_9.23.0.json';
const manifest=await fetch(manifestUrl).then(r=>{if(!r.ok)throw new Error(`manifest ${r.status}`);return r.json();});
const item=manifest.cudnn['windows-x86_64'].cuda12;
const url='https://developer.download.nvidia.com/compute/cudnn/redist/'+item.relative_path;
const size=Number(item.size);
async function range(start,end){
  const r=await fetch(url,{headers:{Range:`bytes=${start}-${end}`}});
  if(r.status!==206||r.headers.get('content-range')!==`bytes ${start}-${end}/${size}`)throw new Error('Unexpected ZIP range');
  const b=Buffer.from(await r.arrayBuffer());if(b.length!==end-start+1)throw new Error('Truncated ZIP range');return b;
}
const tail=await range(size-65536,size-1),eocd=tail.lastIndexOf(Buffer.from([0x50,0x4b,0x05,0x06]));
if(eocd<0)throw new Error('Missing ZIP central directory');
const cdSize=tail.readUInt32LE(eocd+12),cdOffset=tail.readUInt32LE(eocd+16);
if(cdSize>16*2**20||cdOffset+cdSize>size)throw new Error('Invalid central directory');
const cd=await range(cdOffset,cdOffset+cdSize-1);
const target=process.argv.find(a=>/^cudnn\w*_9\.dll$/.test(a))??'cudnn64_9.dll';
let found;
for(let at=0;at<cd.length;){
  if(cd.readUInt32LE(at)!==0x02014b50)throw new Error('Invalid central directory record');
  const len=cd.readUInt16LE(at+28),extra=cd.readUInt16LE(at+30),comment=cd.readUInt16LE(at+32);
  const name=cd.toString('utf8',at+46,at+46+len);
  if(name.replaceAll('\\','/').endsWith('/bin/x64/'+target))found={name,method:cd.readUInt16LE(at+10),compressed:cd.readUInt32LE(at+20),bytes:cd.readUInt32LE(at+24),offset:cd.readUInt32LE(at+42)};
  if(process.argv.includes('--list')&&name.endsWith('.dll'))console.log({name,compressed:cd.readUInt32LE(at+20)});
  at+=46+len+extra+comment;
}
if(process.argv.includes('--list'))process.exit(0);
if(!found||found.compressed>512*2**20||found.bytes>2*2**30)throw new Error('Unexpected cuDNN library');
const local=await range(found.offset,found.offset+29);
if(local.readUInt32LE(0)!==0x04034b50)throw new Error('Invalid local ZIP header');
const offset=found.offset+30+local.readUInt16LE(26)+local.readUInt16LE(28);
const compressed=await range(offset,offset+found.compressed-1);
const data=found.method===8?zlib.inflateRawSync(compressed):found.method===0?compressed:null;
if(!data||data.length!==found.bytes)throw new Error('Invalid uncompressed cuDNN loader');
fs.writeFileSync(path.join(root,target),data,{flag:'wx'});
fs.writeFileSync(path.join(root,target+'.artifact.json'),JSON.stringify({manifestUrl,url,archive_manifest_sha256:item.sha256,
  whole_archive_verified:false,entry:found,sha256:crypto.createHash('sha256').update(data).digest('hex'),
  note:'Range extraction; verify NVIDIA Authenticode signature before loading'},null,2));
console.log(`Extracted ${data.length} bytes; signature verification required`);
