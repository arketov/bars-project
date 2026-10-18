const fs = require('node:fs');
const path = require('node:path');
const readline = require('node:readline');
const crypto = require('node:crypto');
const cutoff = '2026-10-18T13:45:00Z';
const base = (process.argv[2] || process.env.USERPROFILE).replaceAll('\\','/');
const roots = ['.codex/sessions', '.codex/archived_sessions', '.claude/projects'];
const toolRe = /tools\.(?:mcp__bars__)?(bars_(?:rag_search|outline|read|projects|status|reindex))\s*\(/g;
function files(dir) { if (!fs.existsSync(dir)) return []; return fs.readdirSync(dir, {withFileTypes:true}).flatMap(e => e.isDirectory() ? files(path.join(dir,e.name)) : e.name.endsWith('.jsonl') ? [path.join(dir,e.name)] : []); }
// Decode only literals. Never execute code read from a transcript.
function literal(src) {
 let i=0; const ws=()=>{while (/\s/.test(src[i]||'') && i<src.length)i++;};
 function val(){ws();let c=src[i];if(c==='"'||c==="'"){let q=c,s='';i++;while(i<src.length){let ch=src[i++];if(ch===q)return s;if(ch==='\\'){let a=src[i++];if(a==='u'){s+=String.fromCharCode(parseInt(src.slice(i,i+4),16));i+=4;}else if(a==='x'){s+=String.fromCharCode(parseInt(src.slice(i,i+2),16));i+=2;}else s+=({n:'\n',r:'\r',t:'\t',b:'\b',f:'\f'}[a]??a);}else s+=ch;}throw Error('string');}
 if(c==='{'){i++;let o={};ws();while(src[i]!=='}'){ws();let k;if(src[i]==='"'||src[i]==="'")k=val();else {let m=/^[A-Za-z_$][\w$]*/.exec(src.slice(i));if(!m)throw Error('key');k=m[0];i+=k.length;}ws();if(src[i++]!==':')throw Error('colon');o[k]=val();ws();if(src[i]===','){i++;ws();}else if(src[i]!=='}')throw Error('object');}i++;return o;}
 if(c==='['){i++;let a=[];ws();while(src[i]!==']'){a.push(val());ws();if(src[i]===','){i++;ws();}else if(src[i]!==']')throw Error('array');}i++;return a;}
 let m=/^(true|false|null|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)/.exec(src.slice(i));if(!m)throw Error('dynamic');i+=m[0].length;return JSON.parse(m[0]);}
 let v=val();ws();if(![',',')',undefined].includes(src[i]))throw Error('expression');return {value:v,end:i};
}
function textOutput(o){if(typeof o==='string')return o;if(Array.isArray(o))return o.map(x=>x.text||'').join('\n');return JSON.stringify(o);}
function blocks(text){let out=[];function walk(o){if(!o)return;if(typeof o==='string'){if(/^(=== (ПОИСК|ДОКУМЕНТ|СТРУКТУРА|ПРОЕКТ|СОСТОЯНИЕ|ПЕРЕИНД)|Ошибка bars|tool (?:call )?error:)/.test(o.trim()))out.push(o);else if(o.trim().startsWith('{')){try{const j=JSON.parse(o);if(j.results&&j.applied)out.push(o);else if(j.path&&j.state)out.push(o);else if(j.projects)out.push(o);}catch{}}return;}if(Array.isArray(o)){o.forEach(walk);return;}for(const [k,v] of Object.entries(o))walk(v);}
 for(const l of text.split('\n')){try{walk(JSON.parse(l));}catch{for(const m of l.matchAll(/"text"\s*:\s*("(?:[^"\\]|\\.)*")/g)){try{walk(JSON.parse(m[1]))}catch{}}}}
 for(const m of text.matchAll(/^=== (?:ПОИСК|ДОКУМЕНТ|СТРУКТУРА|ПРОЕКТЫ|СОСТОЯНИЕ ИНДЕКСА|ПЕРЕИНД)[^\n]*/gm)){const tail=text.slice(m.index);const next=/\n=== (?:ПОИСК|ДОКУМЕНТ|СТРУКТУРА|ПРОЕКТЫ|СОСТОЯНИЕ ИНДЕКСА|ПЕРЕИНД)/.exec(tail);out.push(tail.slice(0,next?next.index:undefined));}
 return out;
}
(async()=>{
 let calls=[], groups=[], manifest=[], malformed=0, claude=0;
 for(const root of roots)for(const file of files(path.join(base,root))){
  let line=0,sid=path.basename(file,'.jsonl'),cwd=null,outputs=new Map(),local=[],waits=[];const hash=crypto.createHash('sha256');const size=fs.statSync(file).size;
  // Fixed byte length: appending sessions cannot move the snapshot boundary.
  const stream=fs.createReadStream(file,{end:size-1});
  stream.on('data',b=>hash.update(b));
  for await(const s of readline.createInterface({input:stream,crlfDelay:Infinity})){line++;let o;try{o=JSON.parse(s)}catch{malformed++;continue;}
   if(o.timestamp&&o.timestamp>cutoff)continue;
   const z=o.payload||o;
   if(o.type==='session_meta'){sid=z.id||sid;cwd=z.cwd||cwd;}
   if(o.type==='turn_context')cwd=z.cwd||cwd;
   if(z.type==='custom_tool_call_output'||z.type==='function_call_output'){let prev=outputs.get(z.call_id);outputs.set(z.call_id,{line,text:(prev?prev.text+'\n':'')+textOutput(z.output)});}
   let sites=[];
   if(z.type==='custom_tool_call'||z.type==='function_call'){
    if(z.name==='wait'||z.name==='functions.wait'){try{waits.push({line,callId:z.call_id,args:JSON.parse(z.arguments||z.input)})}catch{}}
    if(/^(?:mcp__bars__)?bars_(rag_search|outline|read|projects|status|reindex)$/.test(z.name||'')){let args;try{args=JSON.parse(z.arguments||z.input||'{}')}catch{}sites.push({tool:z.name.replace('mcp__bars__',''),args,raw:z.arguments||z.input});}
    else if(z.name==='exec'||z.name==='functions.exec'){
     const code=z.input||z.arguments||'';toolRe.lastIndex=0;let m;
     while((m=toolRe.exec(code))){let args,end;try{({value:args,end}=literal(code.slice(toolRe.lastIndex)))}catch{}sites.push({tool:m[1],args,raw:code.slice(toolRe.lastIndex,toolRe.lastIndex+(end||200)),dynamic:args===undefined});}
    }
   }
   if(o.type==='assistant')for(const b of o.message?.content||[])if(b.type==='tool_use'&&/bars_/i.test(b.name)){claude++;sites.push({tool:b.name,args:b.input});}
   if(sites.length){let group={file:file.replaceAll('\\','/').replace(base+'/',''),line,sid,cwd,time:o.timestamp,callId:z.call_id,code:z.input||z.arguments||'',sites};local.push(group);}
  }
  manifest.push({file:file.replaceAll('\\','/').replace(base+'/',''),bytes:size,sha256:hash.digest('hex'),lines:line});
  for(const g of local){g.output=outputs.get(g.callId)||null;const cell=/Script running with cell ID (\S+)/.exec(g.output?.text||'')?.[1];if(cell){g.waitLines=[];for(const w of waits.filter(w=>w.line>g.line&&String(w.args.cell_id)===cell)){const o=outputs.get(w.callId);if(o){g.output.text+='\n'+o.text;g.waitLines.push(o.line);if(!/Script running with cell ID/.test(o.text))break;}}}groups.push(g);for(let j=0;j<g.sites.length;j++)calls.push({...g.sites[j],group:groups.length-1,site:j,sid:g.sid,time:g.time,file:g.file,line:g.line,cwd:g.cwd});}
 }

 const d={cutoff,malformed,claude,manifest,groups,calls};
 const freq=xs=>Object.fromEntries([...xs.reduce((m,x)=>m.set(String(x),(m.get(String(x))||0)+1),new Map())].sort((a,b)=>b[1]-a[1]));
 let errors=[],searches=[];
 groups.forEach((g,i)=>{g.blocks=blocks(g.output?.text||'');for(const b of g.blocks){
 if(/^Ошибка bars|^tool (?:call )?error:/.test(b)&&!(/tool (?:call )?error:/.test(b)&&!b.includes('bars/'))){errors.push({group:i,kind:/Ошибка bars \(([^)]+)/.exec(b)?.[1]||(b.includes('Transport closed')?'transport_closed':'other'),message:b.replaceAll(base,'$HOME').replaceAll(base.replaceAll('/','\\'),'$HOME')});}
 if(/^=== ПОИСК:/.test(b))searches.push({group:i,count:Number(/^=== ПОИСК: результатов (\d+)/.exec(b)[1]),topPath:/^path: (.+)$/m.exec(b)?.[1],minScore:/min_score: (.+)/.exec(b)?.[1],rerankError:/rerank_error: (.+)/.exec(b)?.[1]});
 }});
 const staticCalls=calls.filter(c=>c.args),search=staticCalls.filter(c=>c.tool==='bars_rag_search');
 const result={cutoff,scope:'Local Codex and Claude JSONL; syntactic call sites, not runtime invocation totals',manifest,summary:{files:manifest.length,malformed,claudeBarsCalls:claude,groups:groups.length,sites:calls.length,sessions:new Set(groups.map(g=>g.sid)).size,duplicateCallIds:groups.length-new Set(groups.map(g=>g.callId)).size,staticSites:staticCalls.length,dynamicSites:calls.length-staticCalls.length,sitesByTool:freq(calls.map(c=>c.tool)),staticSearchSites:search.length,searchKeys:freq(search.flatMap(c=>Object.keys(c.args))),limits:freq(search.map(c=>c.args.limit??'default')),formats:freq(search.map(c=>c.args.format??'default')),broad:search.filter(c=>!c.args.files?.length&&!c.args.modules?.length).length,noCyrillic:search.filter(c=>!/[А-Яа-яЁё]/.test(c.args.query)).length,groupsWithTruncation:groups.filter(g=>/Warning: truncated output/.test(g.output?.text||'')).length,groupsWithoutRecognizedResponse:groups.filter(g=>!g.blocks.length).length,observedTextSearches:searches.length,emptySearches:searches.filter(s=>s.count===0).length,topChats:searches.filter(s=>/^(docs\/)?chats\//.test(s.topPath)).length,rerankFallbacks:searches.filter(s=>s.rerankError).length,errors:freq(errors.map(e=>e.kind))},errors,searches,groups:groups.map((g,i)=>({group:i,file:g.file,line:g.line,waitLines:g.waitLines||[],session:g.sid,time:g.time,tools:g.sites.map(s=>({tool:s.tool,literalArguments:!!s.args,keys:s.args?Object.keys(s.args):undefined,limit:s.args?.limit,format:s.args?.format,files:s.args?.files,modules:s.args?.modules,min_score:s.args?.min_score})),truncated:/Warning: truncated output/.test(g.output?.text||''),responseCount:g.blocks.length}))};
 if(result.summary.sites!==Object.values(result.summary.sitesByTool).reduce((a,b)=>a+b,0))throw Error('site totals mismatch');
 if(result.summary.staticSites+result.summary.dynamicSites!==result.summary.sites)throw Error('argument totals mismatch');
 fs.writeFileSync(path.join(__dirname,'evidence.json'),JSON.stringify(result,null,2)+'\n');
 console.log(JSON.stringify(result.summary,null,2));
})();
