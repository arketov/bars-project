// Независимая проверка весов схемы баллов bars (шаг 2.3.7) по дампу. Загрузка, свой конвейер, метрики, статистика.
import fs from 'node:fs';
import path from 'node:path';
import { readRows, prepare } from './lib.mjs';

export const REPO = 'C:/Users/Shket/OneDrive/Рабочий стол/SELF Repository/bars-project';
export const DUMP = path.join(REPO, 'docs/knowledge/search-scores-2026-11-05/dump');
const strip = (s) => s.replace(/^\uFEFF/, '');
const readJson = (p) => JSON.parse(strip(fs.readFileSync(p, 'utf8')));

// Идентификатор в запросе — как в hybrid-search.md: слово с `_`, `.` или `/` внутри.
export const hasIdent = (text) => /[\p{L}\p{N}][_./][\p{L}\p{N}]/u.test(text ?? '');

export function load() {
  const main = new Map();
  for (const q of readJson(path.join(REPO, 'docs/knowledge/search-bench-2026-11-03/queries.json'))) main.set(q.id, q);
  const quality = path.join(REPO, 'crates/bars-core/examples/quality');
  const neg = new Map();
  for (const c of readJson(path.join(quality, 'no-answer-bars.json'))) neg.set(c.id, { project: 'bars-project', split: c.split, query: c.query });
  for (const c of readJson(path.join(quality, 'overdrive.json'))) if (c.category === 'negative') neg.set(c.id, { project: 'overdrive-bots', split: c.split, query: c.query });
  const Q = { main: [], neg: [] };
  for (const r of readRows(DUMP)) {
    if (r.error || r.mode === 'paired') continue;
    const q = prepare(r);
    if (main.has(r.id)) { const m = main.get(r.id); q.meta = { id: r.id, project: m.project, group: m.group, split: m.split, query: m.query, ident: hasIdent(m.query) }; Q.main.push(q); }
    else if (neg.has(r.id)) { const m = neg.get(r.id); q.meta = { id: r.id, ...m, ident: hasIdent(m.query) }; Q.neg.push(q); }
  }
  // RRF-балл и место по RRF в пуле — для вариантов выбора кусков в rerank.
  for (const q of [...Q.main, ...Q.neg]) {
    q.rrf = new Float64Array(q.n);
    for (let i = 0; i < q.n; i++) q.rrf[i] = (q.tr[i] != null ? 1 / (61 + q.tr[i]) : 0) + (q.vr[i] != null ? 1 / (61 + q.vr[i]) : 0);
    q.rrfOrder = [...Array(q.n).keys()].filter((i) => q.tr[i] != null || q.vr[i] != null).sort((a, b) => q.rrf[b] - q.rrf[a] || q.id[a] - q.id[b]);
    q.isRel = Array.from({ length: q.n }, (_, i) => (q.rel[i]?.length ?? 0) > 0);
  }
  return Q;
}

// cfg: rerank, K, wc, wf, wp, fold, theta;
// выбор кусков в rerank: sel = 's0' | 'rrf' | 'union' (по очереди из S0 и RRF до K) | 'mix' (kS0 из S0, остальное из RRF);
// S0 с собственными весами: wc0 (по умолчанию 1), wf0, wp0 (по умолчанию = итоговым);
// pmode: 'raw' | 'bin' (p = 1 только при полном совпадении); fpow: f^γ; wpId: w_p для запросов с идентификатором.
// rrfBase: прежний конвейер (RRF → первые K → rerank → порядок по r).
export function run(q, cfg) {
  const n = q.n;
  const wpEff = cfg.wpId != null && q.meta.ident ? cfg.wpId : cfg.wp;
  const fv = (i) => (cfg.fpow && cfg.fpow !== 1 ? Math.pow(q.f[i], cfg.fpow) : q.f[i]);
  const pv = (i) => (cfg.pmode === 'bin' ? (q.p[i] >= 0.999 ? 1 : 0) : q.p[i]);
  let ranked = [];
  if (cfg.rrfBase) {
    let order = q.rrfOrder;
    if (cfg.rerank) {
      const top = order.slice(0, cfg.K).filter((i) => q.r[i] >= 0 && q.r[i] >= cfg.theta);
      ranked = top.map((i) => ({ i, score: q.r[i] }));
    } else ranked = order.map((i) => ({ i, score: q.rrf[i] }));
  } else {
    const wc0 = cfg.wc0 ?? 1, wf0 = cfg.wf0 ?? cfg.wf, wp0 = cfg.wp0 ?? wpEff;
    const s0 = new Float64Array(n);
    for (let i = 0; i < n; i++) s0[i] = wc0 * q.c[i] + wf0 * fv(i) + wp0 * pv(i);
    const order = [...Array(n).keys()].sort((a, b) => s0[b] - s0[a] || q.f[b] - q.f[a] || q.id[a] - q.id[b]);
    let take = order;
    if (cfg.rerank) {
      const K = cfg.K, sel = cfg.sel ?? 's0';
      if (sel === 's0') take = order.slice(0, K);
      else if (sel === 'rrf') take = q.rrfOrder.slice(0, K);
      else {
        const chosen = new Set();
        if (sel === 'union') {
          let a = 0, b = 0;
          while (chosen.size < K && (a < order.length || b < q.rrfOrder.length)) {
            if (a < order.length) chosen.add(order[a++]);
            if (chosen.size < K && b < q.rrfOrder.length) chosen.add(q.rrfOrder[b++]);
          }
        } else if (sel === 'mix') {
          for (const i of order.slice(0, cfg.kS0)) chosen.add(i);
          for (const i of q.rrfOrder) { if (chosen.size >= K) break; chosen.add(i); }
          for (const i of order) { if (chosen.size >= K) break; chosen.add(i); }
        }
        take = [...chosen];
      }
    }
    for (const i of take) {
      let anchor, score;
      if (cfg.rerank) {
        anchor = q.r[i];
        if (anchor < 0) continue;
        score = anchor + cfg.wc * q.c[i] + cfg.wf * fv(i) + wpEff * pv(i);
      } else {
        anchor = q.c[i];
        score = anchor + cfg.wf * fv(i) + wpEff * pv(i);
      }
      if (anchor < cfg.theta) continue;
      ranked.push({ i, score });
    }
  }
  ranked.sort((a, b) => b.score - a.score || q.f[b.i] - q.f[a.i] || q.line[a.i] - q.line[b.i] || q.id[a.i] - q.id[b.i]);
  const sections = new Map();
  for (const x of ranked) {
    const sec = sections.get(q.s[x.i]);
    if (sec) sec.chunks.push(x.i);
    else sections.set(q.s[x.i], { s: q.s[x.i], d: q.d[x.i], score: x.score, chunks: [x.i], pos: sections.size });
  }
  const out = [...sections.values()];
  if (cfg.fold) {
    for (const sec of out) sec.score += cfg.fold * (sec.chunks.length - 1);
    out.sort((a, b) => b.score - a.score || a.pos - b.pos);
  }
  return out;
}

