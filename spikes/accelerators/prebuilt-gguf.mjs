import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import net from 'node:net';
import {spawn,execFileSync} from 'node:child_process';
import {createRequire} from 'node:module';
import {performance} from 'node:perf_hooks';

const root=process.cwd();
const session=path.join(root,'docs/chats/2026-11-01-rtx-accelerators');
const fixture=process.env.BARS_PROBE_RUNS_DIR??path.join(session,'fixture-runs');
const require=createRequire(path.join(session,'fixture-node/package.json'));
const koffi=require('koffi');
const kernel=koffi.load('kernel32.dll');
const psapi=koffi.load('psapi.dll');
const openProcess=kernel.func('void * __stdcall OpenProcess(uint32_t, bool, uint32_t)');
const closeHandle=kernel.func('bool __stdcall CloseHandle(void *)');
const memoryStatus=kernel.func('bool __stdcall GlobalMemoryStatusEx(void *)');
const processMemory=psapi.func('bool __stdcall GetProcessMemoryInfo(void *, void *, uint32_t)');
const getTimes=kernel.func('bool __stdcall GetProcessTimes(void *,void *,void *,void *,void *)');
function processCpu(handle){const b=Array.from({length:4},()=>Buffer.alloc(8));if(!getTimes(handle,...b))throw new Error('Process CPU sample failed');return Number(b[2].readBigUInt64LE()+b[3].readBigUInt64LE())/1e7;}
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
function system() {
  const mem=Buffer.alloc(64); mem.writeUInt32LE(64);
  if(!memoryStatus(mem)) throw new Error('GlobalMemoryStatusEx failed');
  return {available_ram:Number(mem.readBigUInt64LE(16)),commit_headroom:Number(mem.readBigUInt64LE(32))};
}
function cpu() {
  const cpus=os.cpus();
  return cpus.reduce((r,c)=>({idle:r.idle+c.times.idle,total:r.total+Object.values(c.times).reduce((a,b)=>a+b,0)}),{idle:0,total:0});
}
function sample(handle,previous) {
  const mem=Buffer.alloc(80); mem.writeUInt32LE(80);
  if(!processMemory(handle,mem,80)) throw new Error('GetProcessMemoryInfo failed');
  const now=cpu(); const total=now.total-previous.total;
  now.own=processCpu(handle);now.at=Date.now();
  const busy=total>0?100*(1-(now.idle-previous.idle)/total):0;
  const own=previous.own===undefined?null:100*(now.own-previous.own)/(os.cpus().length*(now.at-previous.at)/1000);
  return {now,value:{at:new Date().toISOString(),...system(),private:Number(mem.readBigUInt64LE(72)),working_set:Number(mem.readBigUInt64LE(16)),
    cpu_total_percent:busy,cpu_process_percent:own,cpu_background_percent:own===null?null:Math.max(0,busy-own),
    gpu:execFileSync('nvidia-smi',['--query-gpu=memory.used,utilization.gpu,power.draw,temperature.gpu','--format=csv,noheader,nounits'],{encoding:'utf8',windowsHide:true}).trim()}};
}
async function freePort() {
  const server=net.createServer();
  await new Promise((resolve,reject)=>server.once('error',reject).listen(0,'127.0.0.1',resolve));
  const port=server.address().port;
  await new Promise(resolve=>server.close(resolve));
  return port;
}
const names=process.argv.slice(2);
if(!names.length) throw new Error('Explicit config names required');
for(const name of names) {
  if(!/^[A-Za-z0-9-]+$/.test(name)) throw new Error('Invalid name');
  const cfg=JSON.parse(fs.readFileSync(path.join(fixture,`${name}.json`)));
  if(cfg.name!==name||cfg.kind!=='gguf'||!['CUDA','Vulkan'].includes(cfg.backend)||![128,512,2048].includes(cfg.batch)||
    !['f16','q8_0','q4_0'].includes(cfg.kv)||typeof cfg.flash!=='boolean'||!Number.isInteger(cfg.repeats)||cfg.repeats<1||cfg.repeats>20)throw new Error('Invalid GGUF configuration');
  const inputs=JSON.parse(fs.readFileSync(cfg.inputs));
  if(!inputs.length||inputs.some(i=>typeof i.query!=='string'||!Array.isArray(i.documents)||typeof i.documents[0]!=='string'))throw new Error('Invalid GGUF inputs');
  const out=path.join(fixture,`${name}.prebuilt.jsonl`);
  const fd=fs.openSync(out,'wx');
  const emit=row=>fs.writeSync(fd,JSON.stringify(row)+'\n');
  let host=system();
  const waiting=performance.now();
  while(host.available_ram<4*2**30 || host.commit_headroom<6*2**30) {
    emit({event:'preflight_wait',...host});
    if(performance.now()-waiting>120000){emit({event:'error',error:'Insufficient memory before model'});fs.closeSync(fd);throw new Error('Insufficient memory before model');}
    await sleep(2000);host=system();
  }
  const backend=cfg.backend.toLowerCase();
  const exe=path.join(session,`fixture-prebuilt/${backend}/llama-server.exe`);
  const port=await freePort();
  const args=['-m',cfg.model,'--host','127.0.0.1','--port',String(port),'--embedding','--pooling','last',
    '--embd-normalize','2','-c','8192','-b','8192','-ub',String(cfg.batch),'-t','8','-tb','8','-ngl','999',
    '--parallel','1','--flash-attn',cfg.flash?'on':'off','--cache-type-k',cfg.kv,'--cache-type-v',cfg.kv,
    '--device',backend==='cuda'?'CUDA0':'Vulkan0','--no-warmup','--verbose'];
  const log=fs.openSync(path.join(fixture,`${name}.prebuilt.stderr.log`),'wx');
  const child=spawn(exe,args,{windowsHide:true,stdio:['ignore',log,log]});
  let spawnError; child.on('error',e=>{spawnError=e;});
  const exit=new Promise(resolve=>child.once('close',(code,signal)=>resolve({code,signal})));
  let exited=false; exit.then(()=>{exited=true;});
  let handle,previous=cpu(),abort,monitor;
  const start=performance.now();
  try {
    emit({event:'start',name,exe,args,host,measurement:'loopback HTTP end-to-end; separate process per config'});
    if(!child.pid) throw new Error('Server did not start');
    handle=openProcess(0x410,false,child.pid);
    if(!handle) throw new Error('OpenProcess failed');
    monitor=setInterval(()=>{
      try {
        if(exited)return;
        const s=sample(handle,previous);previous=s.now;
        fs.appendFileSync(path.join(fixture,`${name}.prebuilt.monitor.jsonl`),JSON.stringify(s.value)+'\n');
        if(s.value.available_ram<2**30 || s.value.commit_headroom<2*2**30) {abort='memory guard';child.kill();}
      }catch(e){abort=e.message;if(!exited)child.kill();}
    },2000);
    const base=`http://127.0.0.1:${port}`;
    for(;;) {
      if(spawnError)throw spawnError;
      if(exited)throw new Error('Server exited during load');
      if(abort)throw new Error(abort);
      if(performance.now()-start>180000)throw new Error('Server load timeout');
      try {if((await fetch(base+'/health',{signal:AbortSignal.timeout(1000)})).ok)break;}catch{}
      await sleep(500);
    }
    emit({event:'ready',seconds:(performance.now()-start)/1000});
    const texts=inputs.flatMap(i=>[
      {id:i.id+'-query',text:'Instruct: Given a web search query, retrieve relevant passages that answer the query\nQuery: '+i.query},
      {id:i.id+'-doc',text:i.documents[0]}
    ]);
    for(let repeat=0;repeat<=cfg.repeats;repeat++) {
      for(const {id,text} of texts) {
        if(abort)throw new Error(abort);
        const begin=performance.now();
        const response=await fetch(base+'/v1/embeddings',{method:'POST',headers:{'Content-Type':'application/json'},
          body:JSON.stringify({input:text,encoding_format:'float',cache_prompt:false}),signal:AbortSignal.timeout(120000)});
        const raw=await response.text();
        if(!response.ok)throw new Error(`HTTP ${response.status}: ${raw.slice(0,500)}`);
        const data=JSON.parse(raw);
        const vector=data.data?.[0]?.embedding ?? data[0]?.embedding;
        if(!Array.isArray(vector)||vector.length!==2560||!vector.every(Number.isFinite)) {
          emit({event:'invalid_response',response:data});
          throw new Error('Invalid embedding output');
        }
        const norm=Math.sqrt(vector.reduce((s,v)=>s+v*v,0));
        if(Math.abs(norm-1)>1e-3)throw new Error(`Non-normalized embedding: ${norm}`);
        emit({event:'row',id,repeat,warmup:repeat===0,seconds:(performance.now()-begin)/1000,vector,usage:data.usage,timings:data.timings});
      }
    }
    emit({event:'complete',name});
    console.log(`${name}: complete`);
  } catch(e) {
    process.exitCode=1;
    emit({event:'error',error:e.message});
    console.error(`${name}: ${e.message}`);
  } finally {
    clearInterval(monitor);
    if(!exited)child.kill();
    const result=await exit;
    if(handle)closeHandle(handle);
    emit({event:'process_exit',...result,guard:abort??null});
    fs.closeSync(log);fs.closeSync(fd);
  }
}
