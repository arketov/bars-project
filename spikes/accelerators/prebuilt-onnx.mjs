// Research-only C API bridge: pinned precompiled Koffi and ONNX Runtime, no compilation.
import fs from 'node:fs';
import path from 'node:path';
import {createRequire} from 'node:module';
import {performance} from 'node:perf_hooks';
import crypto from 'node:crypto';
import {fileURLToPath} from 'node:url';
const root=process.cwd(), session=path.join(root,'docs/chats/2026-11-01-rtx-accelerators');
const fixture=process.env.BARS_PROBE_RUNS_DIR??path.join(session,'fixture-runs');
const require=createRequire(path.join(session,'fixture-node/package.json'));
const koffi=require('koffi');
const {Tokenizer}=require('@huggingface/tokenizers');
const name=process.argv[2];
if(!name||!/^[A-Za-z0-9-]+$/.test(name))throw new Error('Config name required');
const cfg=JSON.parse(fs.readFileSync(path.join(fixture,`${name}.json`)));
if(cfg.name!==name||cfg.kind!=='onnx'||!['cuda','directml','tensorrt'].includes(cfg.backend)||
  !Number.isInteger(cfg.batch)||cfg.batch<1||cfg.batch>64||!Number.isInteger(cfg.repeats)||cfg.repeats<1||cfg.repeats>20)throw new Error('Invalid ONNX configuration');
