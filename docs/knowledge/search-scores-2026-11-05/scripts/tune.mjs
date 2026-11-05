// Шаг 2.3.7: подбор весов и порога схемы баллов по дампу `bench_quality dump` (без моделей).
//   node tune.mjs <каталог-дампа> <корень-репозитория> [режим]
// Режимы: base (сравнение схем), grid (сетка по подбору → проверка), theta (порог), links, ops, neg,
// groups, all (по умолчанию — всё подряд). Метрики — «первым / Hit@3 / MRR@10»; подбор по нечётным
// запросам (`split = tune`), проверка по чётным (`test`), как в правиле приёмки Т-12.
import fs from 'node:fs';
import path from 'node:path';
import { readRows, prepare, run, summarize, fmt, pct } from './lib.mjs';

const [dumpDir, repo, mode = 'all'] = process.argv.slice(2);
if (mode === 'all') {
  console.log('---\ntitle: Схема баллов поиска — выдача подбора весов и порога по дампу 05-11-2026 (шаг 2.3.7)\n---\n\n# Схема баллов поиска — выдача подбора весов и порога по дампу 05-11-2026\n\nМашинный вывод `scripts/tune.mjs` (режим `all`) по [дампу](Index.md); разбор и выводы — в [исследовании схемы баллов](../search-scores.md). Метрики: первым / Hit@3 / MRR@10, %, %, число.\n');
}
const strip = (s) => s.replace(/^﻿/, '');
const readJson = (p) => JSON.parse(strip(fs.readFileSync(p, 'utf8')));

// ---- метаданные запросов ------------------------------------------------------------------
const main = new Map(); // id → {project, group, split, golds}
for (const q of readJson(path.join(repo, 'docs/knowledge/search-bench-2026-11-03/queries.json'))) {
  main.set(q.id, { project: q.project, group: q.group, split: q.split, golds: q.golds });
}
const quality = path.join(repo, 'crates/bars-core/examples/quality');
const neg = new Map(); // id → {project, split}
const ops = new Map(); // id → {category, split}
for (const c of readJson(path.join(quality, 'no-answer-bars.json'))) neg.set(c.id, { project: 'bars-project', split: c.split });
for (const c of readJson(path.join(quality, 'overdrive.json'))) if (c.category === 'negative') neg.set(c.id, { project: 'overdrive-bots', split: c.split });
const phraseAbsent = new Map();
for (const c of readJson(path.join(quality, 'operators-bars.json'))) {
  if (c.category === 'negative') phraseAbsent.set(c.id, { split: c.split });
  else ops.set(c.id, { category: c.category, split: c.split });
}

// ---- дамп ---------------------------------------------------------------------------------
const rows = readRows(dumpDir);
const errors = rows.filter((r) => r.error);
const Q = { main: [], neg: [], ops: [], absent: [] };
const pairedOf = new Map();
for (const r of rows) {
  if (r.error) continue;
  const q = prepare(r);
  if (r.mode === 'paired') { pairedOf.set(r.id, q); continue; }
  if (main.has(r.id)) {
    const m = main.get(r.id);
    q.meta = { id: r.id, ...m, all: false };
    q.golds = m.golds;
    Q.main.push(q);
  } else if (neg.has(r.id)) { q.meta = { id: r.id, ...neg.get(r.id) }; Q.neg.push(q); }
  else if (phraseAbsent.has(r.id)) { q.meta = { id: r.id, ...phraseAbsent.get(r.id) }; Q.absent.push(q); }
  else if (ops.has(r.id)) { q.meta = { id: r.id, ...ops.get(r.id) }; Q.ops.push(q); }
}
for (const q of Q.ops) if (pairedOf.has(q.meta.id)) q.paired = pairedOf.get(q.meta.id);
const nCand = rows.filter((r) => !r.error).map((r) => r.pool.length);
console.log(`Дамп: ${rows.length} строк, ошибок ${errors.length}; main ${Q.main.length}, neg ${Q.neg.length}, ops ${Q.ops.length}, фраза-нет ${Q.absent.length}; пул ${Math.min(...nCand)}…${Math.max(...nCand)}`);
for (const e of errors) console.log(`Ошибка ${e.id} ${e.mode}: ${e.error}`);

