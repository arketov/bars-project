import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import crypto from 'node:crypto';
const root=process.cwd(),fixture=process.env.BARS_PROBE_RUNS_DIR??path.join(root,'docs/chats/2026-11-01-rtx-accelerators/fixture-runs');
const evidence=process.env.BARS_PROBE_EVIDENCE_DIR??path.join(root,'docs/knowledge/accelerators-2026-11-01');
const inputs=JSON.parse(fs.readFileSync(path.join(evidence,'inputs.json')));
const rows=file=>fs.readFileSync(file,'utf8').split('\n').filter(Boolean).map(JSON.parse);
const median=a=>{const x=[...a].sort((a,b)=>a-b);return x.length?(x.length%2?x[Math.floor(x.length/2)]:(x[x.length/2-1]+x[x.length/2])/2):null;};
const percentile=(a,p)=>{const x=[...a].sort((a,b)=>a-b);return x.length?x[Math.ceil(x.length*p)-1]:null;};
const sigmoid=x=>1/(1+Math.exp(-x));
const dot=(a,b)=>a.reduce((s,x,i)=>s+x*b[i],0);
const cosine=(a,b)=>dot(a,b)/Math.sqrt(dot(a,a)*dot(b,b));
const sha=buffer=>crypto.createHash('sha256').update(buffer).digest('hex');
const order=(scores,threshold=0)=>scores.map((v,i)=>({v,i})).filter(x=>sigmoid(x.v)>=threshold).sort((a,b)=>b.v-a.v||a.i-b.i).map(x=>x.i);
const same=(a,b)=>JSON.stringify(a)===JSON.stringify(b);
const baselineGguf=rows(path.join(fixture,'gguf-Vulkan-off-f16-serial.prebuilt.jsonl')).filter(r=>r.event==='row'&&r.repeat===1);
if(baselineGguf.length!==8)throw new Error('Incomplete GGUF reference');
const baselineMap=new Map(baselineGguf.map(r=>[r.id,r]));
function referenceGguf(cfg){
  const stem=cfg.kv!=='f16'||cfg.batch!==512?'on-f16':'off-f16';
  const suffix=cfg.name.includes('-controlled')?'controlled':'serial';
  const filename=path.join(fixture,`gguf-${cfg.backend}-${stem}-${suffix}.prebuilt.jsonl`);
  if(!fs.existsSync(filename))return baselineMap;
  return new Map(rows(filename).filter(r=>r.event==='row'&&r.repeat===1).map(r=>[r.id,r]));
}
const baselineOnnx=rows(path.join(fixture,'onnx-directml-b16.ffi.jsonl')).filter(r=>r.event==='row'&&r.repeat===1);
const fixedOnnx=rows(path.join(fixture,'onnx-directml-fixed-v2.ffi.jsonl')).filter(r=>r.event==='row'&&r.repeat===1);
const allFiles=fs.readdirSync(fixture).filter(f=>f.endsWith('.prebuilt.jsonl')||f.endsWith('.ffi.jsonl')).sort();
const summary=[],raw=[],monitors=[],nativeLogs=[],completedProfiles=[],configs=[];
for(const file of allFiles){
  const content=rows(path.join(fixture,file)),name=file.replace(/\.(prebuilt|ffi)\.jsonl$/,'');
  const complete=content.some(r=>r.event==='complete');
  if(complete)completedProfiles.push(...content.filter(r=>r.event==='profile').map(r=>({name,file:path.basename(r.path)})));
  // Early debugging and overlapping GGUF attempts remain explicitly separate.
  const selected=file.endsWith('.ffi.jsonl')||name.endsWith('-serial')||name.includes('-controlled');
  if(!selected)continue;
  const cfg=JSON.parse(fs.readFileSync(path.join(fixture,name+'.json')));
  configs.push({name,config:{...cfg,model:cfg.kind==='gguf'?'Qwen3-Embedding-4B-Q4_K_M.gguf':'bge-reranker-v2-m3 FP16',
    tokenizer:cfg.tokenizer?'rozgo/bge-reranker-v2-m3 tokenizer.json':null,inputs:path.basename(cfg.inputs),profile:cfg.profile?true:null,cache:cfg.cache?'owned trt-cache':null}});
  const warm=content.filter(r=>r.event==='row'&&r.warmup),measured=content.filter(r=>r.event==='row'&&!r.warmup);
  const unique=[...new Set(measured.map(r=>r.id))];
  const expected=cfg.kind==='gguf'?8:JSON.parse(fs.readFileSync(cfg.inputs)).length;
  if(complete&&(measured.length!==expected*cfg.repeats||unique.length!==expected))throw new Error(`Incomplete successful series ${name}`);
  for(const id of unique)if(complete&&measured.filter(r=>r.id===id).length!==cfg.repeats)throw new Error(`Missing repeats ${name}/${id}`);
  const mfile=path.join(fixture,file.replace(/\.(prebuilt|ffi)\.jsonl$/,'.$1.monitor.jsonl'));
  const monitor=fs.existsSync(mfile)?rows(mfile):[];
  const processFile=path.join(fixture,name+'.ffi.process.json');
  const processResult=fs.existsSync(processFile)?JSON.parse(fs.readFileSync(processFile)):null;
  const item={name,kind:cfg.kind,backend:cfg.backend,complete,n:measured.length,process:processResult,
    error:content.find(r=>r.event==='error')?.error??null,
    median_s:median(measured.map(r=>r.seconds)),p95_s:percentile(measured.map(r=>r.seconds),.95),
    load_s:content.find(r=>r.event==='load'||r.event==='ready')?.seconds??null,
    warmup_median_s:median(warm.map(r=>r.seconds)),
    sampled_private_peak:monitor.length?Math.max(...monitor.map(m=>m.private)):null,
    sampled_gpu_total_mib_peak:monitor.length?Math.max(...monitor.map(m=>Number(m.gpu.split(',')[0]))):null,
    cpu_total_median:median(monitor.map(m=>m.cpu_total_percent)),
    cpu_background_median:median(monitor.map(m=>m.cpu_background_percent).filter(v=>v!==null&&v!==undefined)),
    batch:cfg.batch,flash:cfg.flash,kv:cfg.kv,binding:cfg.binding,graph:cfg.graph,fixed:cfg.fixed};
  if(cfg.kind==='gguf'&&complete){
    let minCos=1,maxComponent=0,maxScoreDelta=0,factorMinCos=1,factorMaxScoreDelta=0;
    const factor=referenceGguf(cfg);
    for(const row of measured){
      const base=baselineMap.get(row.id);
      if(!base||row.vector.length!==base.vector.length||!row.vector.every(Number.isFinite))throw new Error('Invalid vector');
      minCos=Math.min(minCos,cosine(base.vector,row.vector));
      factorMinCos=Math.min(factorMinCos,cosine(factor.get(row.id).vector,row.vector));
      maxComponent=Math.max(maxComponent,...row.vector.map((v,i)=>Math.abs(v-base.vector[i])));
    }
    const first=new Map(measured.filter(r=>r.repeat===1).map(r=>[r.id,r]));
    for(const a of baselineGguf.filter(r=>r.id.endsWith('-query')))for(const b of baselineGguf.filter(r=>r.id.endsWith('-doc')))
      maxScoreDelta=Math.max(maxScoreDelta,Math.abs(cosine(a.vector,b.vector)-cosine(first.get(a.id).vector,first.get(b.id).vector)));
    for(const a of [...factor.values()].filter(r=>r.id.endsWith('-query')))for(const b of [...factor.values()].filter(r=>r.id.endsWith('-doc')))
      factorMaxScoreDelta=Math.max(factorMaxScoreDelta,Math.abs(cosine(a.vector,b.vector)-cosine(first.get(a.id).vector,first.get(b.id).vector)));
    Object.assign(item,{min_vector_cosine:minCos,max_component_delta:maxComponent,max_query_document_cosine_delta:maxScoreDelta,
      factor_min_vector_cosine:factorMinCos,factor_max_query_document_cosine_delta:factorMaxScoreDelta});
    const log=fs.readFileSync(path.join(fixture,name+'.prebuilt.stderr.log'),'utf8');
    item.native=log.split('\n').filter(l=>/offloaded.*layers|flash_attn.*=|n_batch\s*=|n_ubatch\s*=|llama_kv_cache: size =/.test(l));
    nativeLogs.push({name,lines:item.native});
  }
  if(cfg.kind==='onnx'&&complete){
    const reference=new Map((cfg.fixed?fixedOnnx:baselineOnnx).map(r=>[r.id,r]));
    const factorFile=cfg.backend==='cuda'?path.join(fixture,cfg.fixed?'onnx-cuda-fixed-v2.ffi.jsonl':'onnx-cuda-b16-v2.ffi.jsonl'):null;
    const factor=factorFile&&fs.existsSync(factorFile)?new Map(rows(factorFile).filter(r=>r.event==='row'&&r.repeat===1).map(r=>[r.id,r])):reference;
    let maxLogit=0,maxScore=0,changedTop1=0,changedTop3=0,thresholdCrossings=0,metricLosses=0;
    const crossings=[];
    let maxFactorLogit=0;
    for(const row of measured){
      const base=reference.get(row.id);if(!base||base.logits.length!==row.logits.length)throw new Error(`Incomparable scores ${name}/${row.id}`);
      const ownBase=factor.get(row.id);if(!ownBase||ownBase.logits.length!==row.logits.length)throw new Error(`Incomparable factor ${name}/${row.id}`);
      maxFactorLogit=Math.max(maxFactorLogit,...row.logits.map((v,i)=>Math.abs(v-ownBase.logits[i])));
      if(!row.logits.every(Number.isFinite))throw new Error('Nonfinite logit');
      row.logits.forEach((v,i)=>{maxLogit=Math.max(maxLogit,Math.abs(v-base.logits[i]));maxScore=Math.max(maxScore,Math.abs(sigmoid(v)-sigmoid(base.logits[i])));if((sigmoid(v)>=.1)!==(sigmoid(base.logits[i])>=.1)){
        thresholdCrossings++;if(row.repeat===1)crossings.push({id:row.id,candidate:i,baseline:sigmoid(base.logits[i]),actual:sigmoid(v),path:inputs.find(c=>c.id===row.id).hits[i].path});
      }});
      changedTop1+=Number(order(row.logits)[0]!==order(base.logits)[0]);
      changedTop3+=Number(!same(order(row.logits).slice(0,3),order(base.logits).slice(0,3)));
      const source=inputs.find(i=>i.id===row.id);
      for(const threshold of [0,.1]){
        const rr=scores=>{const ranked=order(scores,threshold);const at=ranked.findIndex(i=>source.hits[i].relevant.length>0);return at>=0?1/(at+1):0;};
        metricLosses+=Number(rr(row.logits)<rr(base.logits));
      }
    }
    Object.assign(item,{max_logit_delta:maxLogit,max_sigmoid_delta:maxScore,factor_max_logit_delta:maxFactorLogit,changed_top1_rows:changedTop1,changed_top3_rows:changedTop3,threshold_crossings:thresholdCrossings,crossings,mrr_loss_rows:metricLosses});
  }
  summary.push(item);
  raw.push(...content.map(row=>({name,row})));
  monitors.push(...monitor.map(row=>({name,row})));
}
const profiles=[];
const profileEvents=[];
for(const {name,file} of completedProfiles){
  const events=JSON.parse(fs.readFileSync(path.join(fixture,file))),providers={},ops={};
  for(const e of events)if(e.cat==='Node'&&e.args?.provider){const p=e.args.provider;providers[p]=(providers[p]??0)+1;const key=p+'/'+e.args.op_name;ops[key]=(ops[key]??0)+1;}
  profiles.push({name,file,providers,ops});
  profileEvents.push(...events.map(event=>({name,event})));
}
const refusals=fs.readdirSync(fixture).filter(f=>f.endsWith('.ffi.preflight.json')).map(f=>JSON.parse(fs.readFileSync(path.join(fixture,f))));
const sourceDir=path.join(root,'spikes/accelerators');
const sources=Object.fromEntries(fs.readdirSync(sourceDir).filter(f=>fs.statSync(path.join(sourceDir,f)).isFile()).map(f=>[f,sha(fs.readFileSync(path.join(sourceDir,f)))]));
fs.writeFileSync(path.join(evidence,'protocol.json'),JSON.stringify({vector_format:'IEEE754 f32 little-endian; vector_offset_f32 counts scalar elements, not bytes',
  timing_valid_for_causal_ranking:false,reason:'varying background load and repeated-baseline drift',configs,
  source_hashes_at_archive:sources,refusals},null,2));
