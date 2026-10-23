import {test} from 'node:test';
import assert from 'node:assert/strict';
import {summarize,compare} from './analyze-model-overlap.mjs';
function fixture(mode='serial') {
  const event = (phase,elapsed_ms,detail) => ({kind:'marker',phase,elapsed_ms,detail,memory:{lifetime_peak_commit_bytes:1024}});
  return [event('warm',0,{embed:{actual:'directml',precision:'fp32'},rerank:{actual:'directml',precision:'fp16'}}),event('reindex_start',10,{mode}),event('reindex_done',100,{ms:90}),event('queries_start',mode==='serial'?101:11,{}),...Array.from({length:100},(_,n)=>({...event(mode==='serial'?'after_index':'during_index',mode==='serial'?102+n:12+n/2,{ok:true,stable:true,ms:n+1,indexing:mode==='overlap',score_kind:'rerank',rerank_error:null}),kind:'search'})),event('end',300,{ok:true})];
}
test('full serial and overlap accepted, percentiles reproducible',()=>{
  const s=summarize(fixture(),'serial','fp32');
  assert.equal(s.queries,100); assert.equal(s.median_ms,50.5); assert.equal(s.p95_ms,95);
  assert.equal(summarize(fixture('overlap'),'overlap','fp32').overlapping,100);
});
test('failed lifecycle never accepted',()=>{const r=fixture();r.at(-1).detail.ok=false;assert.throws(()=>summarize(r,'serial','fp32'),/Incomplete/);});
test('CPU fallback rejected',()=>{const r=fixture();r[0].detail.embed.actual='cpu';assert.throws(()=>summarize(r,'serial','fp32'),/runtime/);});
test('missing or changed responses rejected',()=>{
  const r=fixture();r[4].detail.stable=false;assert.throws(()=>summarize(r,'serial','fp32'),/queries/);
  assert.throws(()=>summarize(fixture().filter((_,i)=>i!==4),'serial','fp32'),/queries/);
});
test('false overlap and early serial start rejected',()=>{
  const r=fixture('overlap');r[4].detail.indexing=false;assert.throws(()=>summarize(r,'overlap','fp32'),/overlap/);
  const s=fixture();s[3].elapsed_ms=99;assert.throws(()=>summarize(s,'serial','fp32'),/overlap/);
});
test('preflight-only real attempt rejected',()=>{
  assert.throws(()=>compare('docs/chats/2026-10-23-index-memory-policy/fp32-8bfc763011d949f6a9223b18c0ec7230'),/did not complete/);
});