// ---- метрики -------------------------------------------------------------------------------
function rankOf(q, sections) {
  const first = new Map();
  sections.forEach((sec, place) => {
    for (const i of sec.chunks) for (const g of q.rel[i]) if (!first.has(g)) first.set(g, place + 1);
  });
  if (!first.size) return Infinity;
  if (q.meta?.all) return first.size >= q.golds ? Math.max(...first.values()) : Infinity;
  return Math.min(...first.values());
}
const ranksOf = (queries, cfg) => queries.map((q) => rankOf(q, run(q, cfg)));
const metrics = (queries, cfg) => summarize(ranksOf(queries, cfg));
const bySplit = (queries, split) => queries.filter((q) => split === 'all' || q.meta.split === (split === 'tune' ? 'tune' : 'test'));
const nresults = (cfg) => (cfg.rerank ? 3 : 5);

function journals(queries, cfg) {
  let seats = 0, chats = 0;
  for (const q of queries) {
    for (const sec of run(q, cfg).slice(0, 3)) { seats++; if (q.docs[sec.d].m === 'chats') chats++; }
  }
  return seats ? chats / seats : 0;
}
function falseShare(queries, cfg) {
  if (!queries.length) return NaN;
  return queries.filter((q) => run(q, cfg).length > 0).length / queries.length;
}

const START = { rerank: true, K: 15, wc: 0, wf: 0.1, wp: 0.1, wl: 0, fold: 0, theta: 0.1 };
const START_NR = { rerank: false, K: 15, wc: 1, wf: 0.1, wp: 0.1, wl: 0, fold: 0, theta: 0.1 };
const RRF_RR = { rrf: true, rerank: true, K: 15, theta: 0.1, wl: 0, fold: 0 };
const RRF_NR = { rrf: true, rerank: false, K: 15, theta: 0, wl: 0, fold: 0 };
const cfgText = (c) => (c.rrf ? `RRF K=${c.K} θ=${c.theta}` : `wc=${c.wc} wf=${c.wf} wp=${c.wp} wl=${c.wl} fold=${c.fold} K=${c.K} θ=${c.theta}`);

function table(title, entries) {
  console.log(`\n### ${title}\n\n| Вариант | Все | Подбор (нечётные) | Проверка (чётные) |\n|---|---|---|---|`);
  for (const [name, cfg] of entries) {
    console.log(`| ${name} | ${fmt(metrics(Q.main, cfg))} | ${fmt(metrics(bySplit(Q.main, 'tune'), cfg))} | ${fmt(metrics(bySplit(Q.main, 'test'), cfg))} |`);
  }
}

// ---- 1. сравнение схем ----------------------------------------------------------------------
if (['all', 'base'].includes(mode)) {
  table('Режим с rerank (K = 15, порог 0,1): прежнее слияние, стартовые веса и ступени', [
    ['только rerank после RRF (прежний конвейер)', RRF_RR],
    ['стартовые веса: r + 0,1·f + 0,1·p', START],
    ['r без добавок', { ...START, wf: 0, wp: 0 }],
    ['r + 0,1·f', { ...START, wp: 0 }],
    ['r + 0,1·p', { ...START, wf: 0 }],
    ['r без порога (θ = 0)', { ...START, theta: 0 }],
    ['K = 30', { ...START, K: 30 }],
    ['K = 10', { ...START, K: 10 }],
  ]);
  table('Режим без rerank: RRF и баллы (опора — косинус c)', [
    ['только полнотекстовый список (порядок bm25)', { fts: true, rerank: false }],
    ['RRF списков FTS и векторов (прежний порядок)', RRF_NR],
    ['c без добавок', { ...START_NR, wf: 0, wp: 0, theta: 0 }],
    ['стартовые веса c + 0,1·f + 0,1·p', { ...START_NR, theta: 0 }],
    ['c + 0,3·f + 0,1·p', { ...START_NR, wf: 0.3, theta: 0 }],
    ['c + 0,5·f + 0,1·p', { ...START_NR, wf: 0.5, theta: 0 }],
    ['только полнотекстовый f + 0,1·p (без embed)', { ...START_NR, wc: 0, wf: 1, wp: 0.1, theta: 0, noc: true }],
  ]);
}

// ---- 2. сетка -------------------------------------------------------------------------------
const WF = [0, 0.05, 0.1, 0.15, 0.2, 0.3];
const WP = [0, 0.05, 0.1, 0.15, 0.2];
const WC = [0, 0.05, 0.1, 0.2, 0.3];
const WF_NR = [0, 0.05, 0.1, 0.2, 0.3, 0.5, 0.8, 1.2];
const WP_NR = [0, 0.05, 0.1, 0.2, 0.3, 0.5];
const FOLD = [0, 0.05, 0.1];
const KS = [10, 15, 20, 30];

