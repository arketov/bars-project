import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {gzipSync,gunzipSync} from 'node:zlib';
const [mode,source,target,manifestPath]=process.argv.slice(2);
const digest=b=>crypto.createHash('sha256').update(b).digest('hex').toUpperCase();
function within(root,name){
  const relative=name.replaceAll('\\','/');
  if(path.posix.isAbsolute(relative)||relative.split('/').some(s=>s==='..'||s==='')||/[:\0]/.test(relative))throw Error('Unsafe corpus path');
  const out=path.resolve(root,relative);
  if(!out.startsWith(path.resolve(root)+path.sep))throw Error('Corpus path escaped root');
  return out;
}
if(mode==='pack'){
  const manifest=JSON.parse(fs.readFileSync(manifestPath));
  const files=manifest.map(row=>{
    const bytes=fs.readFileSync(within(source,row.file));
    if(digest(bytes)!==row.sha256)throw Error('Frozen input changed: '+row.file);
    return {...row,text:bytes.toString('utf8')};
  });
  fs.writeFileSync(target,gzipSync(JSON.stringify({schema:1,files})),{flag:'wx'});
  console.log(`Frozen ${files.length} documents`);
}else if(mode==='unpack'){
  const bundle=JSON.parse(gunzipSync(fs.readFileSync(source)));
  if(bundle.schema!==1||!Array.isArray(bundle.files))throw Error('Invalid corpus schema');
  if(fs.existsSync(target))throw Error('Corpus destination exists');
  // Validate every path/hash before creating the destination.
  for(const row of bundle.files){within(target,row.file);if(digest(Buffer.from(row.text))!==row.sha256)throw Error('Corpus digest mismatch');}
  fs.mkdirSync(target);
  for(const row of bundle.files){const file=within(target,row.file);fs.mkdirSync(path.dirname(file),{recursive:true});fs.writeFileSync(file,row.text,{flag:'wx'});}
  fs.writeFileSync(manifestPath,JSON.stringify(bundle.files.map(({file,sha256})=>({file,sha256})),null,2)+'\n');
  console.log(`Restored ${bundle.files.length} verified documents`);
}else if(mode==='select-longest'){
  const bundle=JSON.parse(gunzipSync(fs.readFileSync(source)));
  if(bundle.schema!==1||!Array.isArray(bundle.files))throw Error('Invalid corpus schema');
  for(const row of bundle.files){within('corpus',row.file);if(digest(Buffer.from(row.text))!==row.sha256)throw Error('Corpus digest mismatch');}
  const files=bundle.files.toSorted((a,b)=>b.text.length-a.text.length||(a.file<b.file?-1:a.file>b.file?1:0)).slice(0,48);
  fs.writeFileSync(target,gzipSync(JSON.stringify({schema:1,files})),{flag:'wx'});
  fs.writeFileSync(manifestPath,JSON.stringify({method:'48 longest documents by Unicode UTF-16 length; path ascending breaks ties',source_sha256:digest(fs.readFileSync(source)),source_count:bundle.files.length,selected_count:files.length,source_characters:bundle.files.reduce((s,r)=>s+r.text.length,0),selected_characters:files.reduce((s,r)=>s+r.text.length,0),files:files.map(({file,sha256,text})=>({file,sha256,characters:text.length}))},null,2)+'\n',{flag:'wx'});
  console.log(`Selected ${files.length} complete documents`);
}else throw Error('Expected pack|unpack|select-longest <source> <target> <manifest>');
