// Шаг 2.3.7: воспроизведение конвейера поиска по дампу `bench_quality dump` и метрики.
// Дамп — строка на запрос: пул кандидатов с сырыми сигналами c, f, p, r (rerank оценил весь пул),
// местом в списках FTS (tr) и векторов (vr), соответствием эталону (rel, dr), результатом
// операторов (ok), документом (d), разделом (s) и связями документов пула (links).
// Конвейер повторяет `crates/bars-core/src/search/hybrid.rs`: S0 → первые K в rerank → S = a + w_c·c
// + w_f·f + w_p·p → порог по опоре a → сигнал l по трём лучшим разделам → свёртка по разделам.
import fs from 'node:fs';
import path from 'node:path';

export function readRows(dir) {
  const rows = [];
  for (const name of fs.readdirSync(dir).filter((n) => n.endsWith('.jsonl')).sort()) {
    for (const line of fs.readFileSync(path.join(dir, name), 'utf8').split('\n')) {
      if (line.trim()) rows.push(JSON.parse(line));
    }
  }
  return rows;
}

// Столбцы пула → typed arrays-объекты: быстрее перебирать в сетке.
export function prepare(row) {
  const n = row.pool.length;
  const c = new Float64Array(n), f = new Float64Array(n), p = new Float64Array(n), r = new Float64Array(n);
  const d = new Int32Array(n), s = new Int32Array(n), id = new Float64Array(n);
  const rel = new Array(n), ok = new Array(n), line = new Array(n);
  for (let i = 0; i < n; i++) {
    const x = row.pool[i];
    c[i] = x.c ?? 0; f[i] = x.f; p[i] = x.p; r[i] = x.r ?? -1;
    d[i] = x.d; s[i] = x.s; id[i] = x.i;
    rel[i] = x.rel; ok[i] = x.ok; line[i] = x.ln[0];
  }
  const adj = new Set(row.links.map(([a, b]) => a * 100000 + b));
  const linked = (a, b) => adj.has(Math.min(a, b) * 100000 + Math.max(a, b));
  return { row, n, c, f, p, r, d, s, id, rel, ok, line, tr: row.pool.map((x) => x.tr), vr: row.pool.map((x) => x.vr), linked, docs: row.docs };
}

// cfg: {rerank, K, wc, wf, wp, wl, fold, theta, rrf}
// Возвращает разделы по убыванию оценки: {s, d, score, chunks:[индексы кусков]}.
export function run(q, cfg) {
  const { n } = q;
  let order = [];
  if (cfg.fts) {
    // Только полнотекстовый список (порядок составного bm25) — как «только FTS» в замерах токенизации.
    order = [...Array(n).keys()].filter((i) => q.tr[i] != null).sort((a, b) => q.tr[a] - q.tr[b]);
    return finish(q, { ...cfg, wl: 0, fold: 0 }, order.map((i) => ({ i, score: -q.tr[i] })), false);
  }
  if (cfg.rrf) {
    // Прежнее слияние: RRF списков FTS и векторов; идентификаторы без места в списках в пуле не нужны.
    const sc = new Float64Array(n);
    for (let i = 0; i < n; i++) sc[i] = (q.tr[i] != null ? 1 / (60 + q.tr[i] + 1) : 0) + (q.vr[i] != null ? 1 / (60 + q.vr[i] + 1) : 0);
    order = [...Array(n).keys()].filter((i) => q.tr[i] != null || q.vr[i] != null);
    order.sort((a, b) => sc[b] - sc[a] || q.id[a] - q.id[b]);
    const score = (i) => sc[i];
    return finish(q, cfg, order.map((i) => ({ i, score: score(i) })), false);
  }
  const s0 = new Float64Array(n);
  const cv = (i) => (cfg.noc ? 0 : q.c[i]);
  for (let i = 0; i < n; i++) s0[i] = cv(i) + cfg.wf * q.f[i] + cfg.wp * q.p[i];
  order = [...Array(n).keys()];
  order.sort((a, b) => s0[b] - s0[a] || q.f[b] - q.f[a] || q.id[a] - q.id[b]);
  const ranked = [];
  const take = cfg.rerank ? order.slice(0, cfg.K) : order;
  for (const i of take) {
    let anchor, score;
    if (cfg.rerank) {
      anchor = q.r[i];
      if (anchor < 0) continue;
      score = anchor + cfg.wc * cv(i) + cfg.wf * q.f[i] + cfg.wp * q.p[i];
    } else {
      anchor = cfg.noc ? 1 : q.c[i];
      score = (cfg.noc ? 0 : anchor) + cfg.wf * q.f[i] + cfg.wp * q.p[i];
    }
    if (anchor < cfg.theta) continue;
    ranked.push({ i, score });
  }
  return finish(q, cfg, ranked, true);
}

function finish(q, cfg, ranked, scoreOrder) {
  if (cfg.rrf && cfg.rerank) {
    // Прежний конвейер: первые K по RRF → rerank → порядок по rerank, порог по rerank.
    const top = ranked.slice(0, cfg.K).filter((x) => q.r[x.i] >= 0);
    ranked = top.map((x) => ({ i: x.i, score: q.r[x.i] })).filter((x) => x.score >= cfg.theta);
    ranked.sort((a, b) => b.score - a.score || q.id[a.i] - q.id[b.i]);
  }
  const cmp = (a, b) => b.score - a.score || q.f[b.i] - q.f[a.i] || q.line[a.i] - q.line[b.i] || q.id[a.i] - q.id[b.i];
  if (cfg.wl > 0 && ranked.length) {
    ranked = ranked.slice().sort(cmp);
    const anchors = [];
    const seen = new Set();
    for (const x of ranked) {
      if (seen.size === 3) break;
      if (!seen.has(q.s[x.i])) { seen.add(q.s[x.i]); if (!anchors.includes(q.d[x.i])) anchors.push(q.d[x.i]); }
    }
    ranked = ranked.map((x) => {
      const doc = q.d[x.i];
      const l = anchors.some((a) => a !== doc && q.linked(a, doc)) ? 1 : 0;
      return { i: x.i, score: x.score + cfg.wl * l, l };
    });
  }
  ranked = ranked.slice().sort(cmp);
  const sections = new Map();
  for (const x of ranked) {
    const sec = sections.get(q.s[x.i]);
    if (sec) sec.chunks.push(x.i);
    else sections.set(q.s[x.i], { s: q.s[x.i], d: q.d[x.i], best: x.i, score: x.score, chunks: [x.i] });
  }
  const out = [...sections.values()];
  const bonus = cfg.fold ?? 0;
  if (bonus) {
    for (const sec of out) sec.score += bonus * (sec.chunks.length - 1);
    out.sort((a, b) => b.score - a.score);
  }
  return out;
}

export function summarize(ranks) {
  const n = ranks.length || 1;
  const first = ranks.filter((r) => r === 1).length / n;
  const hit3 = ranks.filter((r) => r <= 3).length / n;
  const mrr = ranks.reduce((a, r) => a + (r <= 10 ? 1 / r : 0), 0) / n;
  return { n: ranks.length, first, hit3, mrr };
}
export const pct = (x) => (100 * x).toFixed(0);
export const fmt = (m) => `${pct(m.first)} / ${pct(m.hit3)} / ${m.mrr.toFixed(3)}`;
