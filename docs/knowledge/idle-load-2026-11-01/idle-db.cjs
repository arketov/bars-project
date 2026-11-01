const {DatabaseSync}=require('node:sqlite');
const fs=require('node:fs');
const projects=['C:/Users/arketov/Desktop/bars-project','C:/Users/arketov/Desktop/cyberProject/ai-mutiplayer-bots'];
const out=[];
for(const project of projects){
 console.error('opening '+project);
 const db=new DatabaseSync(project+'/.bars/index.sqlite',{readOnly:true});
 console.error('opened');
 db.exec('PRAGMA foreign_keys=ON; BEGIN');
 const tables=['graph_meta','graph_sources','graph_nodes','graph_edges','graph_evidence','graph_edge_evidence','graph_node_sources'];
 const info={utc:new Date().toISOString(),project,tables:tables.map(name=>{console.error('count '+name);return {name,count:db.prepare(`SELECT count(*) AS n FROM ${name}`).get().n}}),metadata:db.prepare('SELECT key,length(value) AS bytes FROM graph_meta ORDER BY key').all(),schema:db.prepare("SELECT name,sql FROM sqlite_master WHERE (type='table' OR type='index' OR type='trigger') AND name LIKE 'graph_%'").all(),deletes:tables.slice(1).map(name=>{console.error('explain '+name);return {name,plan:db.prepare(`EXPLAIN QUERY PLAN DELETE FROM ${name}`).all()}})};
 out.push(info);db.exec('ROLLBACK');db.close();
}
fs.writeFileSync('docs/knowledge/idle-load-2026-11-01/sql-schema.json',JSON.stringify(out,null,2));
console.log(JSON.stringify(out.map(({project,tables,metadata,deletes})=>({project,tables,metadata:metadata.filter(x=>x.bytes>10000),deletes})),null,2));
