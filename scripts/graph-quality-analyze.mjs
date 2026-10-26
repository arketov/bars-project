import fs from 'node:fs';
import crypto from 'node:crypto';
import zlib from 'node:zlib';
import assert from 'node:assert/strict';
const [root, goldFile, resultsFile, output] = process.argv.slice(2);
const read = p => JSON.parse(fs.readFileSync(p));
const data = read(`${root}/dataset.json`), gold = read(goldFile), results = read(resultsFile);
assert.equal(results.snapshot, data.snapshot);
const bySource = new Map(data.sources.map(s => [s.meta.reference.id, s]));
const byEv = new Map(data.evidence.map(e => [e.id,e]));
const byNode = new Map(data.nodes.map(n => [n.reference.id,n]));
for (const e of data.evidence) {
  const s = bySource.get(e.document.id);
  assert.equal(s.meta.hash,e.source_hash);
  assert.equal(Buffer.from(s.text).subarray(e.span.start,e.span.end).toString('utf8'),e.quote);
}
for (const c of gold.cases) for (const e of c.evidence) {
  const s = data.sources.find(s=>s.meta.docs_path===e.path);
  assert(s,`gold missing source: ${e.path}`);
  const bytes=Buffer.from(s.text);
  assert.equal(crypto.createHash('sha256').update(bytes).digest('hex'),e.sha256);
  assert.equal(bytes.subarray(e.start,e.end).toString(),e.needle);
}
const size = v => Buffer.byteLength(JSON.stringify(v));
function pack(packets,budget) {
  const kept=[];
  for(const p of packets) if(size([...kept,p])<=budget) kept.push(p);
  assert(size(kept)<=budget);
  return {packets:kept,bytes:size(kept),skipped:packets.length-kept.length};
}
// A proof path is an atomic packet; oversized packets are skipped, never cut.
function graphPackets(reply) {
  if(reply.error) return [];
  const ev=new Map(reply.evidence.map(e=>[e.id,e]));
  const paths=new Map(reply.sources.map(s=>[s.reference.id,s.docs_path]));
  const nodes=new Map(reply.nodes.map(n=>[n.reference.id,n]));
  return reply.items.flatMap(item=>{
    let ids=[];
    if(item.type==='relation') ids=item.assertion.evidence;
    if(item.type==='path'||item.type==='impact') ids=item.path.evidence;
    if(['role_group','audit','conflict'].includes(item.type)) ids=item.evidence;
    if(item.type==='topic') ids=item.representatives.flatMap(r=>{
      assert(nodes.has(r.id),'topic representative absent from reply');return nodes.get(r.id).evidence;
    });
    const evidence=[...new Set(ids)].map(id=>{
      assert(ev.has(id),`proof absent from reply: ${id}`);
      const e=ev.get(id);assert(paths.has(e.document.id),'source absent from reply');
      return {path:paths.get(e.document.id),start:e.span.start,end:e.span.end,first_line:e.span.first_line,last_line:e.span.last_line,text:e.quote};
    });
    const labels=item.type==='path'||item.type==='impact'?item.path.nodes.map(r=>{
      assert(nodes.has(r.id),'path node absent from reply');return nodes.get(r.id).label;
    }):undefined;
    // Extra proofs of a role/topic may be trimmed; a relation/path proof remains atomic.
    if(item.type==='role_group') return evidence.map(e=>({reason:{type:item.type,role:item.role},evidence:[e]}));
    if(item.type==='topic') return evidence.map(e=>({reason:{type:item.type,label:item.label},evidence:[e]}));
    return [{reason:item,labels,evidence}];
  });
}
const hitPacket = h=>({reason:'search_hit',evidence:[{path:h.path.replace(/^docs\//,''),first_line:h.first_line,last_line:h.last_line,text:h.text}]});
function score(c,packed) {
  const ev=packed.packets.flatMap(p=>p.evidence), expected=c.evidence;
  const match=(a,b)=>a.path===b.path && (a.start!==undefined?a.start<=b.start&&a.end>=b.end:
    a.first_line<=b.first_line&&a.last_line>=b.first_line&&a.text.includes(b.needle));
  const found=expected.filter(e=>ev.some(x=>match(x,e))).length;
  return {...packed,found,expected:expected.length,recall:expected.length?found/expected.length:null,
    complete:expected.length?found===expected.length:null,
    // Focus on annotated evidence, not semantic precision of every unlabelled paragraph.
    annotated_focus:ev.length?ev.filter(e=>expected.some(x=>match(e,x))).length/ev.length:null,
    correct_role_found:c.scenario==='S3'?expected.filter(e=>packed.packets.some(p=>p.reason?.type==='role_group'&&p.reason.role===e.role&&p.evidence.some(x=>match(x,e)))).length:null};
}
const rows=gold.cases.map(c=>{
  const raw=results.rows.find(r=>r.id===c.id);
  assert(raw,`missing result ${c.id}`);
  const variants={graph:score(c,pack(graphPackets(raw.graph),gold.budget_bytes))};
  for(const v of raw.variants) {
    for(const h of v.hits) assert(data.sources.some(s=>h.path===s.meta.path),`hit outside frozen corpus: ${h.path}`);
    const packets=v.hits.map(hitPacket);
    variants[`${v.name}_hits`]=score(c,pack(packets,gold.budget_bytes));
    if(v.related) packets.push(...graphPackets(v.related));
    variants[v.name]=score(c,pack(packets,gold.budget_bytes));
  }
  const topicStats=reply=>{
    if(!reply||reply.error) return null;
    const topics=reply.items.filter(i=>i.type==='topic');
    const ids=new Set(topics.flatMap(t=>t.members.map(r=>r.id)));
    const reps=new Set(topics.flatMap(t=>t.representatives.map(r=>r.id)));
    const expected=[...new Set(c.evidence.map(e=>e.path))].map(p=>data.sources.find(s=>s.meta.docs_path===p).meta.reference.id);
    return {groups:topics.length,members:ids.size,representatives:reps.size,
      expected_documents:expected.length,member_hits:expected.filter(id=>ids.has(id)).length,
      representative_hits:expected.filter(id=>reps.has(id)).length,truncated:reply.truncated};
  };
  return {id:c.id,scenario:c.scenario,split:c.split,kind:c.kind,variants,
    explicit_checks:{empty:c.expected_empty?raw.graph.items?.length===0:null,
      no_path:c.expected_path===false?!raw.graph.items?.some(i=>i.type==='path'):null,
      honest_partial:c.expected_semantic_partial?raw.graph.layer_state==='partial'&&raw.graph.coverage?.semantic_completeness==null:null},
    topics:c.scenario==='S6'?{communities:topicStats(raw.graph),modules:topicStats(raw.graph_modules)}:null,
    graph_unbudgeted_found:score(c,{packets:graphPackets(raw.graph),bytes:0,skipped:0}).found,
    graph_error:raw.graph.error??null,graph_items:raw.graph.items?.length??0,
    graph_truncated:raw.graph.truncated,graph_partial:raw.graph.layer_state,
    graph_missing_roles:raw.graph.missing_roles,
    hit3:Object.fromEntries(raw.variants.map(v=>[v.name,c.evidence.length?v.hits.slice(0,3).some(h=>c.evidence.some(e=>h.path===`docs/${e.path}`&&h.first_line<=e.first_line&&h.last_line>=e.first_line&&h.text.includes(e.needle))):null]))};
});
const summaries=[];
for(const split of ['dev','holdout']) for(const scenario of ['all',...Array.from({length:8},(_,i)=>`S${i+1}`)]) {
  const part=rows.filter(r=>r.split===split&&(scenario==='all'||r.scenario===scenario));
  for(const name of Object.keys(rows[0].variants)) {
    const values=part.map(r=>r.variants[name]), expected=values.reduce((n,v)=>n+v.expected,0),found=values.reduce((n,v)=>n+v.found,0);
    summaries.push({split,scenario,name,expected,found,recall:expected?found/expected:null,
      complete:values.filter(v=>v.complete===true).length,scored_cases:values.filter(v=>v.complete!==null).length,
      mean_bytes:values.reduce((n,v)=>n+v.bytes,0)/values.length});
  }
}
const assertions=data.edges.filter(e=>!['contains','covers','next_chunk','links_to','mentions','similar_to','co_occurs','member_of'].includes(e.predicate)).map(e=>({id:e.id,subject:byNode.get(e.from)?.label,predicate:e.predicate,object:byNode.get(e.to)?.label,polarity:e.polarity,qualifiers:e.qualifiers,evidence:e.evidence.map(id=>{const ev=byEv.get(id);return {path:bySource.get(ev.document.id).meta.docs_path,quote:ev.quote,span:ev.span}})}));
const report={snapshot:data.snapshot,source_count:data.sources.length,evidence_count:data.evidence.length,
  exact_spans:true,gold_sha256:crypto.createHash('sha256').update(fs.readFileSync(goldFile)).digest('hex'),
  summaries,rows,assertions,broken_links:data.broken_links};
fs.writeFileSync(output,zlib.gzipSync(JSON.stringify(report)),{flag:'wx'});
console.log(JSON.stringify({sources:report.source_count,evidence:report.evidence_count,assertions:assertions.length,
  summaries:summaries.filter(s=>s.scenario==='all'),graph_errors:rows.filter(r=>r.graph_error).map(r=>({id:r.id,error:r.graph_error}))},null,2));
