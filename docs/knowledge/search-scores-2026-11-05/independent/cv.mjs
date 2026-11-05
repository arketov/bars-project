// (a) Чувствительность и парные взаимодействия на всех 168; перекрёстная проверка выбора по сетке:
// половины, leave-one-project-out, 200 случайных делений пополам (по проектам поровну).
//   node cv.mjs
import { load, ranks, summ, fmt, rr, bootstrap } from './common.mjs';
const Q = load();
const M = Q.main;
const START = { rerank: true, K: 15, wc: 0, wf: 0.1, wp: 0.1, fold: 0, theta: 0 };
const START_NR = { rerank: false, wf: 0.1, wp: 0.1, fold: 0, theta: 0 };
const m = (cfg, qs = M) => summ(ranks(qs, cfg));

function oneAtATime(base, axes, label) {
  console.log(`\n### Чувствительность (${label}), все 168, θ = 0; MRR (первым/Hit@3)\n\n| Параметр | значения |\n|---|---|`);
  for (const [k, vs] of Object.entries(axes)) console.log(`| ${k} | ${vs.map((v) => { const x = m({ ...base, [k]: v }); return `${v}: **${x.mrr.toFixed(3)}** (${(100 * x.first).toFixed(0)}/${(100 * x.hit3).toFixed(0)})`; }).join('; ')} |`);
}
function pair(base, ka, va, kb, vb, label) {
  console.log(`\n#### ${label}: MRR на 168 (строки ${ka}, столбцы ${kb})\n\n| ${ka} \\ ${kb} | ${vb.join(' | ')} |\n|---|${vb.map(() => '---').join('|')}|`);
  for (const a of va) console.log(`| ${a} | ${vb.map((b) => m({ ...base, [ka]: a, [kb]: b }).mrr.toFixed(3)).join(' | ')} |`);
}

oneAtATime(START, { wc: [0, 0.05, 0.1, 0.2, 0.3, 0.5, 0.7, 1], wf: [0, 0.05, 0.1, 0.15, 0.2, 0.3, 0.5], wp: [0, 0.05, 0.1, 0.15, 0.2, 0.3, 0.5], fold: [0, 0.02, 0.05, 0.1], K: [10, 15, 20, 25, 30, 40] }, 'rerank, старт r + 0,1f + 0,1p, K = 15');
pair(START, 'wf', [0, 0.05, 0.1, 0.15, 0.2, 0.3], 'wp', [0, 0.05, 0.1, 0.15, 0.2, 0.3], 'rerank w_f × w_p');
pair(START, 'wc', [0, 0.1, 0.2, 0.3, 0.5, 1], 'wf', [0, 0.05, 0.1, 0.15, 0.2, 0.3], 'rerank w_c × w_f');
pair(START, 'K', [10, 15, 20, 30], 'wf', [0, 0.05, 0.1, 0.15, 0.2, 0.3], 'rerank K × w_f');
pair(START, 'fold', [0, 0.05, 0.1], 'wf', [0, 0.05, 0.1, 0.15, 0.2, 0.3], 'rerank fold × w_f');
oneAtATime(START_NR, { wf: [0, 0.05, 0.1, 0.15, 0.2, 0.3, 0.5], wp: [0, 0.05, 0.1, 0.15, 0.2, 0.3, 0.5], fold: [0, 0.02, 0.05, 0.1] }, 'без rerank, старт c + 0,1f + 0,1p');
pair(START_NR, 'wf', [0, 0.05, 0.1, 0.15, 0.2, 0.3, 0.5], 'wp', [0, 0.05, 0.1, 0.15, 0.2, 0.3, 0.5], 'без rerank w_f × w_p');