const dll=process.argv[3];
if(!dll||!path.isAbsolute(dll))throw new Error('Absolute runtime DLL required');
const fd=fs.openSync(path.join(fixture,`${name}.ffi.jsonl`),'wx');
const emit=row=>fs.writeSync(fd,JSON.stringify(row)+'\n');
const lib=koffi.load(dll);
const base=lib.func('void * __stdcall OrtGetApiBase()')();
const [getApi,getVersion]=koffi.decode(base,'void *',2);
const call=(ptr,signature,...args)=>koffi.call(ptr,koffi.proto(signature),...args);
const version=call(getVersion,'const char * __stdcall Version()');
const api=call(getApi,'void * __stdcall GetApi(uint32_t)',24);
if(!api)throw new Error('C API 24 unavailable');
// Derive ABI offsets from the pinned binding; duplicate wasm declarations occupy one slot.
const layout=JSON.parse(fs.readFileSync(new URL('./ort-api-layout.json',import.meta.url)));
if(layout.api_version!==24||layout.pointer_size!==8||process.arch!=='x64')throw new Error('Requires x64 C API 24 layout');
const names=layout.fields;
const functions={};
function bind(n,args,ret='void *') {
  const index=names.indexOf(n);
  if(index<0)throw new Error(`Unknown API ${n}`);
  const ptr=koffi.decode(api,index*8,'void *');
  const prototype=koffi.proto(`${ret} __stdcall ${n}(${args})`);
  functions[n]=(...values)=>koffi.call(ptr,prototype,...values);
}
bind('GetErrorMessage','void *','str');bind('ReleaseStatus','void *','void');
for(const [n,args] of Object.entries({
  CreateEnv:'int,str,_Out_ void **',
  CreateSessionOptions:'_Out_ void **',
  SetSessionGraphOptimizationLevel:'void *,int',SetIntraOpNumThreads:'void *,int',
  DisableMemPattern:'void *',SetSessionExecutionMode:'void *,int',
  CreateSession:'void *,str16,void *,_Out_ void **',
  CreateCpuMemoryInfo:'int,int,_Out_ void **',
  CreateMemoryInfo:'str,int,int,int,_Out_ void **',
  CreateTensorWithDataAsOrtValue:'void *,void *,size_t,void *,size_t,int,_Out_ void **',
  Run:'void *,void *,str *,void **,size_t,str *,size_t,_Out_ void **',
  GetTensorMutableData:'void *,_Out_ void **',
  GetTensorTypeAndShape:'void *,_Out_ void **',GetTensorElementType:'void *,_Out_ int *',GetTensorShapeElementCount:'void *,_Out_ size_t *',
  CreateCUDAProviderOptions:'_Out_ void **',
  UpdateCUDAProviderOptions:'void *,str *,str *,size_t',
  SessionOptionsAppendExecutionProvider_CUDA_V2:'void *,void *',
  CreateTensorRTProviderOptions:'_Out_ void **',
  UpdateTensorRTProviderOptions:'void *,str *,str *,size_t',
  SessionOptionsAppendExecutionProvider_TensorRT_V2:'void *,void *',
  CreateIoBinding:'void *,_Out_ void **',BindInput:'void *,str,void *',BindOutput:'void *,str,void *',
  RunWithBinding:'void *,void *,void *',SynchronizeBoundInputs:'void *',SynchronizeBoundOutputs:'void *',
  EnableProfiling:'void *,str16',SessionEndProfiling:'void *,void *,_Out_ void **',GetAllocatorWithDefaultOptions:'_Out_ void **',
  AllocatorFree:'void *,void *',
}))bind(n,args);
for(const n of ['Env','SessionOptions','Session','MemoryInfo','Value','CUDAProviderOptions','TensorRTProviderOptions','IoBinding','TensorTypeAndShapeInfo'])bind('Release'+n,'void *','void');
function check(status){if(status){const message=functions.GetErrorMessage(status);functions.ReleaseStatus(status);throw new Error(message);}}
function pointer(n,...args){const out=[null];check(functions[n](...args,out));if(!out[0])throw new Error(`Null ${n}`);return out[0];}
const owned=[];
function own(type,n,...args){const p=pointer(n,...args);owned.push(()=>functions['Release'+type](p));return p;}
const cpuInfo=own('MemoryInfo','CreateCpuMemoryInfo',1,0);
function tensor(info,data,shape,type){return pointer('CreateTensorWithDataAsOrtValue',info,data,data.byteLength??shape.reduce((a,b)=>a*b,1)*(type===7?8:4),new BigInt64Array(shape.map(BigInt)),shape.length,type);}
function encoding(tokenizer,query,document){
  const a=tokenizer.encode(query,{add_special_tokens:false}).ids;
  const b=tokenizer.encode(document,{add_special_tokens:false}).ids;
  while(a.length+b.length>508){if(a.length>b.length)a.pop();else b.pop();}
  return [0,...a,2,2,...b,2];
}
let cudaLib, cudaFree, cudaMemcpy, sync, buffers;
try {
  emit({event:'start',name,version,dll,api:24,source_sha256:crypto.createHash('sha256').update(fs.readFileSync(fileURLToPath(import.meta.url))).digest('hex'),
    tokenizer:'@huggingface/tokenizers 0.2.0; explicit XLM-R longest-first 512'});
  const env=own('Env','CreateEnv',2,'bars-accelerators');
  const opts=own('SessionOptions','CreateSessionOptions');
  check(functions.SetSessionGraphOptimizationLevel(opts,99));check(functions.SetIntraOpNumThreads(opts,8));
  if(cfg.backend==='directml') {
    check(functions.DisableMemPattern(opts));check(functions.SetSessionExecutionMode(opts,0));
    check(lib.func('void * __stdcall OrtSessionOptionsAppendExecutionProvider_DML(void *,int)')(opts,0));
  } else {
    if(cfg.backend==='tensorrt') {
      const trt=own('TensorRTProviderOptions','CreateTensorRTProviderOptions');
      fs.mkdirSync(cfg.cache,{recursive:true});
      const values={trt_fp16_enable:'1',trt_engine_cache_enable:'1',trt_engine_cache_path:cfg.cache,trt_max_workspace_size:String(2*2**30),
        trt_profile_min_shapes:'input_ids:1x1,attention_mask:1x1',trt_profile_opt_shapes:'input_ids:4x512,attention_mask:4x512',trt_profile_max_shapes:'input_ids:16x512,attention_mask:16x512'};
      check(functions.UpdateTensorRTProviderOptions(trt,Object.keys(values),Object.values(values),Object.keys(values).length));
      check(functions.SessionOptionsAppendExecutionProvider_TensorRT_V2(opts,trt));
    } else if(cfg.backend!=='cuda')throw new Error('Unknown backend');
    const cuda=own('CUDAProviderOptions','CreateCUDAProviderOptions');
    const values={device_id:'0',use_tf32:'0',enable_cuda_graph:cfg.graph?'1':'0',gpu_mem_limit:String(8*2**30)};
    check(functions.UpdateCUDAProviderOptions(cuda,Object.keys(values),Object.values(values),Object.keys(values).length));
    check(functions.SessionOptionsAppendExecutionProvider_CUDA_V2(opts,cuda));
  }
  const profile=path.join(fixture,`${name}.profile`);
  if(cfg.profile)check(functions.EnableProfiling(opts,profile));
  const started=performance.now();
  const sess=own('Session','CreateSession',env,cfg.model,opts);
  emit({event:'load',seconds:(performance.now()-started)/1000});
  const tokenizer=new Tokenizer(JSON.parse(fs.readFileSync(cfg.tokenizer)),JSON.parse(fs.readFileSync(path.join(path.dirname(cfg.tokenizer),'tokenizer_config.json'))));
  const tokenizerJson=JSON.parse(fs.readFileSync(cfg.tokenizer));
  const special=tokenizerJson.added_tokens.filter(t=>['<s>','</s>','<pad>'].includes(t.content)).map(t=>[t.content,t.id]);
  if(JSON.stringify(special.sort())!==JSON.stringify([['<s>',0],['</s>',2],['<pad>',1]].sort()))throw new Error('Unexpected special token IDs');
  const inputs=JSON.parse(fs.readFileSync(cfg.inputs));
  if(!inputs.length||inputs.some(i=>!i.documents?.length))throw new Error('Empty inputs');
  if(cfg.graph){
    if(!cfg.fixed||!cfg.binding||cfg.backend!=='cuda'||inputs.some(i=>i.documents.length!==cfg.batch))throw new Error('Invalid graph workload');
    cudaLib=koffi.load('C:/Program Files/NVIDIA GPU Computing Toolkit/CUDA/v12.8/bin/cudart64_12.dll');
    const malloc=cudaLib.func('int cudaMalloc(_Out_ void **,size_t)');
    cudaFree=cudaLib.func('int cudaFree(void *)');
    cudaMemcpy=cudaLib.func('int cudaMemcpy(void *,void *,size_t,int)');
    sync=cudaLib.func('int cudaDeviceSynchronize()');
    const alloc=bytes=>{const p=[null];if(malloc(p,bytes)!==0)throw new Error('cudaMalloc failed');owned.push(()=>cudaFree(p[0]));return p[0];};
    const info=own('MemoryInfo','CreateMemoryInfo','Cuda',0,0,0);
    const bytes=cfg.batch*512*8;
    buffers={ids:alloc(bytes),mask:alloc(bytes),output:alloc(cfg.batch*4),bytes};
    buffers.values=[tensor(info,buffers.ids,[cfg.batch,512],7),tensor(info,buffers.mask,[cfg.batch,512],7),tensor(info,buffers.output,[cfg.batch,1],1)];
    buffers.values.forEach(v=>owned.push(()=>functions.ReleaseValue(v)));
    buffers.binding=own('IoBinding','CreateIoBinding',sess);
    check(functions.BindInput(buffers.binding,'input_ids',buffers.values[0]));check(functions.BindInput(buffers.binding,'attention_mask',buffers.values[1]));
    check(functions.BindOutput(buffers.binding,'logits',buffers.values[2]));
  }
  for(let repeat=0;repeat<=cfg.repeats;repeat++)for(const input of inputs){
    const start=performance.now(),logits=[],shapes=[];
    for(let offset=0;offset<input.documents.length;offset+=cfg.batch){
      const docs=input.documents.slice(offset,offset+cfg.batch);
      const encoded=docs.map(d=>encoding(tokenizer,input.query,d));
      const length=cfg.fixed?512:Math.max(...encoded.map(e=>e.length));
      const ids=new BigInt64Array(docs.length*length),mask=new BigInt64Array(ids.length);ids.fill(1n);
      encoded.forEach((e,i)=>e.forEach((v,j)=>{ids[i*length+j]=BigInt(v);mask[i*length+j]=1n;}));
      shapes.push([docs.length,length]);
      if(buffers){
        if(cudaMemcpy(buffers.ids,ids,buffers.bytes,1)||cudaMemcpy(buffers.mask,mask,buffers.bytes,1))throw new Error('GPU input copy failed');
        check(functions.RunWithBinding(sess,null,buffers.binding));check(functions.SynchronizeBoundOutputs(buffers.binding));
        const result=new Float32Array(docs.length);
        if(cudaMemcpy(result,buffers.output,result.byteLength,2)||sync())throw new Error('GPU output copy failed');
        logits.push(...result);
      }else{
        const values=[tensor(cpuInfo,ids,[docs.length,length],7),tensor(cpuInfo,mask,[docs.length,length],7)];
        let output,binding;
        try{
          if(cfg.binding){
            const result=new Float32Array(docs.length);output=tensor(cpuInfo,result,[docs.length,1],1);
            binding=pointer('CreateIoBinding',sess);
            check(functions.BindInput(binding,'input_ids',values[0]));check(functions.BindInput(binding,'attention_mask',values[1]));check(functions.BindOutput(binding,'logits',output));
            check(functions.SynchronizeBoundInputs(binding));check(functions.RunWithBinding(sess,null,binding));check(functions.SynchronizeBoundOutputs(binding));
            logits.push(...result);
          }else{
            const result=[null];check(functions.Run(sess,null,['input_ids','attention_mask'],values,2,['logits'],1,result));output=result[0];
            const info=pointer('GetTensorTypeAndShape',output),kind=[0],count=[0];
            try{check(functions.GetTensorElementType(info,kind));check(functions.GetTensorShapeElementCount(info,count));
              if(kind[0]!==1||Number(count[0])!==docs.length)throw new Error('Unexpected logits dtype/size');
            }finally{functions.ReleaseTensorTypeAndShapeInfo(info);}
            const data=pointer('GetTensorMutableData',output);logits.push(...koffi.decode(data,'float',docs.length));
          }
        }finally{
          if(binding)functions.ReleaseIoBinding(binding);if(output)functions.ReleaseValue(output);values.forEach(v=>functions.ReleaseValue(v));
        }
      }
    }
    if(logits.length!==input.documents.length||!logits.every(Number.isFinite))throw new Error('Invalid scores');
    emit({event:'row',id:input.id,repeat,warmup:repeat===0,seconds:(performance.now()-start)/1000,logits,shapes});
  }
  if(cfg.profile){
    const allocator=pointer('GetAllocatorWithDefaultOptions');const p=pointer('SessionEndProfiling',sess,allocator);
    emit({event:'profile',path:koffi.decode(p,'char',-1)});check(functions.AllocatorFree(allocator,p));
  }
  emit({event:'complete',name});console.log(`${name}: complete`);
}catch(e){emit({event:'error',error:e.message});console.error(`${name}: ${e.message}`);process.exitCode=1;}
finally{for(const release of owned.reverse())release();fs.closeSync(fd);}