function grid(base, axes) {
  const names = Object.keys(axes);
  const out = [];
  const go = (k, cur) => {
    if (k === names.length) { out.push(cur); return; }
    for (const v of axes[names[k]]) go(k + 1, { ...cur, [names[k]]: v });
  };
  go(0, base);
  return out;
}
const complexity = (c) => [c.wc ?? 0, c.wf, c.wp, c.wl, c.fold].filter((x) => x > 0).length * 10 + (c.wc ?? 0) + c.wf + c.wp + c.wl + c.fold;
function search(configs, label) {
  const tune = bySplit(Q.main, 'tune');
  const scored = configs.map((cfg) => ({ cfg, m: metrics(tune, cfg) }));
  scored.sort((a, b) => b.m.mrr - a.m.mrr || b.m.first - a.m.first);
  const best = scored[0].m.mrr;
  // среди равных по MRR подбора (в пределах 0,002) берётся самая простая схема
  const near = scored.filter((x) => best - x.m.mrr <= 0.002).sort((a, b) => complexity(a.cfg) - complexity(b.cfg));
  console.log(`\n#### ${label}: ${configs.length} схем; лучший MRR подбора ${best.toFixed(3)}; в пределах 0,002 — ${near.length}`);
  console.log('| Схема (по подбору) | Подбор | Проверка | Все |\n|---|---|---|---|');
  const show = [near[0], ...scored.slice(0, 5).filter((x) => x !== near[0])];
  for (const x of show) {
    console.log(`| ${cfgText(x.cfg)} | ${fmt(x.m)} | ${fmt(metrics(bySplit(Q.main, 'test'), x.cfg))} | ${fmt(metrics(Q.main, x.cfg))} |`);
  }
  return { chosen: near[0].cfg, scored };
}

let chosenRR = null, chosenNR = null, gridRR = null, gridNR = null;
if (['all', 'grid', 'theta', 'links', 'groups', 'ops', 'neg', 'sixteen'].includes(mode)) {
  console.log('\n## Сетки по нечётным запросам');
  const rr = search(grid({ ...START, theta: 0, wl: 0 }, { wc: WC, wf: WF, wp: WP, fold: FOLD, K: KS }), 'rerank: w_c × w_f × w_p × fold × K');
  gridRR = rr.chosen;
  const nr = search(grid({ ...START_NR, theta: 0, wl: 0 }, { wf: WF_NR, wp: WP_NR, fold: FOLD }), 'без rerank: w_f × w_p × fold (c — опора)');
  gridNR = nr.chosen;
  // Перекрёстная проверка (ниже) показала, что схемы сетки не лучше стартовых весов на второй половине:
  // дальнейшие разделы считаются на стартовых весах, которые и приняты.
  chosenRR = { ...START };
  chosenNR = { ...START_NR };
}


