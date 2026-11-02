// Sequential loopback rerank; only the child created here may be terminated.
import fs from 'node:fs';
import path from 'node:path';
import net from 'node:net';
import {spawn,execFileSync} from 'node:child_process';
import {createRequire} from 'node:module';
import {performance} from 'node:perf_hooks';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import {read,inside,sha} from './freeze.mjs';

const [evidence,modelsFile,exe,name]=process.argv.slice(2);
assert(evidence&&modelsFile&&exe&&name,'evidence models.json llama-server.exe model-name');
assert(/^[a-z0-9-]+$/.test(name));
const out=inside(evidence),artifact=read(modelsFile).find(m=>m.name===name);
assert(artifact,'Known model');
const inputFile=path.join(out,'inputs.json'),bytes=fs.readFileSync(inputFile),protocol=read(path.join(out,'protocol.json'));
assert.equal(sha(bytes),protocol.inputs_sha256);
const inputs=JSON.parse(bytes);
assert.equal(inputs.length,protocol.retrieval_cases+protocol.controlled_registry_cases);
const require=createRequire(path.resolve('docs/chats/2026-11-01-rtx-accelerators/fixture-node/package.json'));
const koffi=require('koffi'),kernel=koffi.load('kernel32.dll');
const memory=kernel.func('bool __stdcall GlobalMemoryStatusEx(void *)');
function state() {
  const b=Buffer.alloc(64);b.writeUInt32LE(64);
  assert(memory(b),'GlobalMemoryStatusEx');
  const [used,free]=execFileSync('nvidia-smi',['--query-gpu=memory.used,memory.free','--format=csv,noheader,nounits'],{encoding:'utf8',windowsHide:true}).trim().split(',').map(Number);
  assert(Number.isFinite(used)&&Number.isFinite(free),'GPU telemetry');
  return {at:new Date().toISOString(),ram:Number(b.readBigUInt64LE(16)),commit:Number(b.readBigUInt64LE(32)),gpu_used_mib:used,gpu_free_mib:free};
}
async function digest(file) {
  const hash=crypto.createHash('sha256');
  for await(const part of fs.createReadStream(file))hash.update(part);
  return hash.digest('hex');
}
assert.equal(await digest(artifact.path),artifact.sha256,'Weights must match pinned hash');
const fd=fs.openSync(path.join(out,`${name}.jsonl`),'wx');
const emit=row=>fs.writeSync(fd,JSON.stringify(row)+'\n');
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
let child,exited=false,timer,abort,exit,log;
try {
  const host=state();
  emit({event:'preflight',...host});
  assert(host.ram>=4*2**30&&host.commit>=6*2**30,'Insufficient RAM/commit');
  assert(host.gpu_free_mib*2**20>artifact.bytes+2*2**30,'Insufficient VRAM');
  const listener=net.createServer();
  await new Promise((resolve,reject)=>listener.once('error',reject).listen(0,'127.0.0.1',resolve));
  const port=listener.address().port;
  await new Promise(resolve=>listener.close(resolve));
  const args=['-m',artifact.path,'--host','127.0.0.1','--port',String(port),'--reranking','--pooling','rank',
    '-c','4096','-b','4096','-ub','4096','-np','1','-ngl','99','-t','8','-tb','8','--flash-attn','off','--no-warmup'];
  log=fs.openSync(path.join(out,`${name}.server.log`),'wx');
  const runtime=[];
  for(const file of [path.resolve(exe),...fs.readdirSync(path.dirname(exe)).filter(f=>f.endsWith('.dll')).map(f=>path.resolve(path.dirname(exe),f))])runtime.push({file,sha256:await digest(file)});
  emit({event:'start',name,artifact,exe:path.resolve(exe),runtime,args,inputs_sha256:sha(bytes),runner_sha256:sha(fs.readFileSync(import.meta.filename))});
  child=spawn(path.resolve(exe),args,{windowsHide:true,stdio:['ignore',log,log]});
  exit=new Promise((resolve,reject)=>{child.once('error',reject);child.once('close',(code,signal)=>{exited=true;resolve({code,signal});});});
  // Attach rejection handler immediately; startup checks below still surface the error.
  exit.catch(e=>{abort=e.message;});
  timer=setInterval(()=>{
    if(exited)return;
    try {
      const s=state();emit({event:'resource',...s});
      if(s.ram<2**30||s.commit<2*2**30){abort='Memory guard';child.kill();}
    } catch(e){abort=e.message;child.kill();}
  },2000);
  const started=performance.now(),base=`http://127.0.0.1:${port}`;
  for(;;) {
    assert(!exited&&!abort,abort??'Server exited while loading');
    assert(performance.now()-started<180000,'Server load timeout');
    try { if((await fetch(base+'/health',{signal:AbortSignal.timeout(1000)})).ok)break; } catch {}
    await sleep(500);
  }
  emit({event:'ready',seconds:(performance.now()-started)/1000,props:await (await fetch(base+'/props')).json()});
  async function post(route,body) {
    assert(!abort&&!exited,abort??'Server exited');
    const response=await fetch(base+route,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body),signal:AbortSignal.timeout(180000)});
    const raw=await response.text();
    assert(response.ok,`HTTP ${response.status}: ${raw.slice(0,500)}`);
    return JSON.parse(raw);
  }
  function scores(response,n) {
    const items=response.results??response.data;
    assert(Array.isArray(items)&&items.length===n,'Exact result count');
    const result=Array(n),seen=new Set();
    for(const item of items) {
      assert(Number.isInteger(item.index)&&item.index>=0&&item.index<n&&!seen.has(item.index),'Unique valid result index');
      const raw=item.relevance_score;
      assert(Number.isFinite(raw),'Finite native score');
      // Encoder Rank returns one raw logit; Qwen Rank already returns P(yes).
      const score=name.startsWith('bge-')?1/(1+Math.exp(-raw)):raw;
      assert(score>=0&&score<=1,'Normalized score');
      seen.add(item.index);result[item.index]=score;
    }
    return result;
  }
  const sanity={query:'What is the capital of China?',documents:['The capital of China is Beijing.','Gravity is a force that attracts two bodies towards each other. It gives weight to physical objects and is responsible for the movement of planets around the sun.']};
  const smoke=await post('/rerank',{...sanity,top_n:2});
  const smokeScores=scores(smoke,2);
  emit({event:'sanity',...sanity,scores:smokeScores,response:smoke,published_logit_difference_qwen06:[7.625,-11.375]});
  assert(smokeScores[0]>.8&&smokeScores[1]<.2,'Positive/negative sanity');
  for(const [index,input] of inputs.entries()) {
    const queryTokens=(await post('/tokenize',{content:input.query,add_special:false})).tokens.length;
    const lengths=[];
    for(const document of input.documents) {
      const n=(await post('/tokenize',{content:document,add_special:false})).tokens.length;
      // 200 tokens exceed the fixed Rank prompt overhead; reject instead of silent cropping.
      assert(n+queryTokens+200<=4096,`Pair exceeds safe context ${input.id}`);
      lengths.push(n);
    }
    const begin=performance.now();
    const response=await post('/rerank',{query:input.query,documents:input.documents,top_n:input.documents.length});
    emit({event:'row',id:input.id,scores:scores(response,input.documents.length),seconds:(performance.now()-begin)/1000,
      raw_response:response,document_tokens:lengths,query_tokens:queryTokens,usage:response.usage??null});
    if(index%10===0)console.log(`${name} ${index+1}/${inputs.length}`);
  }
  emit({event:'complete',name,rows:inputs.length});
} catch(e) {
  emit({event:'error',error:e.message});process.exitCode=1;console.error(`${name}: ${e.message}`);
} finally {
  clearInterval(timer);
  if(child&&!exited)child.kill();
  if(exit) {try {emit({event:'exit',...await exit,guard:abort??null});}catch(e){emit({event:'exit_error',error:e.message});process.exitCode=1;}}
  if(log!==undefined)fs.closeSync(log);
  fs.closeSync(fd);
}
