// build.mjs <proj>: spec/<proj>.mjs -> проверяет опоры по индексу копии, пишет gold в ~/.bars/bench/<proj>/queries.json
import {DatabaseSync} from 'node:sqlite';import fs from 'node:fs';
const proj=process.argv[2];const SPEC=(process.env.USERPROFILE+'/.bars/bench/spec/').split(String.fromCharCode(92)).join('/');
const {default:spec}=await import(fs.existsSync(SPEC+proj+'.mjs')?'file:///'+SPEC+proj+'.mjs':'../spec/'+proj+'.mjs');
const d=new DatabaseSync(process.env.USERPROFILE+'/.bars/bench/'+proj+'/index.sqlite',{readOnly:true});
const out=[];let bad=0;const ids=new Set();
for(const q of spec){
 if(ids.has(q.id)){console.log('ДУБЛЬ',q.id);bad++}ids.add(q.id);
 const gold=[];
 for(const g of q.gold||[]){
  const N=x=>x.split(/\s+/).join(' ');const rows=d.prepare("select c.id,d.path,c.first_line,c.last_line,c.heading_chain h,c.text t from chunks c join documents d on d.id=c.document_id where d.path like ?").all('%'+g.doc+'%').filter(r=>N(r.t).includes(N(g.ev)));
  if(rows.length===0){console.log('НЕТ ОПОРЫ',q.id,g.doc,g.ev.slice(0,50));bad++;continue}
  if(rows.length>1){console.log('НЕ ЕДИНСТВЕННА',q.id,rows.length,g.ev.slice(0,50));bad++;}
  const r=rows[0];gold.push({path:r.path,heading:r.h.split('/').slice(-2).join('/'),first_line:r.first_line,last_line:r.last_line,evidence:g.ev});
 }
 out.push({id:q.id,project:proj,group:q.group,query:q.q,split:(parseInt(q.id.split('-')[1],10)%2===0)?'test':'tune',gold,answerable:gold.length>0});
}
fs.writeFileSync(process.env.USERPROFILE+'/.bars/bench/'+proj+'/queries.json',JSON.stringify(out,null,1));
console.log(proj,out.length,'запросов, ошибок',bad);
