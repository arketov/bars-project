import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {createRequire} from 'node:module';
import {spawn,execFileSync} from 'node:child_process';
const session=path.join(process.cwd(),'docs/chats/2026-11-01-rtx-accelerators');
const fixture=process.env.BARS_PROBE_RUNS_DIR??path.join(session,'fixture-runs');
const require=createRequire(path.join(session,'fixture-node/package.json'));
const koffi=require('koffi'),kernel=koffi.load('kernel32.dll'),psapi=koffi.load('psapi.dll');
const open=kernel.func('void * __stdcall OpenProcess(uint32_t,bool,uint32_t)');
const close=kernel.func('bool __stdcall CloseHandle(void *)');
const mem=kernel.func('bool __stdcall GlobalMemoryStatusEx(void *)');
const pmem=psapi.func('bool __stdcall GetProcessMemoryInfo(void *,void *,uint32_t)');
const getTimes=kernel.func('bool __stdcall GetProcessTimes(void *,void *,void *,void *,void *)');
const processCpu=handle=>{const b=Array.from({length:4},()=>Buffer.alloc(8));if(!getTimes(handle,...b))throw new Error('Process CPU sample failed');return Number(b[2].readBigUInt64LE()+b[3].readBigUInt64LE())/1e7;};
const cpu=()=>os.cpus().reduce((r,c)=>({idle:r.idle+c.times.idle,total:r.total+Object.values(c.times).reduce((a,b)=>a+b,0)}),{idle:0,total:0});
const state=()=>{const b=Buffer.alloc(64);b.writeUInt32LE(64);if(!mem(b))throw new Error('Memory sample failed');return{available_ram:Number(b.readBigUInt64LE(16)),commit_headroom:Number(b.readBigUInt64LE(32))};};
const [dll,...names]=process.argv.slice(2);
if(!dll||!names.length)throw new Error('Runtime DLL and config names required');
for(const name of names){
  if(!/^[A-Za-z0-9-]+$/.test(name))throw new Error('Invalid name');
  const memory=state();if(memory.available_ram<4*2**30||memory.commit_headroom<6*2**30){
    fs.writeFileSync(path.join(fixture,`${name}.ffi.preflight.json`),JSON.stringify({name,at:new Date().toISOString(),...memory,minimum_ram:4*2**30,minimum_commit:6*2**30,started:false}),{flag:'wx'});
    throw new Error('Insufficient memory before ONNX');
  }
  const stderr=fs.openSync(path.join(fixture,`${name}.ffi.stderr.log`),'wx');
  const child=spawn(process.execPath,['spikes/accelerators/prebuilt-onnx.mjs',name,dll],{windowsHide:true,stdio:['ignore','inherit',stderr],
    env:{...process.env,PATH:path.join(session,'fixture-prebuilt/tensorrt')+';'+path.join(session,'fixture-prebuilt/cudnn-loader')+';C:/bt/bars/release;'+process.env.PATH}});
  if(!child.pid)throw new Error('ONNX child did not start');
  const handle=open(0x410,false,child.pid);if(!handle){child.kill();throw new Error('OpenProcess failed');}
  let previous=cpu(),guard=null,exited=false;
  const exit=new Promise((resolve,reject)=>{child.once('error',reject);child.once('close',(code,signal)=>{exited=true;resolve({code,signal});});});
  const timer=setInterval(()=>{
    if(exited)return;
    try{
      const b=Buffer.alloc(80);b.writeUInt32LE(80);if(!pmem(handle,b,80)){if(child.exitCode!==null)return;throw new Error('Process memory sample failed');}
      const now=cpu(),total=now.total-previous.total;
      now.own=processCpu(handle);now.at=Date.now();
      const busy=total>0?100*(1-(now.idle-previous.idle)/total):0;
      const own=previous.own===undefined?null:100*(now.own-previous.own)/(os.cpus().length*(now.at-previous.at)/1000);
      const row={at:new Date().toISOString(),...state(),private:Number(b.readBigUInt64LE(72)),working_set:Number(b.readBigUInt64LE(16)),
        cpu_total_percent:busy,cpu_process_percent:own,cpu_background_percent:own===null?null:Math.max(0,busy-own),
        gpu:execFileSync('nvidia-smi',['--query-gpu=memory.used,utilization.gpu,power.draw,temperature.gpu','--format=csv,noheader,nounits'],{encoding:'utf8',windowsHide:true}).trim()};
      previous=now;fs.appendFileSync(path.join(fixture,`${name}.ffi.monitor.jsonl`),JSON.stringify(row)+'\n');
      if(row.available_ram<2**30||row.commit_headroom<2*2**30){guard='memory';child.kill();}
    }catch(e){guard=e.message;child.kill();}
  },2000);
  const timeout=setTimeout(()=>{guard='timeout';child.kill();},1800000);
  try{const result=await exit;fs.writeFileSync(path.join(fixture,`${name}.ffi.process.json`),JSON.stringify({...result,guard,memory}));
    if(result.code!==0||result.signal!==null)process.exitCode=1;
  }
  finally{clearInterval(timer);clearTimeout(timeout);if(!exited)child.kill();close(handle);fs.closeSync(stderr);}
  if(guard)throw new Error(`Aborted ${name}: ${guard}`);
}
