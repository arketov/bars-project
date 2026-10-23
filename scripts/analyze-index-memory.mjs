import fs from 'node:fs';
import path from 'node:path';
import {gunzipSync} from 'node:zlib';
import {pathToFileURL} from 'node:url';

export function summarize(rows) {
  const searches=rows.filter(r=>r.kind==='search');
  const overlap=searches.filter(r=>r.detail.before&&r.detail.after&&r.detail.indexing);
  const completed=rows.at(-1)?.kind==='end'&&rows.at(-1)?.detail.ok===true;
  const samples=rows.filter(r=>r.memory);
  const peak=key=>samples.length?Math.max(...samples.map(r=>r.memory[key]??0)):null;
  const timings=data=>{
    const sorted=data.map(r=>r.detail.ms).sort((a,b)=>a-b);
    return {count:sorted.length,median_ms:sorted.length>=100?sorted[Math.ceil(sorted.length*.5)-1]:null,
      p95_ms:sorted.length>=100?sorted[Math.ceil(sorted.length*.95)-1]:null,max_ms:sorted.at(-1)??null};
  };
  const first=searches.at(0)?.detail,last=searches.at(-1)?.detail;
  const start=rows.find(r=>r.kind==='marker'&&r.phase==='reindex_start')?.elapsed_ms;
  const end=rows.find(r=>r.kind==='timing'&&r.phase==='reindex_done')?.elapsed_ms;
  const samplePeak=data=>data.length?Math.max(...data.map(r=>r.memory.private_bytes)):null;
  const measured=samples.filter(r=>r.kind==='sample'&&Number.isFinite(r.memory.private_bytes));
  const before=Number.isFinite(start)?measured.filter(r=>r.elapsed_ms<start):[];
  const during=Number.isFinite(start)&&Number.isFinite(end)&&end>=start
    ?measured.filter(r=>r.elapsed_ms>=start&&r.elapsed_ms<=end):[];
  return {completed,accepted:completed&&searches.length===100&&searches.every(r=>r.detail.ok===true)
      &&rows.some(r=>r.phase==='reindex_done')&&rows.some(r=>r.phase==='warm'&&r.kind==='runtime'),
    searches:timings(searches),full_overlap:timings(overlap),failed:searches.filter(r=>!r.detail.ok).length,
    sampled_private_peak_bytes:peak('private_bytes'),lifetime_peak_commit_bytes:peak('lifetime_peak_commit_bytes'),
    working_set_peak_bytes:peak('working_set_bytes'),runtime:rows.find(r=>r.kind==='runtime')?.detail??null,
    reindex_ms:rows.find(r=>r.phase==='reindex_done')?.detail.ms??null,
    reindex_memory:{before_sampled_private_peak_bytes:samplePeak(before),during_sampled_private_peak_bytes:samplePeak(during),during_samples:during.length},
    index_progress:{docs_first:first?.docs_before??null,docs_last:last?.docs_after??null,
      chunks_first:first?.chunks_before??null,chunks_last:last?.chunks_after??null},
    phases:rows.filter(r=>r.kind==='plateau_end').map(r=>({phase:r.phase,...r.memory}))};
}

export function summarizePair(rows) {
  const base=summarize(rows);
  const event=phase=>rows.find(r=>r.phase===phase&&r.kind==='timing')?.detail;
  const embed=event('embed_load')?.runtime,rerank=event('rerank_load')?.runtime;
  const reload=event('rerank_after_reload');
  const accepted=base.completed&&[embed,rerank].every(r=>r?.actual==='directml'&&r?.precision==='fp16'&&!r.fallback_reason)
    &&reload?.scores?.length===15&&reload.scores.every(Number.isFinite)&&reload.max_score_delta<=1e-5
    &&['embed_batch_retained','rerank_retained','rerank_dropped','all_models_dropped','worker_shutdown'].every(p=>rows.some(r=>r.phase===p&&r.kind==='plateau_end'));
  return {...base,accepted};
}

function readRows(file) {
  const bytes=fs.readFileSync(file);
  return (file.endsWith('.gz')?gunzipSync(bytes).toString():bytes.toString()).trim().split(/\r?\n/).filter(Boolean).map(JSON.parse);
}
function main(root) {
  if(!root)throw Error('Expected evidence directory');
  const series=[];
  for(const name of fs.readdirSync(root).filter(n=>/^fp(?:16|32)-(?:on|off)-[124]-memory\.jsonl\.gz$/.test(n))) {
    const stem=name.replace('-memory.jsonl.gz','');
    const ortFile=path.join(root,`${stem}-ort.jsonl.gz`);
    const ort=fs.existsSync(ortFile)?readRows(ortFile):[];
    const allocators=ort.filter(r=>/bfc_arena|allocator|arena.*extend/i.test(`${r.location} ${r.message}`));
    const placements=ort.flatMap(r=>{const match=r.message?.match(/Node\(s\) placed on \[([^\]]+)\]\. Number of nodes: (\d+)/);return match?[{ms:r.ms,provider:match[1],nodes:Number(match[2])}]:[];});
    const gpuFile=path.join(root,`${stem}.gpu.jsonl.gz`);
    const gpu=fs.existsSync(gpuFile)?readRows(gpuFile):[];
    const populated=gpu.filter(r=>r.gpu?.length);
    const gpuPeak=key=>populated.length?Math.max(...populated.map(r=>r.gpu.reduce((s,a)=>s+Number(a[key]),0))):null;
    series.push({name:stem,...summarize(readRows(path.join(root,name))),ort_events:ort.length,
      placements,gpu_samples:gpu.length,gpu_dedicated_peak_bytes:gpuPeak('DedicatedUsage'),gpu_shared_peak_bytes:gpuPeak('SharedUsage'),
      allocator_events:allocators.length,allocator_messages:[...new Set(allocators.map(r=>r.message))]});
  }
  const pairs=[];
  for(const name of fs.readdirSync(root).filter(n=>/^pair-\d+\.jsonl\.gz$/.test(n))) {
    pairs.push({name,...summarizePair(readRows(path.join(root,name)))});
  }
  const out={schema:1,series,pairs,matrix_complete:series.length===12&&series.every(s=>s.accepted),
    repeated_pair_complete:pairs.length>=2&&pairs.every(p=>p.accepted),timing_valid:false};
  fs.writeFileSync(path.join(root,'summary.json'),JSON.stringify(out,null,2)+'\n');
  console.log(JSON.stringify({series:series.length,pairs:pairs.length,matrix_complete:out.matrix_complete,repeated_pair_complete:out.repeated_pair_complete}));
}
if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href)main(process.argv[2]);
