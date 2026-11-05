// Диагностика: где стоит правильный кусок в порядке S0 (стартовые веса) и в порядке RRF; его rerank.
import path from 'node:path';
import { readRows, prepare } from './lib.mjs';
const [dumpDir, ...ids] = process.argv.slice(2);
for (const row of readRows(dumpDir)) {
  if (!ids.includes(row.id) || row.mode !== 'search') continue;
  const q = prepare(row);
  const idx = [...Array(q.n).keys()];
  const s0 = (i) => q.c[i] + 0.1 * q.f[i] + 0.1 * q.p[i];
  const bySum = idx.slice().sort((a, b) => s0(b) - s0(a));
  const rrf = (i) => (q.tr[i] != null ? 1 / (61 + q.tr[i]) : 0) + (q.vr[i] != null ? 1 / (61 + q.vr[i]) : 0);
  const byRrf = idx.slice().sort((a, b) => rrf(b) - rrf(a));
  const byR = idx.slice().sort((a, b) => q.r[b] - q.r[a]);
  const gold = idx.filter((i) => q.rel[i].length);
  console.log(row.id, 'пул', q.n, gold.map((i) => `кусок ${i}: место по S0 ${bySum.indexOf(i) + 1}, по RRF ${byRrf.indexOf(i) + 1}, по rerank ${byR.indexOf(i) + 1}; r=${q.r[i].toFixed(2)} c=${q.c[i].toFixed(2)} f=${q.f[i].toFixed(2)} p=${q.p[i].toFixed(2)} tr=${q.tr[i]} vr=${q.vr[i]}`).join(' | '));
}
