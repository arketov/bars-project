import fs from 'node:fs';
const [casesFile, ...dirs] = process.argv.slice(2);
if (!casesFile || !dirs.length) throw Error('analyze.mjs <cases.json> <run-dir>...');
const cases = JSON.parse(fs.readFileSync(casesFile, 'utf8').replace(/^\uFEFF/, ''));
const quantile = (v, q) => [...v].sort((a,b)=>a-b)[Math.ceil(q*v.length)-1];
const results = dirs.map(dir => {
  const cfg = JSON.parse(fs.readFileSync(`${dir}/config.json`, 'utf8'));
  const rows = fs.readFileSync(`${dir}/rows.jsonl`, 'utf8').trim().split(/\r?\n/).map(JSON.parse);
  const expected = new Set(cases.flatMap(c => [0,1,2].map(r=>`${c.id}:${r}`)));
  for (const r of rows) {
    if (!expected.delete(`${r.id}:${r.repeat}`)) throw Error(`duplicate or unknown ${dir}:${r.id}:${r.repeat}`);
    if (!Number.isFinite(r.total) || r.hits.some(h=>!Number.isFinite(h.score))) throw Error('nonfinite');
  }
  if (expected.size) throw Error(`incomplete ${dir}: ${expected.size}`);
  const scores = {};
  for (const split of ['tune', 'test']) {
    const positive = cases.filter(c=>c.split===split && c.gold.length);
    const rr = positive.map(c=>{
      const row=rows.find(r=>r.id===c.id && r.repeat===0);
      const rank=row.hits.findIndex(h=>h.relevant.length)+1;
      return {id:c.id, rank};
    });
    const times=rows.filter(r=>cases.some(c=>c.id===r.id && c.split===split)).map(r=>r.total);
    scores[split]={n:positive.length, hit1:rr.filter(r=>r.rank===1).length, hit3:rr.filter(r=>r.rank>0&&r.rank<=3).length, hit10:rr.filter(r=>r.rank>0&&r.rank<=10).length, mrr10:rr.reduce((s,r)=>s+(r.rank>0&&r.rank<=10?1/r.rank:0),0)/rr.length, median:quantile(times,.5),p95:quantile(times,.95), ranks:rr};
  }
  return {dir,config:cfg,scores};
});
if (new Set(results.map(r=>r.config.cases_blake3)).size!==1) throw Error('different cases');
console.log(JSON.stringify(results,null,2));
