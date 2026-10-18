import fs from 'node:fs';
import {DatabaseSync} from 'node:sqlite';
const root='spikes/inference/data';
const out=`${root}/scale-5000`;
if(fs.existsSync(out))throw Error('new directory required');fs.mkdirSync(out);
fs.copyFileSync(`${root}/overdrive/index.sqlite`,`${out}/index.sqlite`);
fs.copyFileSync(`${root}/overdrive/inputs.json`,`${out}/inputs.json`);
const db=new DatabaseSync(`${out}/index.sqlite`);
const original=db.prepare('SELECT c.*,d.path,d.module,d.title,d.hash,f.heading_chain AS fts_heading,f.body AS fts_body FROM chunks c JOIN documents d ON d.id=c.document_id JOIN chunks_fts f ON f.rowid=c.id ORDER BY c.id').all();
const doc=db.prepare('INSERT INTO documents(path,module,title,size,modified_ns,hash) VALUES(?,?,?,?,NULL,?)');
const chunk=db.prepare('INSERT INTO chunks(document_id,ordinal,first_line,last_line,heading_chain,text,embed_hash) VALUES(?,0,?,?,?,?,?)');
const fts=db.prepare('INSERT INTO chunks_fts(rowid,heading_chain,body) VALUES(?,?,?)');
db.exec('BEGIN');
for(let n=original.length;n<5000;n++){
  const r=original[(n-original.length)%original.length];
  const d=doc.run(`synthetic/${n}/${r.path}`,r.module,r.title,r.text.length,r.hash);
  const c=chunk.run(d.lastInsertRowid,r.first_line,r.last_line,r.heading_chain,r.text,r.embed_hash);
  fts.run(c.lastInsertRowid,r.fts_heading,r.fts_body);
}
db.exec('COMMIT');db.close();
fs.writeFileSync(`${out}/source.json`,JSON.stringify({source:'overdrive frozen index',chunks:5000,uniqueVectors:946,synthetic:true,limitation:'Repeats existing texts/vectors; scale timing only, not a new quality corpus.'},null,2));
console.log('prepared synthetic 5000-chunk scale index');
