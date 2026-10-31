import fs from 'fs'; import path from 'path';
const root = process.argv[2];
const dirs = fs.readdirSync(root,{withFileTypes:true}).filter(d=>d.isDirectory()).map(d=>d.name).sort();
const body = t => t.replace(/^---[\s\S]*?---\s*/,'').split(/\r?\n/);
const entries = lines => { // entries: list items or dated paragraphs; block = consecutive non-empty, non-heading lines
  const out=[]; let cur=null;
  for (const l of lines){ if(/^#/.test(l)||!l.trim()){ if(cur){out.push(cur);cur=null;} continue;}
    if(/^\s*[-*] |^\d{2}-\d{2}-\d{4}|^\d+\. /.test(l)&&!/^\s{2,}/.test(l)){ if(cur)out.push(cur); cur={lines:1,chars:l.length}; }
    else if(cur){cur.lines++;cur.chars+=l.length;} else {cur={lines:1,chars:l.length};} }
  if(cur)out.push(cur); return out; };
const rows=[];
for(const d of dirs){ const p=path.join(root,d); const r={d,files:fs.readdirSync(p).length};
  const lp=path.join(p,'log.md'), hp=path.join(p,'handoff.md');
  if(fs.existsSync(lp)){ const L=body(fs.readFileSync(lp,'utf8')); const e=entries(L); r.logEntries=e.length; r.logLines=L.filter(x=>x.trim()&&!/^#/.test(x)).length; r.logChars=e.reduce((a,b)=>a+b.chars,0); r.longEntries=e.filter(x=>x.chars>600).length; r.maxEntry=Math.max(0,...e.map(x=>x.chars)); }
  if(fs.existsSync(hp)){ const t=fs.readFileSync(hp,'utf8'); const H=body(t); r.hLines=H.filter(x=>x.trim()).length; r.hChars=H.join('\n').length;
    const hs=H.filter(x=>/^## /.test(x)).map(x=>x.toLowerCase());
    r.hSec=['сделано','не сделано','сложност','дальше'].map(k=>hs.some(h=>h.includes(k))?1:0).join('');
    r.hLinks=(t.match(/\]\([^)]+\)/g)||[]).length; r.hPrevRef=/\.\.\/20\d\d-/.test(t)?1:0; }
  rows.push(r);}
const q=(a,p)=>{a=[...a].sort((x,y)=>x-y);return a[Math.floor((a.length-1)*p)];};
const s=k=>{const a=rows.map(r=>r[k]).filter(x=>x!==undefined);return `${k}: n=${a.length} med=${q(a,.5)} p90=${q(a,.9)} max=${Math.max(...a)} sum=${a.reduce((x,y)=>x+y,0)}`;};
for(const k of ['files','logEntries','logLines','logChars','maxEntry','longEntries','hLines','hChars','hLinks']) console.log(s(k));
const sec={}; rows.forEach(r=>{if(r.hSec)sec[r.hSec]=(sec[r.hSec]||0)+1}); console.log('sections сд/не/слож/дал', sec);
console.log('handoff refs prev session', rows.filter(r=>r.hPrevRef).length);
const byDay={}; rows.forEach(r=>{const day=r.d.slice(0,10); byDay[day]=(byDay[day]||0)+1}); console.log('per day', JSON.stringify(byDay));
console.log('top handoffs by chars', rows.filter(r=>r.hChars).sort((a,b)=>b.hChars-a.hChars).slice(0,6).map(r=>r.d+':'+r.hChars+'/'+r.hLines).join(' '));
console.log('top logs by entries', rows.filter(r=>r.logEntries).sort((a,b)=>b.logEntries-a.logEntries).slice(0,6).map(r=>r.d+':'+r.logEntries+'/'+r.logChars).join(' '));
console.log('logs with 0 entries', rows.filter(r=>r.logEntries===0).map(r=>r.d).join(' '));
