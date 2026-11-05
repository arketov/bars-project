// (b) Варианты вне сетки: состав кусков для rerank при K = 15, S0 со своими весами, нормирование f/p,
// w_c до 1, w_p для запросов с идентификатором. Полнота rerank-окна — доля запросов, у которых правильный кусок
// попал в первые K (не зависит от итоговых весов).
//   node variants.mjs
import { load, ranks, summ, fmt, rr, compare, run } from './common.mjs';
const Q = load();
const M = Q.main;
const T = (s) => M.filter((q) => q.meta.split === s);
const START = { rerank: true, K: 15, wc: 0, wf: 0.1, wp: 0.1, fold: 0, theta: 0 };
const START_NR = { rerank: false, wf: 0.1, wp: 0.1, fold: 0, theta: 0 };
const RRF = { rrfBase: true, rerank: true, K: 15, theta: 0 };

// Полнота окна rerank: правильный кусок среди выбранных K (имеет r ≥ 0 и попадает в take).
function windowRecall(qs, cfg) {
  let hit = 0;
  for (const q of qs) {
    // с θ = 0 и без итогового отбора: раздел найден, если любой выбранный кусок релевантен
    const secs = run(q, { ...cfg, theta: -1 });
    if (secs.some((s) => s.chunks.some((i) => q.isRel[i]))) hit++;
  }
  return hit / qs.length;
}
const poolHas = M.filter((q) => q.isRel.some(Boolean)).length;
console.log(`Правильный кусок есть в пуле у ${poolHas} из ${M.length} запросов (потолок полноты).`);

const base = ranks(M, START);
const baseRRF = ranks(M, RRF);
function row(name, cfg) {
  const r = ranks(M, cfg);
  const c = compare(M, base, r);
  const cR = compare(M, baseRRF, r);
  const sub = (s) => summ(r.filter((_, i) => M[i].meta.split === s));
  console.log(`| ${name} | ${cfg.rerank ? (100 * windowRecall(M, cfg)).toFixed(1) : '—'} | ${fmt(summ(r))} | ${fmt(sub('tune'))} | ${fmt(sub('test'))} | ${c.bs.mean >= 0 ? '+' : ''}${c.bs.mean.toFixed(3)} [${c.bs.lo.toFixed(3)}; ${c.bs.hi.toFixed(3)}] | +${c.won3.length}/−${c.lost3.length} | ${c.rwin.length}:${c.rloss.length} p=${c.pRR.toFixed(2)} | +${cR.won3.length}/−${cR.lost3.length} |`);
}
const head = () => console.log('| Вариант | Окно rerank: правильный в K, % | Все 168 | Нечётные | Чётные | Δ MRR к старту [95 % бутстрап] | Hit@3 к старту +/− | RR лучше:хуже, знаки | Hit@3 к RRF +/− |\n|---|---|---|---|---|---|---|---|---|');

console.log('\n### С rerank, θ = 0, K = 15 (если не сказано иное)\n');
head();
row('RRF → rerank → порядок по r (прежний)', RRF);
row('старт: S0 = c+0,1f+0,1p, S = r+0,1f+0,1p', START);
row('K = 20', { ...START, K: 20 });
row('K = 25', { ...START, K: 25 });
row('K = 30', { ...START, K: 30 });
row('выбор по RRF, S = r+0,1f+0,1p', { ...START, sel: 'rrf' });
row('выбор: по очереди S0 и RRF до 15', { ...START, sel: 'union' });
for (const k of [5, 8, 10, 12]) row(`выбор: ${k} по S0 + добор по RRF`, { ...START, sel: 'mix', kS0: k });
for (const [wf0, wp0] of [[0.2, 0.1], [0.3, 0.1], [0.5, 0.1], [1, 0.1], [0.3, 0.3], [0.1, 0], [0.1, 0.3], [0.05, 0.05]]) row(`S0 = c+${wf0}f+${wp0}p, итог старт`, { ...START, wf0, wp0 });
row('S0 = 0,5c+0,1f+0,1p', { ...START, wc0: 0.5 });
for (const wc of [0.1, 0.3, 0.5, 1]) row(`w_c = ${wc}`, { ...START, wc });
row('p бинарный (только полное совпадение)', { ...START, pmode: 'bin' });
row('p бинарный, w_p = 0,05', { ...START, pmode: 'bin', wp: 0.05 });
row('f^0,5', { ...START, fpow: 0.5 });
row('f^2', { ...START, fpow: 2 });
row('f^2, w_f = 0,15', { ...START, fpow: 2, wf: 0.15 });
for (const wpId of [0.2, 0.3, 0.5]) row(`w_p = ${wpId} при идентификаторе в запросе`, { ...START, wpId });
row('w_f = 0,15, w_p = 0,05', { ...START, wf: 0.15, wp: 0.05 });
row('w_c = 0,1, w_p = 0,05', { ...START, wc: 0.1, wp: 0.05 });

console.log('\n### Без rerank (опора c), θ = 0\n');
head();
row('RRF списков (прежний)', { rrfBase: true, rerank: false, theta: 0 });
row('старт c+0,1f+0,1p', START_NR);
row('p бинарный', { ...START_NR, pmode: 'bin' });
row('f^0,5', { ...START_NR, fpow: 0.5 });
row('f^2, w_f = 0,15', { ...START_NR, fpow: 2, wf: 0.15 });
for (const wpId of [0.2, 0.3]) row(`w_p = ${wpId} при идентификаторе`, { ...START_NR, wpId });

// группы, где rerank-старт теряет против RRF: полнота окна по группам
console.log('\n### Полнота окна rerank (K = 15) по группам, %: S0 / RRF / по очереди / K = 30 по S0\n\n| Группа | N | S0 | RRF | по очереди | S0, K = 30 |\n|---|---|---|---|---|---|');
for (const g of [...new Set(M.map((q) => q.meta.group))]) {
  const qs = M.filter((q) => q.meta.group === g);
  console.log(`| ${g} | ${qs.length} | ${[START, { ...START, sel: 'rrf' }, { ...START, sel: 'union' }, { ...START, K: 30 }].map((c) => (100 * windowRecall(qs, c)).toFixed(0)).join(' | ')} |`);
}
// идентификаторы: детектор запроса
console.log('\nЗапросы с идентификатором (детектор `_ . /` внутри слова): ' + M.filter((q) => q.meta.ident).map((q) => `${q.meta.id}(${q.meta.group})`).join(' '));
console.log('Группа identifier без идентификатора по детектору: ' + M.filter((q) => q.meta.group === 'identifier' && !q.meta.ident).map((q) => `${q.meta.id}: «${q.meta.query}»`).join('; '));
