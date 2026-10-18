import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {DatabaseSync} from 'node:sqlite';
const [exe,workspace,casesFile,archivesCsv,out]=process.argv.slice(2);
const call=(...args)=>{const r=spawnSync(exe,args,{encoding:'utf8'});if(r.status!==0)throw Error(r.stderr);return r.stdout.trim();};
call('verify',workspace);
assert(!fs.existsSync(out),'new data directory required');fs.mkdirSync(out,{recursive:true});
fs.copyFileSync(path.join(workspace,'index.sqlite'),path.join(out,'index.sqlite'));
const db=new DatabaseSync(path.join(out,'index.sqlite'),{readOnly:true});
const query=db.prepare('SELECT d.path,c.heading_chain,c.first_line,c.last_line,c.text FROM chunks c JOIN documents d ON d.id=c.document_id WHERE c.id=?');
const all=archivesCsv.split(',').flatMap(p=>zlib.gunzipSync(fs.readFileSync(p)).toString().trim().split('\n').map(JSON.parse));
const cases=JSON.parse(fs.readFileSync(casesFile,'utf8'));
const inputs=cases.map(c=>{
  const rows=all.filter(r=>r.mode==='fused'&&r.candidates===30&&r.id===c.id&&r.repeat===0);
  assert.equal(rows.length,1,`missing/duplicate frozen pool ${c.id}`);
  const hits=rows[0].hits.slice(0,15);
  const documents=hits.map(h=>{
    const row=query.get(h.id);assert(row);assert.equal(row.path,h.path);assert.equal(row.heading_chain,h.heading);
    assert.equal(row.first_line,h.first);assert.equal(row.last_line,h.last);
    return `${row.path}\n${row.heading_chain}\n${row.text}`;
  });
  return {id:c.id,query:c.query,split:c.split,gold_count:c.gold.length,hits,documents};
});
db.close();
fs.writeFileSync(path.join(out,'inputs.json'),JSON.stringify(inputs,null,2));
fs.copyFileSync(path.join(workspace,'manifest.json'),path.join(out,'source-manifest.json'));
fs.copyFileSync(casesFile,path.join(out,'cases.json'));
fs.writeFileSync(path.join(out,'manifest.json'),JSON.stringify({source:workspace,candidatesSource:'frozen stage9 fused rows; C30 first15',
  cases_blake3:call('digest',casesFile),inputs_blake3:call('digest',path.join(out,'inputs.json')),
  index_blake3:call('digest',path.join(out,'index.sqlite')),archives:archivesCsv.split(',').map(p=>({path:p,blake3:call('digest',p)})),count:inputs.length},null,2));
console.log(`prepared ${inputs.length} frozen query/pool pairs`);
