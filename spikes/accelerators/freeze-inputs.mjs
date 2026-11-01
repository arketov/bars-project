import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import crypto from 'node:crypto';
import {DatabaseSync} from 'node:sqlite';

const root = process.cwd();
const evidence = process.argv[2] && path.resolve(process.argv[2]);
if(!evidence||!evidence.toLowerCase().startsWith((root+path.sep).toLowerCase())||fs.existsSync(evidence))throw new Error('A new output directory inside this checkout is required');
fs.mkdirSync(evidence, {recursive:true});
const read = file => JSON.parse(zlib.gunzipSync(fs.readFileSync(file)));
const full = read('docs/knowledge/memory-acceptance-2026-10-20/data-bars-inputs.json.gz');
if (!Array.isArray(full) || full.length !== 24) throw new Error('Expected frozen 24-question bars corpus');
const source = path.join(process.env.LOCALAPPDATA,'bars/bench-quality/bars-2026-10-18/index.sqlite');
const db = new DatabaseSync(source,{readOnly:true});
const chunk = db.prepare('SELECT c.text,c.heading_chain,c.first_line,c.last_line,d.path FROM chunks c JOIN documents d ON d.id=c.document_id WHERE c.id=?');
for(const input of full) {
  input.documents=input.hits.map(hit=>{
    const row=chunk.get(hit.id);
    if(!row||row.path!==hit.path||row.heading_chain!==hit.heading||row.first_line!==hit.first||row.last_line!==hit.last)throw new Error(`Frozen candidate mismatch: ${input.id}/${hit.id}`);
    return `${row.path}\n${row.heading_chain}\n${row.text}`;
  });
  if(!input.documents.length)throw new Error('Empty candidates');
}
db.close();
fs.writeFileSync(path.join(evidence,'inputs.json'), JSON.stringify(full,null,2));
const chosen = [full[0],full[6],full[12],full[18]];
fs.writeFileSync(path.join(evidence,'gguf-inputs.json'), JSON.stringify(chosen,null,2));
fs.writeFileSync(path.join(evidence,'fixed-inputs.json'), JSON.stringify(chosen.map(i=>({...i,documents:i.documents.slice(0,4)})),null,2));
const models = path.join(process.env.USERPROFILE,'.bars/models');
const gguf = path.join(models,'models--Qwen--Qwen3-Embedding-4B-GGUF/blobs/2b0cf8f17b4c723c27303015383c27ec4bf2d8314bb677d05e920dd70bb0f16b');
const onnx = path.join(models,'models--onnx-community--bge-reranker-v2-m3-ONNX/snapshots/6f5ff65298512715a1e669753bc754d2bc8f367b/onnx/model_fp16.onnx');
const tokenizer = path.join(models,'models--rozgo--bge-reranker-v2-m3/snapshots/fbd57b17b4db111a9d16813bb08b4c804fac18e9/tokenizer.json');
async function sha(file) {
  const hash=crypto.createHash('sha256');
  for await (const chunk of fs.createReadStream(file)) hash.update(chunk);
  return hash.digest('hex');
}
const artifacts=[];
for (const [role,file] of Object.entries({gguf,onnx,tokenizer})) {
  console.log(`Hashing ${role}`);
  artifacts.push({role,file,sha256:await sha(file),bytes:fs.statSync(file).size});
  console.log(`Hashed ${role}`);
}
if (artifacts[0].sha256 !== path.basename(gguf)) throw new Error('GGUF digest mismatch');
fs.writeFileSync(path.join(evidence,'artifacts.json'),JSON.stringify(artifacts,null,2));
console.log(JSON.stringify({cases:full.length,artifacts},null,2));
