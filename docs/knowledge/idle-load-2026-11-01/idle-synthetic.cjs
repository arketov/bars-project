const {DatabaseSync}=require('node:sqlite');
const {createInterface}=require('node:readline');
const mode=process.argv[2];
const path=process.argv[3];
const schema=JSON.parse(require('node:fs').readFileSync('docs/knowledge/idle-load-2026-11-01/sql-schema.json','utf8'))[1];
const db=new DatabaseSync(path);
db.exec(`PRAGMA journal_mode=WAL; CREATE TABLE graph_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL) ${mode==='large-rowid'?'':'WITHOUT ROWID'}; CREATE TABLE graph_edges(id INTEGER PRIMARY KEY,data TEXT);`);
db.exec('BEGIN');
for(const {key,bytes} of schema.metadata){
 const size=mode==='small-without-rowid'?Math.min(bytes,100):bytes;
 db.prepare('INSERT INTO graph_meta VALUES(?,?)').run(key,key==='read_revision'?'0':'x'.repeat(size));
}
for(let i=0;i<100;i++)db.prepare('INSERT INTO graph_edges VALUES(?,?)').run(i,'edge');
db.exec(`CREATE TRIGGER graph_read_graph_edges_DELETE AFTER DELETE ON graph_edges BEGIN UPDATE graph_meta SET value=CAST(value AS INTEGER)+1 WHERE key='read_revision'; END; COMMIT; PRAGMA wal_checkpoint(TRUNCATE);`);
console.log(JSON.stringify({ready:true,mode,rows:100,sqlite:db.prepare('select sqlite_version() v').get().v}));
createInterface({input:process.stdin}).on('line',()=>{
 const cpu=process.cpuUsage(),start=performance.now();
 db.exec('DELETE FROM graph_edges');
 const result={mode,rows:100,ms:performance.now()-start,cpu:process.cpuUsage(cpu),revision:db.prepare("SELECT value FROM graph_meta WHERE key='read_revision'").get().value};
 console.log(JSON.stringify(result));
});
