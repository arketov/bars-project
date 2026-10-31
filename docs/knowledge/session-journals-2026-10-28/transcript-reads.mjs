import fs from 'fs'; import path from 'path'; import os from 'os';
const home=os.homedir(); const files=[];
for(const d of fs.readdirSync(path.join(home,'.claude/projects'))) if(/bars-project/.test(d)) for(const f of fs.readdirSync(path.join(home,'.claude/projects',d))) if(f.endsWith('.jsonl')) files.push(path.join(home,'.claude/projects',d,f));
const S={sess:0,readOther:0,readOtherFirst15:0,otherCalls:0,otherChars:0,ownReads:0,logOther:0,logOtherChars:0,writes:0, sessWithWrite:0, searchChats:0, ragSearch:0, ragHitsChatsInTop:0};
const re=/chats[\/]+(\d{4}-\d{2}-\d{2}-[a-z0-9-]+)[\/]+(handoff|log)\.md/i;
for(const f of files){ const txt=fs.readFileSync(f,'utf8'); const calls=[]; const res={};
  for(const line of txt.split('\n')){ if(!line)continue; let o; try{o=JSON.parse(line)}catch{continue}; const c=o.message?.content; if(!Array.isArray(c))continue;
    for(const b of c){ if(b.type==='tool_use') calls.push({id:b.id,name:b.name,s:JSON.stringify(b.input||{}).replace(/\\/g,'/')}); if(b.type==='tool_result'){res[b.tool_use_id]=typeof b.content==='string'?b.content:JSON.stringify(b.content||'');}}}
  if(!calls.length) continue; S.sess++;
  const own=new Set(); calls.forEach(c=>{const m=c.s.match(re); if(m&&(/Write|Edit/.test(c.name)||/>>|Add-Content|Set-Content|cat >/.test(c.s))) own.add(m[1]);});
  if(own.size) S.sessWithWrite++;
  let ro=0, first=-1;
  calls.forEach((c,i)=>{ const m=c.s.match(re); if(/bars_rag_search/.test(c.name)){S.ragSearch++; if(/"modules":\[[^\]]*chats/.test(c.s))S.searchChats++; const r=res[c.id]||''; const top=(r.match(/module: (\w+)/g)||[]).slice(0,5); if(top.includes('module: chats')) S.ragHitsChatsInTop++;}
    if(!m) return; const w=/Write|Edit/.test(c.name)||/>>|Add-Content|Set-Content|cat >/.test(c.s); if(w){S.writes++;return;}
    if(own.has(m[1])){S.ownReads++;return;}
    if(m[2]==='handoff'){ro++;S.otherCalls++;S.otherChars+=(res[c.id]||'').length; if(first<0)first=i;} else {S.logOther++; S.logOtherChars+=(res[c.id]||'').length;} });
  if(ro){S.readOther++; if(first<15)S.readOtherFirst15++;}
}
console.log(JSON.stringify(S,null,0));
