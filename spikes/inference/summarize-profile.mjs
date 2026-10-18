import fs from 'node:fs';
import path from 'node:path';
const [dir]=process.argv.slice(2);
const observations=JSON.parse(fs.readFileSync(path.join(dir,'observations.json'),'utf8'));
const file=fs.readdirSync(dir).find(f=>f.startsWith('ort_')&&f.endsWith('.json'));
if(!file)throw Error('missing ORT trace');
const trace=JSON.parse(fs.readFileSync(path.join(dir,file),'utf8'));
const kernels=trace.filter(e=>e.cat==='Node'&&e.args?.provider&&e.name.endsWith('_kernel_time'));
const grouped=new Map();
for(const e of kernels){const key=`${e.args.provider}/${e.args.op_name}`;const g=grouped.get(key)??{events:0,microseconds:0};g.events++;g.microseconds+=e.dur;grouped.set(key,g);}
const providers={};for(const e of kernels){const p=providers[e.args.provider]??{events:0,microseconds:0};p.events++;p.microseconds+=e.dur;providers[e.args.provider]=p;}
const summary={config:observations.config,load:observations.load,profileRows:observations.rows.length,providers,
  operators:[...grouped].map(([name,stats])=>({name,...stats})).sort((a,b)=>b.microseconds-a.microseconds),
  scope:'diagnostic trace; event durations are not uninstrumented benchmark latency or GPU hardware counters'};
fs.writeFileSync(path.join(dir,'summary.json'),JSON.stringify(summary,null,2)+'\n');console.log(JSON.stringify({...summary,operators:summary.operators.slice(0,8)},null,2));
