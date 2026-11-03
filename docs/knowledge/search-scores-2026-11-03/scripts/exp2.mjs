// exp2.mjs — (A) составной bm25 с весами колонок как ОДИН сигнал + покрытие + фраза;
// (B) rerank + полнотекстовый сигнал на сохранённых выдачах bars-cli (cli-full.json, топ-10 с оценками rerank): сумма против RRF;
// (C) порог по оценке rerank; (D) свёртка кусков одного раздела на тех же выдачах.
// Запуск: cd scripts; TOK_NPM=$PWD node exp2.mjs > ../exp2.txt; mv exp2.json ..
import fs from 'node:fs';
import { loadProject as lp, buildFts, poolFor, PROJECTS } from './feat.mjs';
import { score, agg, GROUPS, pct } from '../../tokenization-2026-11-03/scripts/lib.mjs';

const data = PROJECTS.map(lp);
const items = [];
for (const p of data) {
  const d = buildFts(p.chunks);
  const byPos = new Map(p.chunks.map((c, i) => [c.path + '|' + c.fl, i]));
  for (const q of p.queries) {
    const pool = poolFor(d, p.chunks, q.query, 300);
    const cli = p.cli[q.id];
    items.push({ q, proj: p.proj, pool, poolIdx: new Map(pool.map((r, i) => [r.idx, i])), cli: cli && !cli.error ? cli : null, byPos, chunks: p.chunks });
  }
  d.close();
}
const toRows = (cs) => cs.map((c) => ({ path: c.path, text: c.t }));
const aggr = (res) => agg(res.map((r) => r.s));
const fmt = (a) => `${pct(a.first)}/${pct(a.hit3)}/${a.mrr.toFixed(3)}`;
const isTune = (x) => x.q.split === 'tune', isTest = (x) => x.q.split === 'test';
const out = {};
const byDesc = (xs) => xs.map((x, i) => [x, i]).sort((a, b) => b[0] - a[0]);

// ---------- (A) составной bm25 + покрытие + фраза, пул top-100 ----------
console.log('=== A. полнотекстовые сигналы: составной bm25(5,3,1,0.3) + покрытие + фраза (пул 100), первым % / Hit@3 % / MRR ===');
const normsA = {
  relmax: (xs) => { const m = Math.max(...xs, 1e-9); return xs.map((x) => x / m); },
  sat: (xs) => { const nz = xs.filter((x) => x > 0).sort((a, b) => a - b); const c = nz.length ? nz[nz.length >> 1] : 1; return xs.map((x) => x / (x + c)); },
  rank10: (xs) => { const r = new Array(xs.length); byDesc(xs).forEach(([x, i], k) => { r[i] = x > 0 ? 1 / (1 + k / 10) : 0; }); return r; },
};
const evalA = (fn, f = () => true) => items.filter(f).map((it) => ({ it, s: score(it.q, toRows(fn(it.pool.slice(0, 100)))) }));
const rankA = (norm, wC, wP) => (pool) => {
  const n = normsA[norm](pool.map((r) => r.bc));
  return pool.map((r, i) => [n[i] + wC * r.cov + wP * r.phrase, i]).sort((a, b) => b[0] - a[0]).map(([, i]) => pool[i].c);
};
const showA = (name, fn) => {
  const all = evalA(fn), t = aggr(all.filter((r) => isTune(r.it))), v = aggr(all.filter((r) => isTest(r.it))), a = aggr(all);
  console.log(name.padEnd(52), 'все', fmt(a), '| подбор', fmt(t), '| проверка', fmt(v));
  return { all: a, tune: t, test: v, ranks: Object.fromEntries(all.map((r) => [r.it.q.id, r.s.rank])) };
};
out.A = {};
out.A.base = showA('один bm25(5,3,1,0.3)', rankA('relmax', 0, 0));
for (const norm of Object.keys(normsA)) for (const wC of [0, 0.1, 0.2, 0.4]) for (const wP of [0, 0.1, 0.2, 0.4]) {
  if (!wC && !wP) continue;
  out.A[`${norm} cov ${wC} phrase ${wP}`] = showA(`${norm}: bm25 + ${wC}·покрытие + ${wP}·фраза`, rankA(norm, wC, wP));
}

