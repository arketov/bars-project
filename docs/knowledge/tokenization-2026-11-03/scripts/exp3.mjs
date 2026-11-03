// exp3.mjs — опечатки: исправление слов запроса по словарю индекса (расстояние Дамерау—Левенштейна) вторым проходом против триграмм.
// Два набора: 5 настоящих опечаток бенча и синтетические — в каждом из 168 запросов портится самое длинное слово (≥6 знаков).
import fs from 'node:fs';
import { DatabaseSync } from 'node:sqlite';
import { PROJECTS, loadProject, evaluate, summarize, line, score, agg, pct } from './lib.mjs';
import * as A from './analyzers.mjs';
const data = PROJECTS.map(loadProject);
const wl = A.words, stem = A.stemRules;
const mapW = (t, f) => wl(t).map(f).join(' ');

function osa(a, b, max) {
  const m = a.length, n = b.length; if (Math.abs(m - n) > max) return max + 1;
  let p2 = null, p1 = Array.from({ length: n + 1 }, (_, j) => j);
  for (let i = 1; i <= m; i++) {
    const cur = [i]; let rowMin = i;
    for (let j = 1; j <= n; j++) {
      let v = Math.min(p1[j] + 1, cur[j - 1] + 1, p1[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
      if (i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) v = Math.min(v, p2[j - 2] + 1);
      cur.push(v); if (v < rowMin) rowMin = v;
    }
    if (rowMin > max) return max + 1;
    p2 = p1; p1 = cur;
  }
  return p1[n];
}
function fuzzyVariant(name, correct) {
  const base = A.makeFts({ name, ncols: 2, cols: (c) => [mapW(c.h, stem), mapW(c.t, stem)], weights: [5, 1],
    qbuild: (q, ctx) => {
      const terms = wl(q).filter((x) => !A.STOP.has(x.toLowerCase())).map(stem);
      if (!correct) return A.orTerms([0, 1], terms);
      const voc = ctx.getVocab(); const extra = [];
      if (!ctx.byLen) { ctx.byLen = new Map(); for (const [t, df] of voc) { const k = t.length; (ctx.byLen.get(k) || ctx.byLen.set(k, []).get(k)).push([t, df]); } }
      for (const t of terms) {
        if (voc.has(t) || t.length < 4 || /\d/.test(t)) continue;
        const max = t.length >= 8 ? 2 : 1; const cands = [];
        for (let L = t.length - max; L <= t.length + max; L++) for (const [v, df] of ctx.byLen.get(L) || []) { const dd = osa(t, v, max); if (dd <= max) cands.push([dd, -df, v]); }
        cands.sort((x, y) => x[0] - y[0] || x[1] - y[1]);
        for (const c of cands.slice(0, 3)) extra.push(c[2]);
      }
      return A.orTerms([0, 1], [...terms, ...extra]);
    } });
  return base;
}
function trigramGrams(name) {
  return { name,
    build(chunks) {
      const d = new DatabaseSync(':memory:');
      d.exec('create virtual table f using fts5(c0,c1,tokenize="trigram")');
      const ins = d.prepare('insert into f(rowid,c0,c1) values (?,?,?)');
      d.exec('begin'); chunks.forEach((c, i) => ins.run(i, c.h, c.t)); d.exec('commit');
      return { d, chunks };
    },
    search(ctx, query, limit) {
      const terms = [];
      for (const w of wl(query)) { const l = w.toLowerCase(); if (l.length < 3 || A.STOP.has(l)) continue; for (let i = 0; i + 3 <= l.length; i++) terms.push('"' + l.slice(i, i + 3) + '"'); }
      const ts = [...new Set(terms)]; if (!ts.length) return [];
      return ctx.d.prepare('select rowid from f where f match ? order by bm25(f,3,1) limit ?').all(ts.join(' OR '), limit).map((r) => ctx.chunks[r.rowid]);
    } };
}
// синтетические опечатки: 0 перестановка двух соседних внутренних, 1 пропуск буквы, 2 замена внутренней буквы
function mutate(query, k) {
  const ws = query.split(/(\s+)/); let bi = -1, bl = 0;
  ws.forEach((w, i) => { const c = [...w].filter((x) => /\p{L}/u.test(x)).length; if (/^\p{L}+$/u.test(w) && c >= 6 && c > bl) { bl = c; bi = i; } });
  if (bi < 0) return query;
  const w = [...ws[bi]]; const m = Math.floor(w.length / 2);
  if (k === 0) [w[m], w[m + 1]] = [w[m + 1], w[m]]; else if (k === 1) w.splice(m, 1); else w[m] = w[m] === 'а' ? 'о' : w[m] === 'a' ? 'e' : String.fromCharCode(w[m].charCodeAt(0) + 1);
  ws[bi] = w.join(''); return ws.join('');
}
const mdata = data.map((p) => ({ ...p, queries: p.queries.map((q, i) => ({ ...q, query: mutate(q.query, i % 3), orig: q.query })) }));
const changed = mdata.flatMap((p) => p.queries).filter((q) => q.query !== q.orig).length;
console.log('синтетических опечаток:', changed, 'из', 168);

const variants = [fuzzyVariant('FTS без исправления', false), fuzzyVariant('FTS + словарь Дамерау–Левенштейна', true), trigramGrams('trigram, все триграммы OR')];
const out = {};
const cache = {};
for (const v of variants) {
  const clean = evaluate(data, v, cache);
  const mut = evaluate(mdata, v, cache);
  const typoOnly = (res) => agg(res.filter((r) => r.group === 'typo').map((r) => r.s));
  const s1 = summarize(clean), s2 = summarize(mut);
  console.log(v.name.padEnd(36), `чистые: первым ${pct(s1.overall.first)} Hit@3 ${pct(s1.overall.hit3)} MRR ${s1.overall.mrr.toFixed(3)} | с опечаткой: первым ${pct(s2.overall.first)} Hit@3 ${pct(s2.overall.hit3)} MRR ${s2.overall.mrr.toFixed(3)} | настоящие опечатки (5): первым ${pct(typoOnly(clean).first)} Hit@3 ${pct(typoOnly(clean).hit3)}`);
  out[v.name] = { clean: s1, mutated: s2 };
}
fs.writeFileSync('exp3.json', JSON.stringify(out));
