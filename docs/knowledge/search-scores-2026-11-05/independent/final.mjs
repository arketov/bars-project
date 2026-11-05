// (d) Перекрёстная проверка выбора среди немногих заранее названных вариантов; итоговые таблицы
// «RRF / старт / кандидаты» на всех 168 и на чётных, по группам и проектам; правило приёмки; потери поимённо.
//   node final.mjs
import { load, ranks, summ, fmt, rr, compare, falseShare } from './common.mjs';
const Q = load();
const M = Q.main, N = Q.neg;

const S = { rerank: true, K: 15, wc: 0, wf: 0.1, wp: 0.1, fold: 0 };
const FAMILY = [
  ['старт', S], ['окно по RRF', { ...S, sel: 'rrf' }], ['окно по очереди', { ...S, sel: 'union' }], ['10 по S0 + RRF', { ...S, sel: 'mix', kS0: 10 }],
  ['w_c 0,1', { ...S, wc: 0.1 }], ['w_c 0,3', { ...S, wc: 0.3 }], ['w_f 0,15 w_p 0,05', { ...S, wf: 0.15, wp: 0.05 }], ['w_c 0,1 w_p 0,05', { ...S, wc: 0.1, wp: 0.05 }],
];
// --- 1. CV по семейству (θ = 0.2) ---
{
  const th = 0.2;
  const R = FAMILY.map(([, c]) => ranks(M, { ...c, theta: th }).map(rr));
  const mean = (row, idx) => idx.reduce((s, j) => s + row[j], 0) / idx.length;
  const pick = (idx) => { let bi = 0; R.forEach((row, i) => { if (mean(row, idx) > mean(R[bi], idx) + 0.002) bi = i; }); return bi; };
  const all = [...M.keys()], by = (f) => all.filter((j) => f(M[j]));
  console.log(`### Выбор среди ${FAMILY.length} заранее названных вариантов (K = 15, θ = 0,2): обучение → проверка\n\n| Деление | Выбран | Δ MRR на проверке к старту |\n|---|---|---|`);
  const folds = [['нечётные → чётные', by((q) => q.meta.split === 'tune'), by((q) => q.meta.split === 'test')], ['чётные → нечётные', by((q) => q.meta.split === 'test'), by((q) => q.meta.split === 'tune')]];
  for (const p of [...new Set(M.map((q) => q.meta.project))]) folds.push([`4 проекта → ${p}`, by((q) => q.meta.project !== p), by((q) => q.meta.project === p)]);
  for (const [n, tr, te] of folds) { const bi = pick(tr); const d = mean(R[bi], te) - mean(R[0], te); console.log(`| ${n} | ${FAMILY[bi][0]} | ${d >= 0 ? '+' : ''}${d.toFixed(3)} |`); }
  let x = 99; const rnd = () => { x = (x * 1103515245 + 12345) & 0x7fffffff; return x / 0x7fffffff; };
  const ds = [], picks = {};
  for (let t = 0; t < 200; t++) {
    const tr = [], te = [];
    for (const p of [...new Set(M.map((q) => q.meta.project))]) by((q) => q.meta.project === p).map((j) => [rnd(), j]).sort((a, b) => a[0] - b[0]).forEach(([, j], k, arr) => (k < arr.length / 2 ? tr : te).push(j));
    const bi = pick(tr); picks[FAMILY[bi][0]] = (picks[FAMILY[bi][0]] ?? 0) + 1; ds.push(mean(R[bi], te) - mean(R[0], te));
  }
  ds.sort((a, b) => a - b);
  console.log(`\n200 случайных делений: Δ MRR на проверке ${(ds.reduce((s, v) => s + v, 0) / 200).toFixed(4)} (5–95 %: ${ds[10].toFixed(3)} … ${ds[189].toFixed(3)}), лучше старта ${ds.filter((d) => d > 0).length}, хуже ${ds.filter((d) => d < 0).length}; выбирались: ${JSON.stringify(picks)}`);
}

