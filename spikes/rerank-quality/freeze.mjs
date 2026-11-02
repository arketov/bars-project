// Research corpus: frozen retrieval, no writes to source projects or their indexes.
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {DatabaseSync} from 'node:sqlite';

export const sha = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
export const read = file => JSON.parse(fs.readFileSync(file, 'utf8'));
export const write = (file, value) => fs.writeFileSync(file, JSON.stringify(value, null, 2)+'\n', {flag:'wx'});
export function inside(file) {
  const resolved=path.resolve(file);
  assert(resolved.startsWith(process.cwd()+path.sep), 'Output must be inside checkout');
  return resolved;
}
export function freeze(out, snapshot) {
  out=inside(out);
  assert(!fs.existsSync(out), 'New evidence directory required');
  const casesFile='crates/bars-core/examples/quality/overdrive.json';
  const cases=read(casesFile);
  assert.equal(cases.length,100);
  const db=new DatabaseSync(path.join(snapshot,'index.sqlite'),{readOnly:true});
  const chunk=db.prepare('SELECT d.path,c.heading_chain,c.first_line,c.last_line,c.text FROM chunks c JOIN documents d ON c.document_id=d.id WHERE c.id=?');
  const inputs=[],baseline=[],sources=[];
  try {
    for(const split of ['tune','test']) {
      const file=`docs/knowledge/quality-2026-10-18/${split==='tune'?'train':'test'}-rows.jsonl.gz`;
      const bytes=fs.readFileSync(file);
      sources.push({file,sha256:sha(bytes)});
      const rows=zlib.gunzipSync(bytes).toString('utf8').trim().split('\n').map(JSON.parse);
      for(const c of cases.filter(c=>c.split===split)) {
        const pool=rows.filter(r=>r.id===c.id&&r.mode==='search'&&r.k===0&&r.repeat===0&&r.candidates===30);
        assert.equal(pool.length,1,`One retrieval row: ${c.id}`);
        const hits=pool[0].hits.slice(0,15);
        const documents=hits.map(h=>{
          const row=chunk.get(h.id);
          assert(row&&row.path===h.path&&row.heading_chain===h.heading&&row.first_line===h.first&&row.last_line===h.last,`Candidate mismatch ${c.id}/${h.id}`);
          const relevant=c.gold.flatMap((g,i)=>g.path===row.path&&row.text.includes(g.evidence)?[i]:[]);
          assert.deepEqual(relevant,h.relevant,`Gold mismatch ${c.id}/${h.id}`);
          return `${row.path}\n${row.heading_chain}\n${row.text}`;
        });
        assert.equal(hits.length,15);
        inputs.push({...c,suite:'retrieval',hits,documents});
        const reranked=rows.filter(r=>r.id===c.id&&r.mode==='search'&&r.k===15&&r.repeat===0&&r.candidates===30);
        assert.equal(reranked.length,1);
        assert.deepEqual([...hits.map(h=>h.id)].sort((a,b)=>a-b),reranked[0].hits.map(h=>h.id).sort((a,b)=>a-b));
        baseline.push({id:c.id,scores:hits.map(h=>reranked[0].hits.find(r=>r.id===h.id).score)});
      }
    }
  } finally { db.close(); }
  // Controlled tail diagnostic, not a retrieval benchmark. Gold and filler frozen before scores.
  const registry=read('crates/bars-core/examples/quality/registry-bars.json');
  const selected=['tune','test'].flatMap(split=>registry.filter(c=>c.split===split&&c.gold.length).slice(0,4));
  const filler='| Служебная заметка | Материал посвящён оформлению страницы: абзацы, шрифты, отступы и цвет фона. |\n';
  const padding=filler.repeat(Math.ceil(4800/filler.length)).slice(0,4800);
  for(const c of selected) {
    const g=c.gold[0],source=fs.readFileSync(g.path,'utf8');
    const lines=source.split(/\r?\n/).filter(line=>line.includes(g.evidence));
    assert.equal(lines.length,1,`One unchanged registry row ${c.id}`);
    const line=lines[0];
    assert(line.length<1100, 'Controlled document must fit 6000 characters');
    const documents=[line+'\n'+padding,padding+'\n'+line,padding+'\n'+filler];
    inputs.push({...c,id:'tail-'+c.id,suite:'controlled-registry',source_sha256:sha(source),
      target_offsets:[0,padding.length+1,null],documents,
      hits:documents.map((_,i)=>({id:i,relevant:i<2?[0]:[]}))});
  }
  fs.mkdirSync(out,{recursive:true});
  write(path.join(out,'inputs.json'),inputs);
  write(path.join(out,'historical-baseline.json'),baseline);
  write(path.join(out,'protocol.json'),{version:1,created:new Date().toISOString(),cases_sha256:sha(fs.readFileSync(casesFile)),sources,
    inputs_sha256:sha(fs.readFileSync(path.join(out,'inputs.json'))),retrieval_cases:100,controlled_registry_cases:8,
    thresholds:[0,.02,.05,.1,.3,.5,.7,.9],selection:'tune only: minimize false nonempty with zero top-3 losses; tie smallest threshold',
    caveats:['Known historical split, not blind holdout','Controlled registry pairs are not end-to-end retrieval','No fresh transformers reference','No product model/default changes']});
  console.log(`Frozen ${inputs.length} cases`);
}
if(process.argv[1]&&path.resolve(process.argv[1])===path.resolve(import.meta.filename)) {
  const [out,snapshot]=process.argv.slice(2);
  assert(out&&snapshot,'output-directory snapshot-workspace');
  freeze(out,snapshot);
}