export function rankOf(q, sections) {
  for (let k = 0; k < sections.length; k++) for (const i of sections[k].chunks) if (q.isRel[i]) return k + 1;
  return Infinity;
}
export const ranks = (qs, cfg) => qs.map((q) => rankOf(q, run(q, cfg)));
export const rr = (r) => (r <= 10 ? 1 / r : 0);
export function summ(rs) {
  const n = rs.length || 1;
  return { n: rs.length, first: rs.filter((r) => r === 1).length / n, hit3: rs.filter((r) => r <= 3).length / n, mrr: rs.reduce((a, r) => a + rr(r), 0) / n };
}
export const fmt = (m) => `${(100 * m.first).toFixed(0)} / ${(100 * m.hit3).toFixed(0)} / ${m.mrr.toFixed(3)}`;
export const falseShare = (qs, cfg) => qs.filter((q) => run(q, cfg).length > 0).length / qs.length;

// --- статистика -------------------------------------------------------------------------------
// Точный двусторонний тест знаков: wins против losses.
export function signTest(w, l) {
  const n = w + l; if (!n) return 1;
  const k = Math.min(w, l);
  let c = 1, s = 0; // C(n,0)
  for (let i = 0; i <= k; i++) { s += c; c = (c * (n - i)) / (i + 1); }
  return Math.min(1, (2 * s) / 2 ** n);
}
// Парный бутстрап по запросам: 95 % интервал разности средних (b − a) и P(разность ≤ 0).
export function bootstrap(a, b, B = 10000, seed = 7) {
  let x = seed >>> 0;
  const rnd = () => { x ^= x << 13; x >>>= 0; x ^= x >> 17; x ^= x << 5; x >>>= 0; return x / 4294967296; };
  const n = a.length, d = a.map((v, i) => b[i] - v);
  const mean = d.reduce((s, v) => s + v, 0) / n;
  const res = new Float64Array(B);
  for (let t = 0; t < B; t++) { let s = 0; for (let j = 0; j < n; j++) s += d[(rnd() * n) | 0]; res[t] = s / n; }
  res.sort();
  const le0 = res.filter((v) => v <= 0).length / B;
  return { mean, lo: res[Math.floor(0.025 * B)], hi: res[Math.floor(0.975 * B)], pLe0: le0 };
}
export function compare(qs, ra, rb) {
  // b против a: выигрыши/потери по Hit@3, «первым», RR
  const ids = (f) => qs.filter((q, i) => f(ra[i], rb[i])).map((q) => q.meta.id);
  const lost3 = ids((a, b) => a <= 3 && b > 3), won3 = ids((a, b) => a > 3 && b <= 3);
  const lost1 = ids((a, b) => a === 1 && b !== 1), won1 = ids((a, b) => a !== 1 && b === 1);
  const rwin = ids((a, b) => rr(b) > rr(a)), rloss = ids((a, b) => rr(b) < rr(a));
  const bs = bootstrap(ra.map(rr), rb.map(rr));
  return { lost3, won3, lost1, won1, rwin, rloss, pRR: signTest(rwin.length, rloss.length), p3: signTest(won3.length, lost3.length), bs };
}