fs.writeFileSync(path.join(evidence,'summary.json'),JSON.stringify({generated_at:new Date().toISOString(),
  limitations:['shared machine; sampled GPU memory is total device usage','GGUF loopback HTTP, ORT direct C API; timings cannot be compared across model kinds','upstream binaries; no product build or test acceptance','four GGUF questions; score parity is not full retrieval quality'],
  runs:summary,profiles,refusals},null,2));
const vectors=[];
let vectorOffset=0;
for(const item of raw)if(item.row.vector){
  const values=item.row.vector;
  if(values.some(v=>Math.fround(v)!==v))throw new Error('Cannot losslessly archive vector as f32');
  const data=Buffer.alloc(values.length*4);values.forEach((v,i)=>data.writeFloatLE(v,i*4));vectors.push(data);
  item.row={...item.row,vector_offset_f32:vectorOffset,vector_length:values.length};delete item.row.vector;
  vectorOffset+=values.length;
}
fs.writeFileSync(path.join(evidence,'vectors.f32.gz'),zlib.gzipSync(Buffer.concat(vectors)));
for(const [file,value] of [['rows.jsonl.gz',raw],['monitor.jsonl.gz',monitors],['native.jsonl.gz',nativeLogs],['profiles.jsonl.gz',profileEvents]])
  fs.writeFileSync(path.join(evidence,file),zlib.gzipSync(value.map(v=>JSON.stringify(v)).join('\n')+'\n'));
const hashes=Object.fromEntries(fs.readdirSync(evidence).filter(f=>f!=='checksums.json'&&fs.statSync(path.join(evidence,f)).isFile()).map(f=>[f,sha(fs.readFileSync(path.join(evidence,f)))]));
fs.writeFileSync(path.join(evidence,'checksums.json'),JSON.stringify(hashes,null,2));
console.log(JSON.stringify(summary.map(({name,complete,n,median_s,error,min_vector_cosine,max_sigmoid_delta,changed_top3_rows})=>({name,complete,n,median_s,error,min_vector_cosine,max_sigmoid_delta,changed_top3_rows})),null,2));
