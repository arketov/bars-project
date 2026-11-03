// exp3.mjs — абсолютная шкала полнотекстового балла: n = min(1, bm25 / идеал запроса), идеал = Σ idf(основа)·5·(k1+1) по основам запроса.
// Зачем: порог (Т-24) должен работать и когда нет ни rerank, ни векторов. Проверяем: (1) разделяются ли по n правильный и неправильный первый результат;
// (2) годится ли n как слагаемое вместе с rerank на выдачах cli-full.json. Запуск: cd scripts; TOK_NPM=$PWD node exp3.mjs > ../exp3.txt; mv exp3.json ..
import fs from 'node:fs';
import { loadProject as lp, buildFts, poolFor, wl, stem, PROJECTS } from './feat.mjs';
import { score, agg, pct } from '../../tokenization-2026-11-03/scripts/lib.mjs';

const data = PROJECTS.map(lp);
const items = [];
for (const p of data) {
  const d = buildFts(p.chunks);
  d.exec("create virtual table v using fts5vocab(f,'row')");
  const df = new Map(d.prepare('select term, doc from v').all().map((r) => [r.term, r.doc]));
  const N = p.chunks.length;
  const byPos = new Map(p.chunks.map((c, i) => [c.path + '|' + c.fl, i]));
  for (const q of p.queries) {
    const pool = poolFor(d, p.chunks, q.query, 300);
    const stems = [...new Set(wl(q.query).map(stem))];
    const ideal = stems.reduce((s, t) => { const n = df.get(t) || 0; return s + (n ? Math.log(1 + (N - n + 0.5) / (n + 0.5)) : 0); }, 0) * 5 * 2.2;
    const cli = p.cli[q.id];
    items.push({ q, proj: p.proj, pool, ideal, cli: cli && !cli.error ? cli : null, byPos, chunks: p.chunks, poolIdx: new Map(pool.map((r, i) => [r.idx, i])) });
  }
  d.close();
}
const toRows = (cs) => cs.map((c) => ({ path: c.path, text: c.t }));
const fmt = (a) => `${pct(a.first)}/${pct(a.hit3)}/${a.mrr.toFixed(3)}`;
const out = {};

// (1) верхний результат полнотекстового поиска: правильный или нет, и его абсолютная оценка n
console.log('=== 1. верхний результат bm25(5,3,1,0.3): доля идеала n у правильных и неправильных ===');
const rows = items.filter((it) => it.pool.length).map((it) => {
  const top = it.pool[0], ok = score(it.q, toRows([top.c])).found === 1;
  return { id: it.q.id, group: it.q.group, ok, n: Math.min(1, top.bc / (it.ideal || 1)), nQ: wl(it.q.query).length };
});
const qs = (a, p) => (a.length ? a.sort((x, y) => x - y)[Math.min(a.length - 1, Math.floor(p * a.length))].toFixed(3) : '—');
const okN = rows.filter((r) => r.ok).map((r) => r.n), badN = rows.filter((r) => !r.ok).map((r) => r.n);
console.log('правильный первый: n', okN.length, 'квантили 10/25/50/75%', [0.1, 0.25, 0.5, 0.75].map((p) => qs([...okN], p)).join(' / '));
console.log('неправильный первый: n', badN.length, 'квантили 10/25/50/75%', [0.1, 0.25, 0.5, 0.75].map((p) => qs([...badN], p)).join(' / '));
out.top1 = { ok: okN.length, bad: badN.length };
out.thr = {};
for (const t of [0.05, 0.1, 0.15, 0.2, 0.3, 0.4]) {
  const keptOk = okN.filter((x) => x >= t).length, keptBad = badN.filter((x) => x >= t).length;
  console.log(`порог n≥${t}: остаётся правильных первых ${keptOk}/${okN.length}, неправильных ${keptBad}/${badN.length}`);
  out.thr[t] = { keptOk, keptBad };
}

// (2) rerank + абсолютный полнотекстовый балл на выдачах CLI
console.log('\n=== 2. rerank + w·n_abs на выдачах CLI (10 кандидатов), первым % / Hit@3 % / MRR ===');
const cand = items.filter((it) => it.cli).map((it) => ({
  it,
  rows: it.cli.results.map((r) => { const idx = it.byPos.get(r.path + '|' + r.first_line), pi = idx === undefined ? undefined : it.poolIdx.get(idx), f = pi === undefined ? null : it.pool[pi]; return { c: idx === undefined ? { path: r.path, t: r.text } : it.chunks[idx], rr: r.score, nabs: f ? Math.min(1, f.bc / (it.ideal || 1)) : 0, rel: f ? f.bc / (it.pool[0].bc || 1) : 0 }; }),
}));
const run = (fn) => agg(cand.map((x) => score(x.it.q, toRows(x.rows.map((r) => [fn(r), r]).sort((a, b) => b[0] - a[0]).map(([, r]) => r.c)))));
console.log('только rerank'.padEnd(40), fmt(run((r) => r.rr)));
out.sum = {};
for (const w of [0.1, 0.2, 0.3, 0.5, 0.75, 1]) {
  const a = run((r) => r.rr + w * r.nabs), b = run((r) => r.rr + w * r.rel);
  console.log(`rerank + ${w}·n_abs`.padEnd(40), fmt(a), ' | rerank + w·relmax', fmt(b));
  out.sum[w] = { nabs: a, relmax: b };
}
fs.writeFileSync('exp3.json', JSON.stringify(out));