// ---------- (B) rerank + FTS на выдачах CLI ----------
console.log('\n=== B. сумма rerank и полнотекстового сигнала в уже выданных 10 кандидатах (cli-full.json) ===');
const cand = [];
let unmatched = 0, total = 0;
for (const it of items) {
  if (!it.cli) continue;
  const rows = it.cli.results.map((r) => {
    total++;
    const idx = it.byPos.get(r.path + '|' + r.first_line);
    if (idx === undefined) unmatched++;
    const pi = idx === undefined ? undefined : it.poolIdx.get(idx);
    const feat = pi === undefined ? null : it.pool[pi];
    return { r, c: idx === undefined ? { path: r.path, t: r.text, h: r.heading_chain } : it.chunks[idx], rr: r.score, bc: feat ? feat.bc : 0, cov: feat ? feat.cov : 0, phrase: feat ? feat.phrase : 0, ftsRank: pi === undefined ? 1e9 : pi + 1 };
  });
  const maxbc = it.pool.length ? it.pool[0].bc : 1;
  const sortedNz = it.pool.map((r) => r.bc).sort((a, b) => a - b);
  const med = sortedNz.length ? sortedNz[sortedNz.length >> 1] : 1;
  cand.push({ it, rows, maxbc, med });
}
console.log('кандидатов в выдачах', total, ', не сопоставлено с чанком индекса', unmatched, '(границы чанков сдвинулись)');
const evalB = (fn, f = () => true) => cand.filter((x) => f(x.it)).map((x) => ({ x, s: score(x.it.q, toRows(fn(x).map((r) => r.c))) }));
const showB = (name, fn) => {
  const all = evalB(fn), t = aggr(all.filter((r) => isTune(r.x.it))), v = aggr(all.filter((r) => isTest(r.x.it))), a = aggr(all);
  console.log(name.padEnd(56), 'все', fmt(a), '| подбор', fmt(t), '| проверка', fmt(v));
  return { all: a, tune: t, test: v, ranks: Object.fromEntries(all.map((r) => [r.x.it.q.id, r.s.rank])) };
};
const sortBy = (x, f) => x.rows.map((r) => [f(r, x), r]).sort((a, b) => b[0] - a[0]).map(([, r]) => r);
out.B = {};
out.B.rerank = showB('только rerank (порядок CLI)', (x) => x.rows);
const rrf = (x, k = 60) => {
  const ftsOrder = [...x.rows].sort((a, b) => a.ftsRank - b.ftsRank), pos = new Map(ftsOrder.map((r, i) => [r, i + 1]));
  return sortBy(x, (r, xx) => 1 / (k + xx.rows.indexOf(r) + 1) + 1 / (k + pos.get(r)));
};
out.B.rrf = showB('RRF(rerank, FTS) k=60 среди 10', (x) => rrf(x));
out.B.rrf5 = showB('RRF(rerank, FTS) k=5 среди 10', (x) => rrf(x, 5));
const nA = { relmax: (r, x) => r.bc / x.maxbc, sat: (r, x) => r.bc / (r.bc + x.med) };
const gridB = [];
for (const w of [0, 0.05, 0.1, 0.2, 0.3, 0.5, 0.75, 1]) for (const wC of [0, 0.1, 0.2]) for (const wP of [0, 0.1, 0.2]) gridB.push([w, wC, wP]);
out.Bw = {};
for (const norm of Object.keys(nA)) {
  let best = null;
  for (const [w, wC, wP] of gridB) {
    const res = evalB((x) => sortBy(x, (r, xx) => r.rr + w * nA[norm](r, xx) + wC * r.cov + wP * r.phrase), (it) => isTune({ q: it.q }));
    const t = aggr(res); const key = t.mrr + 0.01 * t.first - 0.001 * w;
    if (!best || key > best.key) best = { key, w: [w, wC, wP] };
  }
  out.Bw[norm] = best.w;
  const [w, wC, wP] = best.w;
  out.B[norm] = showB(`rerank + ${w}·bm25(${norm}) + ${wC}·покрытие + ${wP}·фраза`, (x) => sortBy(x, (r, xx) => r.rr + w * nA[norm](r, xx) + wC * r.cov + wP * r.phrase));
  for (const ww of [0.1, 0.2, 0.3, 0.5]) out.B[`${norm} ${ww}`] = showB(`  только rerank + ${ww}·bm25(${norm})`, (x) => sortBy(x, (r, xx) => r.rr + ww * nA[norm](r, xx)));
}
// по группам для лучшей суммы
{
  const [w, wC, wP] = out.Bw.relmax;
  const res = evalB((x) => sortBy(x, (r, xx) => r.rr + w * nA.relmax(r, xx) + wC * r.cov + wP * r.phrase));
  const baseR = evalB((x) => x.rows);
  const by = (rs, f) => { const m = {}; for (const r of rs) (m[f(r)] ||= []).push(r.s); return Object.fromEntries(Object.entries(m).map(([k, v]) => [k, agg(v)])); };
  const bg = by(res, (r) => r.x.it.q.group), bgb = by(baseR, (r) => r.x.it.q.group), bp = by(res, (r) => r.x.it.proj), bpb = by(baseR, (r) => r.x.it.proj);
  console.log('\nПо группам (rerank → rerank + сумма):');
  for (const g of Object.keys(GROUPS)) console.log(GROUPS[g].padEnd(30), String(bg[g].n).padStart(3), fmt(bgb[g]), '→', fmt(bg[g]));
  console.log('По проектам:'); for (const p of PROJECTS) console.log(p.padEnd(18), fmt(bpb[p]), '→', fmt(bp[p]));
  const rb = Object.fromEntries(baseR.map((r) => [r.x.it.q.id, r.s.rank || 99])), lost = [], gained = [];
  for (const r of res) { const a = r.s.rank || 99, b = rb[r.x.it.q.id]; if (a > b && (b <= 3 || a > 3)) lost.push(r.x.it.q.id); else if (a < b && (a <= 3 || b > 3)) gained.push(r.x.it.q.id); }
  console.log('потеряно', lost.length, lost.join(' '), '| выиграно', gained.length, gained.join(' '));
  out.Bdetail = { w: [w, wC, wP], byGroup: bg, byGroupBase: bgb, byProject: bp, byProjectBase: bpb, lost, gained };
}

