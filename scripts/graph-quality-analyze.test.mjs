import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import zlib from 'node:zlib';
import {spawnSync} from 'node:child_process';

test('evaluation preserves UTF-8 spans, atomic paths, missing roles and empty denominators',()=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'bars-graph-evaluation-'));
  const text='Модель не требует сети.\n',bytes=Buffer.from(text),needle='не требует сети';
  const first=Buffer.byteLength(text.slice(0,text.indexOf(needle)));
  const ev={id:'ev',document:{id:'s'},source_hash:'revision',span:{start:0,end:bytes.length,first_line:1,last_line:1},quote:text};
  const source={meta:{reference:{id:'s'},path:'docs/feature.md',docs_path:'feature.md',hash:'revision'},text};
  const data={snapshot:'snapshot',sources:[source],nodes:[],edges:[],evidence:[ev],broken_links:[]};
  const gold={budget_bytes:1024,cases:[{id:'case',split:'dev',scenario:'S3',kind:'conditional',evidence:[{
    path:'feature.md',needle,first_line:1,start:first,end:first+Buffer.byteLength(needle),sha256:crypto.createHash('sha256').update(bytes).digest('hex'),role:'constraint'}]},
    {id:'empty',split:'holdout',scenario:'S8',kind:'negative',evidence:[]}]};
  const graph={items:[{type:'role_group',role:'behavior',evidence:['ev']},
    {type:'path',path:{nodes:[],evidence:['ev'],steps:[],complete:true,padding:'x'.repeat(3000)}}],
    evidence:[ev],nodes:[],sources:[source.meta],layer_state:'partial',missing_roles:['constraint'],truncated:false};
  const variants=['hybrid','hybrid_wide','structural','mix'].map(name=>({name,hits:[{path:'docs/feature.md',text,first_line:1,last_line:1}]}));
  variants.push({name:'mix_rerank',hits:[],related:{...graph,items:[graph.items[0]]}});
  const results={snapshot:'snapshot',rows:[{id:'case',graph,variants},{id:'empty',graph:{...graph,items:[]},variants}]};
  for(const [file,value] of [['dataset.json',data],['gold.json',gold],['results.json',results]]) fs.writeFileSync(path.join(root,file),JSON.stringify(value));
  const run=spawnSync(process.execPath,['scripts/graph-quality-analyze.mjs',root,path.join(root,'gold.json'),path.join(root,'results.json'),path.join(root,'analysis.gz')],{encoding:'utf8'});
  assert.equal(run.status,0,run.stderr);
  const report=JSON.parse(zlib.gunzipSync(fs.readFileSync(path.join(root,'analysis.gz'))));
  assert.equal(report.rows[0].variants.graph.recall,1);
  assert.equal(report.rows[0].variants.graph.correct_role_found,0);
  assert.equal(report.rows[0].variants.graph.skipped,1);
  assert(report.rows[0].variants.graph.bytes<=gold.budget_bytes);
  assert.equal(report.rows[1].variants.graph.recall,null);
  assert.equal(report.rows[0].variants.mix_rerank_hits.found,0);
  assert.equal(report.rows[0].variants.mix_rerank.found,1);
  assert.equal(report.summaries.find(s=>s.split==='dev'&&s.scenario==='S3'&&s.name==='mix_rerank').found,1);
  // An incomplete returned proof must not be reconstructed from the full corpus silently.
  graph.evidence=[];
  fs.writeFileSync(path.join(root,'results.json'),JSON.stringify(results));
  const missing=spawnSync(process.execPath,['scripts/graph-quality-analyze.mjs',root,path.join(root,'gold.json'),path.join(root,'results.json'),path.join(root,'missing.gz')],{encoding:'utf8'});
  assert.notEqual(missing.status,0);
  graph.evidence=[ev];
  fs.writeFileSync(path.join(root,'results.json'),JSON.stringify(results));
  // Corrupt source evidence must make the measurement fail, not silently score it.
  data.evidence[0].quote='Неверная цитата';
  fs.writeFileSync(path.join(root,'dataset.json'),JSON.stringify(data));
  const bad=spawnSync(process.execPath,['scripts/graph-quality-analyze.mjs',root,path.join(root,'gold.json'),path.join(root,'results.json'),path.join(root,'invalid.gz')],{encoding:'utf8'});
  assert.notEqual(bad.status,0);
});