// --- 2. итоговые таблицы ---
const RRF = { rrfBase: true, rerank: true, K: 15, theta: 0.1 };
const V = [
  ['RRF→rerank (база, θ 0,1)', RRF],
  ['старт (θ 0,1)', { ...S, theta: 0.1 }],
  ['A: старт, θ 0,2', { ...S, theta: 0.2 }],
  ['B: окно по RRF, θ 0,2', { ...S, sel: 'rrf', theta: 0.2 }],
  ['C: K 25, θ 0,2', { ...S, K: 25, theta: 0.2 }],
];
const NR = [['RRF без rerank', { rrfBase: true, rerank: false, theta: 0 }], ['старт c+0,1f+0,1p, θ 0,2', { rerank: false, wf: 0.1, wp: 0.1, fold: 0, theta: 0.2 }]];
const rk = new Map([...V, ...NR].map(([n, c]) => [n, ranks(M, c)]));
const sub = (r, f) => summ(r.filter((_, i) => f(M[i])));
console.log(`\n### Итог: первым / Hit@3 / MRR@10; ложные ответы на 54 без ответа\n\n| Вариант | Все 168 | Нечётные | Чётные (проверка) | Ложные |\n|---|---|---|---|---|`);
for (const [n, c] of [...V, ...NR]) { const r = rk.get(n); console.log(`| ${n} | ${fmt(summ(r))} | ${fmt(sub(r, (q) => q.meta.split === 'tune'))} | ${fmt(sub(r, (q) => q.meta.split === 'test'))} | ${(100 * falseShare(N, c)).toFixed(0)} % |`); }
for (const key of ['group', 'project']) {
  console.log(`\n| ${key === 'group' ? 'Группа' : 'Проект'} | N | ${V.map(([n]) => n).join(' | ')} |\n|---|---|${V.map(() => '---').join('|')}|`);
  const vals = [...new Set(M.map((q) => q.meta[key]))].sort((a, b) => M.filter((q) => q.meta[key] === b).length - M.filter((q) => q.meta[key] === a).length);
  for (const g of vals) console.log(`| ${g} | ${M.filter((q) => q.meta[key] === g).length} | ${V.map(([n]) => fmt(sub(rk.get(n), (q) => q.meta[key] === g))).join(' | ')} |`);
}
// --- 3. приёмка и потери ---
function accept(name, baseName, scope) {
  const a = rk.get(baseName), b = rk.get(name);
  const qs = scope === 'test' ? (q) => q.meta.split === 'test' : () => true;
  const fails = [];
  const check = (label, f) => {
    const ma = sub(a, (q) => qs(q) && f(q)), mb = sub(b, (q) => qs(q) && f(q));
    const d1 = 100 * (mb.first - ma.first), d3 = 100 * (mb.hit3 - ma.hit3), dm = mb.mrr - ma.mrr;
    if (d3 < -1 - 1e-9 || dm < -0.01 - 1e-9 || d1 < -1 - 1e-9) fails.push(`${label} (N ${ma.n}: первым ${d1.toFixed(1)} п.п., Hit@3 ${d3.toFixed(1)} п.п., MRR ${dm.toFixed(3)})`);
  };
  check('в целом', () => true);
  for (const g of [...new Set(M.map((q) => q.meta.group))]) if (M.filter((q) => q.meta.group === g).length >= 10) check(`группа ${g}`, (q) => q.meta.group === g);
  for (const p of [...new Set(M.map((q) => q.meta.project))]) check(`проект ${p}`, (q) => q.meta.project === p);
  const idx = M.map((q, i) => i).filter((i) => qs(M[i]));
  const c = compare(idx.map((i) => M[i]), idx.map((i) => a[i]), idx.map((i) => b[i]));
  console.log(`\n**${name} против «${baseName}»${scope === 'test' ? ' (только чётные)' : ' (все 168)'}**: ${fails.length ? 'нарушения: ' + fails.join('; ') : 'правило приёмки (первым, Hit@3 ≤ 1 п.п., MRR ≤ 0,01; целое, группы ≥ 10, проекты) соблюдено'}.`);
  console.log(`Hit@3 потеряно ${c.lost3.length} (${c.lost3.join(' ')}), выиграно ${c.won3.length} (${c.won3.join(' ')}), знаки p = ${c.p3.toFixed(2)}; первым −${c.lost1.length} +${c.won1.length}; RR лучше:хуже ${c.rwin.length}:${c.rloss.length} (p = ${c.pRR.toFixed(2)}); Δ MRR ${c.bs.mean.toFixed(3)} [95 %: ${c.bs.lo.toFixed(3)}; ${c.bs.hi.toFixed(3)}], P(Δ ≤ 0) = ${c.bs.pLe0.toFixed(2)}.`);
}
for (const n of ['старт (θ 0,1)', 'A: старт, θ 0,2', 'B: окно по RRF, θ 0,2', 'C: K 25, θ 0,2']) { accept(n, 'RRF→rerank (база, θ 0,1)', 'all'); accept(n, 'RRF→rerank (база, θ 0,1)', 'test'); }
for (const n of ['A: старт, θ 0,2', 'B: окно по RRF, θ 0,2', 'C: K 25, θ 0,2']) accept(n, 'старт (θ 0,1)', 'all');
accept('старт c+0,1f+0,1p, θ 0,2', 'RRF без rerank', 'all');
accept('старт c+0,1f+0,1p, θ 0,2', 'RRF без rerank', 'test');
