// Проверка сигнала связей l конвейером lib.mjs (w_l), метрики — свои.
import { load, rankOf, summ, fmt, compare } from './common.mjs';
import { run as libRun } from './lib.mjs';
const Q = load(); const M = Q.main;
for (const [label, base] of [['rerank', { rerank: true, K: 15, wc: 0, wf: 0.1, wp: 0.1, fold: 0, theta: 0.2 }], ['без rerank', { rerank: false, K: 15, wc: 1, wf: 0.1, wp: 0.1, fold: 0, theta: 0 }]]) {
  const r0 = M.map((q) => rankOf(q, libRun(q, { ...base, wl: 0 })));
  const out = [0.02, 0.05, 0.1, 0.2].map((wl) => { const r = M.map((q) => rankOf(q, libRun(q, { ...base, wl }))); const c = compare(M, r0, r); return `w_l ${wl}: ${fmt(summ(r))} (Hit@3 +${c.won3.length}/−${c.lost3.length}, Δ MRR ${c.bs.mean.toFixed(3)} [${c.bs.lo.toFixed(3)}; ${c.bs.hi.toFixed(3)}])`; });
  console.log(`${label}: w_l 0: ${fmt(summ(r0))}; ${out.join('; ')}`);
}