// ---------- (C) порог по оценке rerank ----------
console.log('\n=== C. порог по оценке rerank: что отсекается ===');
const relOf = (x) => x.rows.map((r) => score({ gold: x.it.q.gold, group: x.it.q.group }, [{ path: r.c.path, text: r.c.t }]).found === 1);
const allRows = cand.flatMap((x) => { const rel = relOf(x); return x.rows.map((r, i) => ({ s: r.rr, rel: rel[i], first: i === 0, q: x.it.q.id })); });
const rels = allRows.filter((r) => r.rel).map((r) => r.s).sort((a, b) => a - b), nrels = allRows.filter((r) => !r.rel).map((r) => r.s).sort((a, b) => a - b);
const qt = (a, p) => (a.length ? a[Math.min(a.length - 1, Math.floor(p * a.length))].toFixed(3) : '—');
console.log('оценка rerank у правильных результатов: n', rels.length, 'квантили 5/25/50/75%', [0.05, 0.25, 0.5, 0.75].map((p) => qt(rels, p)).join(' / '));
console.log('оценка rerank у прочих: n', nrels.length, 'квантили 25/50/75/95%', [0.25, 0.5, 0.75, 0.95].map((p) => qt(nrels, p)).join(' / '));
out.C = {};
for (const t of [0.02, 0.05, 0.1, 0.2, 0.3, 0.5]) {
  const lostRel = rels.filter((s) => s < t).length, cutJunk = nrels.filter((s) => s < t).length;
  const qs = new Set(allRows.filter((r) => r.rel).map((r) => r.q)), kept = new Set(allRows.filter((r) => r.rel && r.s >= t).map((r) => r.q));
  console.log(`порог ${t}: правильных отсечено ${lostRel}/${rels.length} (запросов без ответа ${qs.size - kept.size}); лишних отсечено ${cutJunk}/${nrels.length}`);
  out.C[t] = { lostRel, relTotal: rels.length, cutJunk, junkTotal: nrels.length, queriesLost: qs.size - kept.size };
}

// ---------- (D) свёртка кусков раздела ----------
console.log('\n=== D. свёртка кусков одного раздела на выдачах CLI (топ-10 кусков → разделы) ===');
const secKey = (r) => r.c.path + '#' + r.c.h;
const fold = (x, bonus, cap = 1) => {
  const m = new Map();
  for (const r of x.rows) { const k = secKey(r); (m.get(k) || m.set(k, []).get(k)).push(r); }
  const secs = [...m.values()].map((rs) => {
    const best = Math.max(...rs.map((r) => r.rr)), others = rs.reduce((s, r) => s + r.rr, 0) - best;
    return { sc: best + bonus * Math.min(cap, others), rows: rs };
  }).sort((a, b) => b.sc - a.sc);
  return secs.map((s) => ({ c: { path: s.rows[0].c.path, t: s.rows.map((r) => r.c.t).join('\n') } }));
};
const evalD = (bonus, f = () => true) => cand.filter((x) => f(x.it)).map((x) => ({ x, s: score(x.it.q, toRows(fold(x, bonus).map((r) => r.c))), n: fold(x, bonus).length }));
const baseD = evalB((x) => x.rows);
out.D = {};
for (const bonus of [0, 0.05, 0.1, 0.2]) {
  const res = evalD(bonus), a = aggr(res);
  const rb = Object.fromEntries(baseD.map((r) => [r.x.it.q.id, r.s.rank || 99]));
  const lost = res.filter((r) => { const v = r.s.rank || 99; const b = rb[r.x.it.q.id]; return v > b && (b <= 3 || v > 3); }).map((r) => r.x.it.q.id);
  const worse = res.filter((r) => (r.s.rank || 99) > rb[r.x.it.q.id]).length;
  console.log(`бонус ${bonus}: ${fmt(a)} (без свёртки ${fmt(aggr(baseD))}); среднее число разделов ${(res.reduce((s, r) => s + r.n, 0) / res.length).toFixed(2)} из 10; запросов с худшим местом ${worse}; потеряно по правилу ${lost.length} ${lost.join(' ')}`);
  out.D[bonus] = { agg: a, worse, lost, avgSections: res.reduce((s, r) => s + r.n, 0) / res.length };
}
const dup = cand.filter((x) => { const k = x.rows.slice(0, 5).map(secKey); return new Set(k).size < k.length; }).length;
console.log(`запросов с повтором раздела в топ-5 куске: ${dup} из ${cand.length}`);
out.D.dup5 = dup;
fs.writeFileSync('exp2.json', JSON.stringify(out));
