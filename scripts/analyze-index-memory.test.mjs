import {test} from 'node:test';
import assert from 'node:assert/strict';
import {summarize,summarizePair,combineMatrices} from './analyze-index-memory.mjs';
const rows=()=>[
  {kind:'runtime',phase:'warm',detail:{embed:{actual:'directml'}}},
  ...Array.from({length:100},(_,n)=>({kind:'search',detail:{ok:true,before:true,after:true,indexing:true,ms:n+1}})),
  {kind:'timing',phase:'reindex_done',detail:{ms:200}},
  {kind:'end',detail:{ok:true}},
];
test('Only complete series accepted; overlap p95 needs 100 overlapping calls',()=>{
  const input=rows();
  assert.equal(summarize(input).accepted,true);
  assert.equal(summarize(input).full_overlap.p95_ms,95);
  input[1].detail.after=false;
  assert.equal(summarize(input).accepted,true);
  assert.equal(summarize(input).full_overlap.count,99);
  assert.equal(summarize(input).full_overlap.p95_ms,null);
  input[2].detail.ok=false;
  assert.equal(summarize(input).accepted,false);
});
test('Truncated, missing and failed run rejected',()=>{
  assert.equal(summarize(rows().slice(0,-1)).accepted,false);
  assert.equal(summarize(rows().filter((_,n)=>n!==3)).accepted,false);
  const input=rows(); input.at(-1).detail.ok=false;
  assert.equal(summarize(input).accepted,false);
  assert.equal(summarize([]).accepted,false);
});
test('Pair requires both FP16 runtimes, reload and completed lifecycle phases',()=>{
  const runtime={actual:'directml',precision:'fp16',fallback_reason:null};
  const input=[...['embed_load','rerank_load'].map(phase=>({kind:'timing',phase,detail:{runtime}})),
    {kind:'timing',phase:'rerank_after_reload',detail:{scores:Array(15).fill(.5),max_score_delta:0}},
    ...['embed_batch_retained','rerank_retained','rerank_dropped','all_models_dropped','worker_shutdown'].map(phase=>({kind:'plateau_end',phase})),
    {kind:'end',detail:{ok:true}}];
  assert.equal(summarizePair(input).accepted,true);
  assert.equal(summarizePair(input.slice(1)).accepted,false);
  input[2].detail.max_score_delta=.1;
  assert.equal(summarizePair(input).accepted,false);
});

test('Reindex memory excludes later samples and needs a completed interval',()=>{
  const input=[
    {kind:'sample',elapsed_ms:1,memory:{private_bytes:20}},
    {kind:'marker',phase:'reindex_start',elapsed_ms:2},
    {kind:'sample',elapsed_ms:3,memory:{private_bytes:30}},
    {kind:'timing',phase:'reindex_done',elapsed_ms:4,detail:{ms:2}},
    {kind:'sample',elapsed_ms:5,memory:{private_bytes:100}},
  ];
  assert.deepEqual(summarize(input).reindex_memory,{before_sampled_private_peak_bytes:20,during_sampled_private_peak_bytes:30,during_samples:1});
  input.splice(3,1);
  assert.equal(summarize(input).reindex_memory.during_sampled_private_peak_bytes,null);
});

test('Complete load queries do not turn a guarded process into a full pass',()=>{
  const datasets=['fp16','fp32'].map(p=>({name:p,corpus:[{file:'a\\b.md',sha256:'AA'}],summary:{pairs:p==='fp16'?[{accepted:true},{accepted:true}]:[],series:['off','on'].flatMap(r=>[1,2,4].map(n=>({name:`${p}-${r}-${n}`,accepted:true,searches:{count:100},full_overlap:{count:100},failed:0})))}}));
  datasets[0].summary.series[0].accepted=false;
  datasets[1].corpus[0]={file:'a/b.md',sha256:'aa'};
  const result=combineMatrices(datasets);
  assert.equal(result.load_queries_complete,true);
  assert.equal(result.full_processes_complete,false);
  assert.equal(result.accepted_processes,11);
  assert.equal(result.repeated_pair_complete,true);
  assert.throws(()=>combineMatrices([datasets[0],datasets[0]]),/Duplicate/);
  datasets[1].corpus[0].sha256='BB';
  assert.throws(()=>combineMatrices(datasets),/corpora differ/);
});