// ---- 2б. устойчивость выбора: перекрёстная проверка и чувствительность ----------------------------
function searchOn(configs, queries) {
  const scored = configs.map((cfg) => ({ cfg, m: metrics(queries, cfg) }));
  scored.sort((a, b) => b.m.mrr - a.m.mrr || b.m.first - a.m.first);
  const best = scored[0].m.mrr;
  return scored.filter((x) => best - x.m.mrr <= 0.002).sort((a, b) => complexity(a.cfg) - complexity(b.cfg))[0];
}
if (['all', 'cv'].includes(mode)) {
  console.log('\n## Перекрёстная проверка выбора весов (подбор на одной половине, проверка на другой)');
  const rrGrid = grid({ ...START, theta: 0, wl: 0 }, { wc: WC, wf: WF, wp: WP, fold: FOLD, K: [10, 15, 20, 30] });
  const rrGrid15 = grid({ ...START, theta: 0, wl: 0 }, { wc: WC, wf: WF, wp: WP, fold: FOLD });
  const nrGrid = grid({ ...START_NR, theta: 0, wl: 0 }, { wf: WF_NR, wp: WP_NR, fold: FOLD });
  for (const [label, cfgs, startCfg, rrfCfg] of [['rerank, K любой', rrGrid, START, RRF_RR], ['rerank, K = 15', rrGrid15, START, RRF_RR], ['без rerank', nrGrid, START_NR, RRF_NR]]) {
    console.log(`
#### ${label}

| Подбор на | Выбранная схема | На подборе | На другой половине | Старт на другой | RRF на другой |
|---|---|---|---|---|---|`);
    for (const [from, to] of [['tune', 'test'], ['test', 'tune']]) {
      const a = bySplit(Q.main, from), b = bySplit(Q.main, to);
      const pick = searchOn(cfgs, a);
      console.log(`| ${from === 'tune' ? 'нечётных' : 'чётных'} | ${cfgText(pick.cfg)} | ${fmt(pick.m)} | ${fmt(metrics(b, pick.cfg))} | ${fmt(metrics(b, { ...startCfg, theta: 0 }))} | ${fmt(metrics(b, { ...rrfCfg, theta: rrfCfg.rrf && rrfCfg.rerank ? 0 : 0 }))} |`);
    }
  }
}
if (['all', 'marg'].includes(mode)) {
  console.log('\n## Чувствительность: один вес меняется, остальные — стартовые (θ = 0), все 168 запросов; MRR / первым / Hit@3');
  const axes = { wf: [0, 0.05, 0.1, 0.15, 0.2, 0.3], wp: [0, 0.05, 0.1, 0.15, 0.2, 0.3], wc: [0, 0.1, 0.2, 0.3, 0.5], fold: [0, 0.05, 0.1, 0.2], K: [5, 10, 15, 20, 30, 40] };
  for (const [label, base] of [['rerank', { ...START, theta: 0 }], ['без rerank', { ...START_NR, theta: 0 }]]) {
    console.log(`
#### ${label}

| Параметр | значения → MRR (первым / Hit@3) |
|---|---|`);
    for (const [name, values] of Object.entries(axes)) {
      if (!base.rerank && name === 'wc') continue;
      if (!base.rerank && name === 'K') continue;
      console.log(`| ${name} | ${values.map((v) => { const m = metrics(Q.main, { ...base, [name]: v }); return `${v}: ${m.mrr.toFixed(3)} (${pct(m.first)}/${pct(m.hit3)})`; }).join('; ')} |`);
    }
  }
  const both = Q.main.filter((q) => q.meta.group === 'twopart');
  const bothTop10 = (cfg) => both.filter((q) => { const secs = run(q, cfg).slice(0, 10); const got = new Set(); secs.forEach((s) => s.chunks.forEach((i) => q.rel[i].forEach((g) => got.add(g)))); return got.size >= q.golds; }).length;
  console.log(`
Группа «два куска» (оба эталона в первых десяти разделах): RRF+rerank ${bothTop10(RRF_RR)}/${both.length}, старт+rerank ${bothTop10(START)}/${both.length}, старт без rerank ${bothTop10(START_NR)}/${both.length}`);
}

// ---- 3. связи ---------------------------------------------------------------------------------
if (['all', 'links'].includes(mode)) {
  console.log('\n## Сигнал связей l');
  for (const [label, base] of [['rerank', chosenRR], ['без rerank', chosenNR]]) {
    console.log(`\n#### ${label}: w_l при подобранных остальных весах\n\n| w_l | Все | Подбор | Проверка | Журналы в топ-3, % | Выдач с l = 1 в пуле, % | Порядок первых трёх изменён, % |\n|---|---|---|---|---|---|---|`);
    const noL = { ...base, wl: 0 };
    for (const wl of [0, 0.03, 0.05, 0.1, 0.2]) {
      const cfg = { ...base, wl };
      let withL = 0, changed = 0;
      for (const q of Q.main) {
        const a = run(q, cfg), b = run(q, noL);
        const top = (s) => s.slice(0, 3).map((x) => x.s).join(',');
        if (top(a) !== top(b)) changed++;
        // l = 1 есть, если у какого-то раздела выдачи документ связан с якорем: проверяем по пулу
        if (wl > 0) {
          const anchors = new Set(b.slice(0, 3).map((x) => x.d));
          if (b.some((x) => [...anchors].some((a2) => a2 !== x.d && q.linked(a2, x.d)))) withL++;
        }
      }
      console.log(`| ${wl} | ${fmt(metrics(Q.main, cfg))} | ${fmt(metrics(bySplit(Q.main, 'tune'), cfg))} | ${fmt(metrics(bySplit(Q.main, 'test'), cfg))} | ${(100 * journals(Q.main, cfg)).toFixed(1)} | ${wl > 0 ? pct(withL / Q.main.length) : '—'} | ${wl > 0 ? pct(changed / Q.main.length) : '—'} |`);
    }
  }
  console.log('\nПо проектам (w_l = 0 → 0,05; rerank):\n\n| Проект | N | w_l = 0 | w_l = 0,05 |\n|---|---|---|---|');
  for (const proj of [...new Set(Q.main.map((q) => q.meta.project))]) {
    const qs = Q.main.filter((q) => q.meta.project === proj);
    console.log(`| ${proj} | ${qs.length} | ${fmt(metrics(qs, { ...chosenRR, wl: 0 }))} | ${fmt(metrics(qs, { ...chosenRR, wl: 0.05 }))} |`);
  }
}

