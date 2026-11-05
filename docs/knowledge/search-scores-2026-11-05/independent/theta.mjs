// (c) Порог θ по опоре: кривая «ложные ответы на 54 запросах без ответа — потери на 168».
//   node theta.mjs
import { load, ranks, summ, falseShare, run } from './common.mjs';
const Q = load();
const M = Q.main, N = Q.neg;
const pct = (x) => (100 * x).toFixed(0);
const split = (qs, s) => qs.filter((q) => q.meta.split === s);
const proj = (qs, p) => qs.filter((q) => q.meta.project === p);

function curve(label, base, thetas) {
  console.log(`\n### ${label}\n\n| θ | Ложные, все 54 | bars (34) | overdrive (20) | подбор / проверка | Потеряно Hit@3 к θ = 0 (поимённо) | Hit@3 | MRR | первым |\n|---|---|---|---|---|---|---|---|---|`);
  const ref = ranks(M, { ...base, theta: 0 });
  for (const theta of thetas) {
    const cfg = { ...base, theta };
    const r = ranks(M, cfg), m = summ(r);
    const lost = M.filter((q, i) => ref[i] <= 3 && r[i] > 3).map((q) => q.meta.id);
    console.log(`| ${theta} | ${pct(falseShare(N, cfg))} % | ${pct(falseShare(proj(N, 'bars-project'), cfg))} | ${pct(falseShare(proj(N, 'overdrive-bots'), cfg))} | ${pct(falseShare(split(N, 'tune'), cfg))} / ${pct(falseShare(split(N, 'test'), cfg))} | ${lost.length}${lost.length ? ` (${lost.join(' ')})` : ''} | ${pct(m.hit3)} | ${m.mrr.toFixed(3)} | ${pct(m.first)} |`);
  }
}
const R = { rerank: true, K: 15, wc: 0, wf: 0.1, wp: 0.1, fold: 0 };
const RT = [0, 0.05, 0.1, 0.15, 0.2, 0.25, 0.3, 0.35, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9];
curve('rerank, стартовые веса, K = 15', R, RT);
curve('rerank, выбор окна по RRF, S = r + 0,1f + 0,1p', { ...R, sel: 'rrf' }, [0, 0.1, 0.15, 0.2, 0.25, 0.3]);
curve('rerank, K = 25', { ...R, K: 25 }, [0, 0.1, 0.15, 0.2, 0.25, 0.3]);
curve('прежний RRF → rerank', { rrfBase: true, rerank: true, K: 15 }, [0, 0.1, 0.15, 0.2, 0.25, 0.3]);
curve('без rerank, опора — косинус c', { rerank: false, wf: 0.1, wp: 0.1, fold: 0 }, [0, 0.3, 0.4, 0.45, 0.5, 0.55, 0.6, 0.65, 0.7, 0.75]);

// Разделимость: максимум опоры на запрос (лучший кусок) у запросов с ответом и без; AUC.
function sep(label, anchor) {
  const pos = M.map(anchor), neg = N.map(anchor);
  let auc = 0;
  for (const a of pos) for (const b of neg) auc += a > b ? 1 : a === b ? 0.5 : 0;
  auc /= pos.length * neg.length;
  const q = (arr, p) => { const s = [...arr].sort((x, y) => x - y); return s[Math.floor(p * (s.length - 1))].toFixed(2); };
  console.log(`| ${label} | ${auc.toFixed(2)} | ${q(pos, 0.05)} / ${q(pos, 0.5)} | ${q(neg, 0.5)} / ${q(neg, 0.95)} |`);
}
console.log('\n### Разделимость запросов с ответом и без по максимуму опоры в выдаче\n\n| Опора | AUC | с ответом: 5 % / медиана | без ответа: медиана / 95 % |\n|---|---|---|---|');
const maxR = (q) => { const secs = run(q, { ...R, theta: 0 }); return secs.length ? Math.max(...secs.flatMap((s) => s.chunks.map((i) => q.r[i]))) : 0; };
const maxC = (q) => Math.max(0, ...Array.from(q.c));
sep('rerank r (окно 15 по S0)', maxR);
sep('косинус c (весь пул)', maxC);
// косинус по проектам: у каждого проекта своя модель/корпус — шкала разная
console.log('\nМаксимум косинуса по проектам (медиана с ответом / медиана без ответа): ' + ['bars-project', 'overdrive-bots'].map((p) => {
  const med = (a) => { const s = [...a].sort((x, y) => x - y); return s[Math.floor(s.length / 2)].toFixed(2); };
  return `${p}: ${med(proj(M, p).map(maxC))} / ${med(proj(N, p).map(maxC))}`;
}).join('; ') + '; остальные проекты с ответом: ' + ['RoleGPT', 'agaugu', 'tmc-opovechenie'].map((p) => { const s = proj(M, p).map(maxC).sort((x, y) => x - y); return `${p} ${s[Math.floor(s.length / 2)].toFixed(2)}`; }).join(', '));
