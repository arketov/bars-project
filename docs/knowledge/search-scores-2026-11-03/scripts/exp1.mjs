// exp1.mjs — только полнотекстовые сигналы: нормирование (rrf/rank10/relmax/sat/z) и сумма с весами против одного bm25 с весами колонок.
// Веса подбираются по нечётным запросам (tune), проверяются на чётных (test). Печатает таблицу и пишет exp1.json (номера запросов, ранги, агрегаты).
// Запуск: cd scripts; node exp1.mjs > ../exp1.txt; mv exp1.json ..
import fs from 'node:fs';
import { loadProject as lp, buildFts, poolFor, PROJECTS } from './feat.mjs';
import { score, agg, GROUPS, pct } from '../../tokenization-2026-11-03/scripts/lib.mjs';

const data = PROJECTS.map(lp);
const items = [];
for (const p of data) {
  const d = buildFts(p.chunks);
  for (const q of p.queries) items.push({ q, proj: p.proj, pool: poolFor(d, p.chunks, q.query) });
  d.close();
}
const SIG = ['b0', 'b1', 'b2', 'b3', 'cov', 'phrase'];
const byDesc = (xs) => xs.map((x, i) => [x, i]).sort((a, b) => b[0] - a[0]);
const norms = {
  rrf: (xs) => { const r = new Array(xs.length); byDesc(xs).forEach(([x, i], k) => { r[i] = x > 0 ? 61 / (60 + k + 1) : 0; }); return r; },
  rank10: (xs) => { const r = new Array(xs.length); byDesc(xs).forEach(([x, i], k) => { r[i] = x > 0 ? 1 / (1 + k / 10) : 0; }); return r; },
  relmax: (xs) => { const m = Math.max(...xs, 1e-9); return xs.map((x) => x / m); },
  sat: (xs) => { const nz = xs.filter((x) => x > 0).sort((a, b) => a - b); const c = nz.length ? nz[nz.length >> 1] : 1; return xs.map((x) => x / (x + c)); },
  z: (xs) => { const n = xs.length || 1, mu = xs.reduce((s, x) => s + x, 0) / n, sd = Math.sqrt(xs.reduce((s, x) => s + (x - mu) ** 2, 0) / n) || 1; return xs.map((x) => (x > 0 ? Math.max(-1, (x - mu) / sd) : -1)); },
};
const cache = {};
function cols(it, norm) {
  const m = (cache[norm] ||= new Map());
  if (!m.has(it)) {
    const c = {};
    for (const s of SIG) { const xs = it.pool.map((r) => r[s]); c[s] = s === 'cov' || s === 'phrase' ? xs : norms[norm](xs); }
    m.set(it, c);
  }
  return m.get(it);
}
function rank(it, norm, w) {
  const c = cols(it, norm);
  return it.pool.map((r, i) => [SIG.reduce((s, k, j) => s + w[j] * c[k][i], 0), i]).sort((a, b) => b[0] - a[0]).map(([, i]) => it.pool[i].c);
}
const evalAll = (fn, filter = () => true) => items.filter(filter).map((it) => ({ it, s: score(it.q, fn(it).map((c) => ({ path: c.path, text: c.t }))) }));
const aggr = (res) => agg(res.map((r) => r.s));
const fmt = (a) => `${pct(a.first)}/${pct(a.hit3)}/${a.mrr.toFixed(3)}`;
const isTune = (x) => x.q.split === 'tune', isTest = (x) => x.q.split === 'test';

const out = { variants: {}, weights: {} };
const show = (name, fn) => {
  const all = evalAll(fn), t = aggr(all.filter((r) => isTune(r.it))), v = aggr(all.filter((r) => isTest(r.it))), a = aggr(all);
  console.log(name.padEnd(50), 'все', fmt(a), '| подбор', fmt(t), '| проверка', fmt(v), '| не найден', pct(1 - a.found) + '%');
  out.variants[name] = { all: a, tune: t, test: v, ranks: Object.fromEntries(all.map((r) => [r.it.q.id, r.s.rank])) };
  return all;
};
console.log('формат: первым % / Hit@3 % / MRR@10; пул — top-100 по bm25(5,3,1,0.3); запросов', items.length, '\n');
const baseRes = show('один bm25(5,3,1,0.3)', (it) => it.pool.map((r) => r.c));
const grid = [];
for (const wN of [0.5, 1]) for (const wH of [0.5, 1]) for (const wE of [0.1, 0.3]) for (const wC of [0, 0.2, 0.5]) for (const wP of [0, 0.2, 0.5]) grid.push([wN, wH, 1, wE, wC, wP]);
const pick = (norm, g) => {
  let best = null;
  for (const w of g) { const t = aggr(evalAll((it) => rank(it, norm, w), isTune)); const key = t.mrr + t.first * 0.01; if (!best || key > best.key) best = { key, w }; }
  return best.w;
};
for (const norm of Object.keys(norms)) {
  const w = pick(norm, grid);
  out.weights[norm] = w;
  show(`сумма, ${norm}, веса по подбору`, (it) => rank(it, norm, w));
  console.log('   веса [название,заголовки,текст,точная,покрытие,фраза]:', w.join(', '));
  const w2 = pick(norm, grid.filter((g) => g[4] === 0 && g[5] === 0));
  show(`  … без покрытия и фразы (${norm})`, (it) => rank(it, norm, w2));
  console.log('   веса:', w2.join(', '));
}
const bestName = Object.keys(norms).sort((a, b) => aggr(evalAll((it) => rank(it, b, out.weights[b]), isTune)).mrr - aggr(evalAll((it) => rank(it, a, out.weights[a]), isTune)).mrr)[0];
console.log('\nлучшее нормирование по подбору:', bestName, out.weights[bestName].join(','));
const res = evalAll((it) => rank(it, bestName, out.weights[bestName]));
const by = (rs, f) => { const m = {}; for (const r of rs) (m[f(r)] ||= []).push(r.s); return Object.fromEntries(Object.entries(m).map(([k, v]) => [k, agg(v)])); };
const bg = by(res, (r) => r.it.q.group), bgBase = by(baseRes, (r) => r.it.q.group), bp = by(res, (r) => r.it.proj), bpBase = by(baseRes, (r) => r.it.proj);
console.log('\nПо группам (bm25 5/3/1/.3 → сумма):');
for (const g of Object.keys(GROUPS)) console.log(GROUPS[g].padEnd(30), String(bg[g].n).padStart(3), fmt(bgBase[g]), '→', fmt(bg[g]));
console.log('\nПо проектам:');
for (const p of PROJECTS) console.log(p.padEnd(18), fmt(bpBase[p]), '→', fmt(bp[p]));
const rb = Object.fromEntries(baseRes.map((r) => [r.it.q.id, r.s.rank || 99])), lost = [], gained = [];
for (const r of res) { const x = r.s.rank || 99, y = rb[r.it.q.id]; if (x > y && (y <= 3 || x > 3)) lost.push(r.it.q.id); else if (x < y && (x <= 3 || y > 3)) gained.push(r.it.q.id); }
console.log('\nпотеряно', lost.length, lost.join(' '), '\nвыиграно', gained.length);
Object.assign(out, { best: bestName, lost, gained, byGroup: bg, byGroupBase: bgBase, byProject: bp, byProjectBase: bpBase });
fs.writeFileSync('exp1.json', JSON.stringify(out));