// ---- 4. порог и запросы без ответа ---------------------------------------------------------
const negAll = () => [...Q.neg, ...Q.absent.filter(() => false)];
if (['all', 'theta', 'neg'].includes(mode)) {
  console.log('\n## Порог θ и запросы без ответа');
  for (const [label, base, thetas] of [
    ['rerank', chosenRR, [0, 0.05, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9]],
    ['без rerank (косинус)', chosenNR, [0, 0.1, 0.2, 0.3, 0.35, 0.4, 0.45, 0.5, 0.55, 0.6, 0.65]],
  ]) {
    console.log(`\n#### ${label} (веса: ${cfgText({ ...base, theta: '·' })})\n\n| θ | Без ответа: ложные, все | подбор | проверка | Основной набор: потеряно запросов в топ-3 | Hit@3 | MRR | первым |\n|---|---|---|---|---|---|---|---|`);
    const ref = ranksOf(Q.main, { ...base, theta: 0 });
    for (const theta of thetas) {
      const cfg = { ...base, theta };
      const ranks = ranksOf(Q.main, cfg);
      const lost = ranks.filter((r, i) => r > 3 && ref[i] <= 3).length;
      const m = summarize(ranks);
      console.log(`| ${theta} | ${pct(falseShare(Q.neg, cfg))} % (${Q.neg.length}) | ${pct(falseShare(bySplit(Q.neg, 'tune'), cfg))} | ${pct(falseShare(bySplit(Q.neg, 'test'), cfg))} | ${lost} | ${pct(m.hit3)} | ${m.mrr.toFixed(3)} | ${pct(m.first)} |`);
    }
    const byProject = {};
    for (const q of Q.neg) (byProject[q.meta.project] ??= []).push(q);
    console.log(`\nЛожные ответы по проектам при θ ∈ {0,1; 0,2; 0,3; 0,5}: ` + Object.entries(byProject).map(([p, qs]) => `${p} (${qs.length}): ` + [0.1, 0.2, 0.3, 0.5].map((t) => pct(falseShare(qs, { ...base, theta: t }))).join(' / ')).join('; '));
  }
}

// ---- 5. операторы ---------------------------------------------------------------------------
if (['all', 'ops'].includes(mode)) {
  console.log('\n## Операторы запроса');
  for (const [label, base] of [['rerank', { ...chosenRR, theta: 0 }], ['без rerank', { ...chosenNR, theta: 0 }]]) {
    let results = 0, ok = 0, bad = [];
    let pairedLost = [], opHit = 0, pairedHit = 0, withPair = 0;
    const byKind = {};
    for (const q of Q.ops) {
      const sections = run(q, base);
      const top = sections.slice(0, nresults(base));
      for (const sec of top) { results++; if (q.ok[sec.best] === false) { bad.push(q.meta.id); } else ok++; }
      const r = rankOf({ ...q, meta: { ...q.meta, all: false }, golds: 1 }, sections);
      const kind = (byKind[q.meta.category] ??= { n: 0, first: 0, hit3: 0, mrr: 0, pHit3: 0, pMrr: 0 });
      kind.n++; kind.first += r === 1; kind.hit3 += r <= 3; kind.mrr += r <= 10 ? 1 / r : 0;
      if (q.paired) {
        withPair++;
        const pr = rankOf({ ...q.paired, meta: { all: false }, golds: 1 }, run(q.paired, base));
        if (r <= 3) opHit++;
        if (pr <= 3) pairedHit++;
        if (pr <= 3 && r > 3) pairedLost.push(q.meta.id);
        kind.pHit3 += pr <= 3; kind.pMrr += pr <= 10 ? 1 / pr : 0;
      }
    }
    console.log(`\n#### ${label}: точность операторов ${ok}/${results} результатов (${pct(ok / Math.max(1, results))} %); нарушения: ${[...new Set(bad)].join(', ') || 'нет'}`);
    console.log(`Hit@3 с оператором ${opHit}/${withPair}, у парного запроса без оператора ${pairedHit}/${withPair}; потеряно против парного: ${pairedLost.join(', ') || 'нет'}\n`);
    console.log('| Оператор | N | первым % | Hit@3 % | MRR | парный Hit@3 % | парный MRR |\n|---|---|---|---|---|---|---|');
    for (const [k, v] of Object.entries(byKind)) console.log(`| ${k} | ${v.n} | ${pct(v.first / v.n)} | ${pct(v.hit3 / v.n)} | ${(v.mrr / v.n).toFixed(3)} | ${pct(v.pHit3 / v.n)} | ${(v.pMrr / v.n).toFixed(3)} |`);
    const emptyAbsent = Q.absent.filter((q) => run(q, base).length === 0).length;
    console.log(`\nФраза, которой нет: пустая выдача у ${emptyAbsent} из ${Q.absent.length} запросов.`);
  }
}

