// Reuse the already-reviewed ORT C API bridge; preserve its raw logits and guarded process result.
import fs from 'node:fs';
import path from 'node:path';
import {spawn,execFileSync} from 'node:child_process';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {read,write,inside,sha} from './freeze.mjs';

const [directory,dll,model,tokenizer]=process.argv.slice(2);
assert(directory&&dll&&model&&tokenizer,'evidence runtime.dll model_fp16.onnx tokenizer.json');
const out=inside(directory),name='onnx-directml-fp16',runs=path.join(out,'onnx-runs');
fs.mkdirSync(runs);
const inputFile=path.join(out,'inputs.json'),protocol=read(path.join(out,'protocol.json'));
assert.equal(sha(fs.readFileSync(inputFile)),protocol.inputs_sha256);
const inputs=read(inputFile);
const cfg={name,kind:'onnx',backend:'directml',model:path.resolve(model),tokenizer:path.resolve(tokenizer),inputs:inputFile,
  batch:4,repeats:1,binding:false,fixed:false,graph:false,profile:false};
write(path.join(runs,name+'.json'),cfg);
const free=Number(execFileSync('nvidia-smi',['--query-gpu=memory.free','--format=csv,noheader,nounits'],{encoding:'utf8',windowsHide:true}).trim());
const preflight={at:new Date().toISOString(),gpu_free_mib:free,model_bytes:fs.statSync(model).size,reserve_bytes:2*2**30};
write(path.join(runs,'vram-preflight.json'),preflight);
assert(Number.isFinite(free)&&free*2**20>preflight.model_bytes+preflight.reserve_bytes,'Insufficient VRAM');
const child=spawn(process.execPath,['spikes/accelerators/run-onnx.mjs',path.resolve(dll),name],{
  env:{...process.env,BARS_PROBE_RUNS_DIR:runs},windowsHide:true,stdio:'inherit'});
const code=await new Promise((resolve,reject)=>{child.once('error',reject);child.once('close',resolve);});
assert.equal(code,0,'Guarded ORT process must succeed');
const result=read(path.join(runs,name+'.ffi.process.json'));
assert.equal(result.code,0);assert.equal(result.guard,null);
const raw=fs.readFileSync(path.join(runs,name+'.ffi.jsonl'),'utf8').trim().split('\n').map(JSON.parse);
assert.equal(raw.filter(r=>r.event==='complete').length,1);
assert(!raw.some(r=>r.event==='error'));
const actual=raw.filter(r=>r.event==='row'&&!r.warmup);
assert.equal(actual.length,inputs.length);
async function digest(file){const hash=crypto.createHash('sha256');for await(const part of fs.createReadStream(file))hash.update(part);return hash.digest('hex');}
const artifacts=[];
for(const file of [dll,model,tokenizer,path.join(path.dirname(tokenizer),'tokenizer_config.json'),
  'spikes/accelerators/ort-api-layout.json','spikes/accelerators/prebuilt-onnx.mjs','spikes/accelerators/run-onnx.mjs'])artifacts.push({file:path.resolve(file),sha256:await digest(file)});
const rows=[{event:'start',name,inputs_sha256:protocol.inputs_sha256,cfg,artifacts},...actual.map(r=>({event:'row',id:r.id,scores:r.logits.map(v=>1/(1+Math.exp(-v))),logits:r.logits,seconds:r.seconds,shapes:r.shapes})),
  {event:'complete',name,rows:actual.length},{event:'exit',...result}];
fs.writeFileSync(path.join(out,name+'.jsonl'),rows.map(r=>JSON.stringify(r)).join('\n')+'\n',{flag:'wx'});