// ---- перекрёстная проверка по матрице RR (схема × запрос) ------------------------------------------
function gridOf(base, axes) {
  let out = [base];
  for (const [k, vs] of Object.entries(axes)) out = out.flatMap((c) => vs.map((v) => ({ ...c, [k]: v })));
  return out;
}
const cplx = (c) => [c.wc, c.wf, c.wp, c.fold].filter((x) => x > 0).length * 10 + (c.wc ?? 0) + c.wf + c.wp + c.fold;
const txt = (c) => `wc=${c.wc ?? '-'} wf=${c.wf} wp=${c.wp} fold=${c.fold}${c.rerank ? ` K=${c.K}` : ''}`;
function cv(label, cfgs, start) {
  const R = cfgs.map((c) => ranks(M, c).map(rr));
  const S = ranks(M, start).map(rr);
  const mean = (row, idx) => idx.reduce((s, j) => s + row[j], 0) / idx.length;
  const pick = (idx) => {
    const sc = R.map((row) => mean(row, idx));
    const best = Math.max(...sc);
    let bi = -1;
    sc.forEach((v, i) => { if (best - v <= 0.002 && (bi < 0 || cplx(cfgs[i]) < cplx(cfgs[bi]))) bi = i; });
    return bi;
  };
  const all = [...M.keys()];
  const by = (f) => all.filter((j) => f(M[j]));
  console.log(`\n### Перекрёстная проверка: ${label} (${cfgs.length} схем; выбор — лучший MRR на обучении, при разнице ≤ 0,002 — самая простая)\n\n| Обучение → проверка | Выбрано | MRR обуч.: выбор / старт | MRR проверки: выбор / старт | Δ проверки |\n|---|---|---|---|---|`);
  const folds = [['нечётные → чётные', by((q) => q.meta.split === 'tune'), by((q) => q.meta.split === 'test')], ['чётные → нечётные', by((q) => q.meta.split === 'test'), by((q) => q.meta.split === 'tune')]];
  for (const p of [...new Set(M.map((q) => q.meta.project))]) folds.push([`4 проекта → ${p}`, by((q) => q.meta.project !== p), by((q) => q.meta.project === p)]);
  let lopo = 0, lopoN = 0;
  for (const [name, tr, te] of folds) {
    const bi = pick(tr);
    const d = mean(R[bi], te) - mean(S, te);
    if (name.startsWith('4')) { lopo += d * te.length; lopoN += te.length; }
    console.log(`| ${name} | ${txt(cfgs[bi])} | ${mean(R[bi], tr).toFixed(3)} / ${mean(S, tr).toFixed(3)} | ${mean(R[bi], te).toFixed(3)} / ${mean(S, te).toFixed(3)} | ${d >= 0 ? '+' : ''}${d.toFixed(3)} |`);
  }
  // 200 случайных делений пополам, поровну в каждом проекте
  let x = 12345; const rnd = () => { x = (x * 1103515245 + 12345) & 0x7fffffff; return x / 0x7fffffff; };
  const deltas = [], opt = [];
  const projects = [...new Set(M.map((q) => q.meta.project))];
  for (let t = 0; t < 200; t++) {
    const tr = [], te = [];
    for (const p of projects) {
      const js = by((q) => q.meta.project === p).map((j) => [rnd(), j]).sort((a, b) => a[0] - b[0]).map((y) => y[1]);
      js.forEach((j, k) => (k < js.length / 2 ? tr : te).push(j));
    }
    const bi = pick(tr);
    deltas.push(mean(R[bi], te) - mean(S, te));
    opt.push(mean(R[bi], tr) - mean(S, tr));
  }
  deltas.sort((a, b) => a - b);
  const avg = (a) => a.reduce((s, v) => s + v, 0) / a.length;
  console.log(`\nLOPO, взвешенно по запросам: Δ MRR выбора против старта = ${(lopo / lopoN >= 0 ? '+' : '') + (lopo / lopoN).toFixed(4)}.`);
  console.log(`200 случайных делений: на обучении выбор лучше старта в среднем на +${avg(opt).toFixed(3)} (оптимизм), на проверке Δ = ${avg(deltas) >= 0 ? '+' : ''}${avg(deltas).toFixed(4)} (5–95 %: ${deltas[10].toFixed(3)} … ${deltas[189].toFixed(3)}); выбор лучше старта на проверке в ${deltas.filter((d) => d > 0).length} из 200, хуже в ${deltas.filter((d) => d < 0).length}.`);
  // лучшая схема на всех 168 (самообман-контроль: это максимум, а не оценка)
  const bi = pick(all);
  const b = bootstrap(S, R[bi]);
  console.log(`Лучшая на всех 168 (in-sample максимум): ${txt(cfgs[bi])} — ${fmt(m(cfgs[bi]))} против старта ${fmt(m(start))}; Δ MRR ${b.mean.toFixed(3)}, 95 % бутстрап ${b.lo.toFixed(3)} … ${b.hi.toFixed(3)}.`);
}
cv('rerank, K = 15', gridOf({ ...START }, { wc: [0, 0.05, 0.1, 0.2, 0.3, 0.5, 1], wf: [0, 0.05, 0.1, 0.15, 0.2, 0.3], wp: [0, 0.05, 0.1, 0.15, 0.2, 0.3], fold: [0, 0.05, 0.1] }), START);
cv('rerank, K ∈ {10, 15, 20, 30}', gridOf({ ...START }, { wc: [0, 0.1, 0.2, 0.3, 0.5], wf: [0, 0.05, 0.1, 0.15, 0.2, 0.3], wp: [0, 0.05, 0.1, 0.15, 0.2], fold: [0, 0.05], K: [10, 15, 20, 30] }), START);
cv('без rerank', gridOf({ ...START_NR }, { wf: [0, 0.05, 0.1, 0.15, 0.2, 0.3, 0.5], wp: [0, 0.05, 0.1, 0.15, 0.2, 0.3, 0.5], fold: [0, 0.05, 0.1] }), START_NR);