// ---- 6. группы и проекты ---------------------------------------------------------------------
if (['all', 'groups'].includes(mode)) {
  console.log('\n## По группам и проектам: стартовые веса и подобранные против прежнего слияния');
  const variants = [['RRF (rerank)', RRF_RR], ['старт', START], ['лучшая схема сетки по подбору', { ...gridRR }]];
  for (const key of ['group', 'project']) {
    console.log(`\n| ${key === 'group' ? 'Группа' : 'Проект'} | N | ${variants.map(([n]) => n).join(' | ')} |\n|---|---|${variants.map(() => '---').join('|')}|`);
    for (const g of [...new Set(Q.main.map((q) => q.meta[key]))]) {
      const qs = Q.main.filter((q) => q.meta[key] === g);
      console.log(`| ${g} | ${qs.length} | ${variants.map(([, c]) => fmt(metrics(qs, c))).join(' | ')} |`);
    }
  }
  for (const [label, a, b] of [['схема сетки против старта', START, gridRR], ['схема сетки против RRF', RRF_RR, gridRR], ['старт против RRF', RRF_RR, START]]) {
    const ra = ranksOf(Q.main, a), rb = ranksOf(Q.main, b);
    const lost = Q.main.filter((q, i) => rb[i] > 3 && ra[i] <= 3).map((q) => q.meta.id);
    const won = Q.main.filter((q, i) => ra[i] > 3 && rb[i] <= 3).map((q) => q.meta.id);
    const worse1 = Q.main.filter((q, i) => rb[i] > 1 && ra[i] === 1).map((q) => q.meta.id);
    const better1 = Q.main.filter((q, i) => ra[i] > 1 && rb[i] === 1).map((q) => q.meta.id);
    console.log(`\n${label}: Hit@3 потеряно ${lost.length} (${lost.join(' ')}), выиграно ${won.length} (${won.join(' ')}); первым потеряно ${worse1.length}, выиграно ${better1.length}`);
  }
}

if (['all', 'sixteen'].includes(mode)) {
  const ids = 'bp-02 bp-07 bp-36 bp-37 bp-38 ob-10 ob-16 ob-20 ob-24 ob-35 rg-05 rg-07 rg-09 rg-23 tm-09 tm-13'.split(' ');
  const qs = ids.map((id) => Q.main.find((q) => q.meta.id === id)).filter(Boolean);
  const cols = [['FTS', { fts: true }], ['RRF без rerank', RRF_NR], ['баллы без rerank', { ...chosenNR, theta: 0 }], ['RRF + rerank', RRF_RR], ['старт + rerank', START], ['подобранные + rerank', chosenRR]];
  console.log('\n## 16 запросов, ухудшившихся при смене полей FTS (tokenization.md): место первого правильного раздела\n');
  console.log('| Запрос | ' + cols.map(([n]) => n).join(' | ') + ' |\n|---|' + cols.map(() => '---').join('|') + '|');
  for (const q of qs) console.log('| ' + q.meta.id + ' | ' + cols.map(([, c]) => { const r = rankOf(q, run(q, c)); return r === Infinity ? '—' : r; }).join(' | ') + ' |');
  console.log('\nПервым / Hit@3 / MRR на этих 16: ' + cols.map(([n, c]) => n + ' ' + fmt(metrics(qs, c))).join('; '));
}

if (['all', 'theta', 'grid'].includes(mode)) {
  console.log(`\nСетка по подбору: rerank ${JSON.stringify(gridRR)}; без rerank ${JSON.stringify(gridNR)}; приняты стартовые веса`);
}
