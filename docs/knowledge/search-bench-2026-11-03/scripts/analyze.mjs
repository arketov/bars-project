// analyze.mjs [папка-бенча] [выходной.json] — метрики вариантов поиска по группам и проектам.
// Варианты: cli (полный конвейер bars-cli), fts-or (как сейчас), fts-head5 (bm25(chunks_fts,5,1)), fts-and.
import fs from 'node:fs';
import { openProject, runFts } from './fts.mjs';

const BENCH = process.argv[2] || process.env.USERPROFILE + '/.bars/bench';
const OUT = process.argv[3] || BENCH + '/summary.json';
const PROJECTS = ['bars-project', 'overdrive-bots', 'RoleGPT', 'agaugu', 'tmc-opovechenie'];
const N = (x) => x.split(/\s+/).join(' ');
const GROUPS = {
  term: 'короткие термины', number: 'номера и даты', identifier: 'идентификаторы', question: 'вопросы обычным языком',
  bag: 'длинный набор слов', 'agent-bag': 'настоящие запросы агентов', typo: 'опечатки', morphology: 'словоформы',
  synonym: 'синонимы и перефразировка', mixed: 'смешанные ru/en', where: '«где описано»', twopart: 'ответ в двух кусках раздела',
  'guide-vs-journals': 'руководство среди журналов',
};

// gold → какие результаты считать правильными
function relevantIdx(gold, row) {
  const t = N(row.text);
  const hits = [];
  gold.forEach((g, i) => { if (g.path === row.path && t.includes(N(g.evidence))) hits.push(i); });
  return hits;
}

function score(q, rows) {
  const top = rows.slice(0, 10);
  const needAll = q.group === 'twopart';
  const seen = new Set();
  let firstRank = 0;
  const gains = [];
  top.forEach((r, i) => {
    const h = relevantIdx(q.gold, r);
    let gain = 0;
    for (const gi of h) {
      const key = needAll ? gi : 'any';
      if (!seen.has(key)) { seen.add(key); gain = 1; }
    }
    if (h.length && !firstRank) firstRank = i + 1;
    gains.push(gain);
  });
  const G = needAll ? q.gold.length : 1;
  let dcg = 0, idcg = 0;
  gains.forEach((g, i) => { dcg += g / Math.log2(i + 2); });
  for (let i = 0; i < Math.min(G, 10); i++) idcg += 1 / Math.log2(i + 2);
  const t3 = top.slice(0, 3);
  const keyS = (r) => r.path + '#' + r.heading_chain;
  const top5 = rows.slice(0, 5);
  return {
    rank: firstRank,
    first: firstRank === 1 ? 1 : 0,
    hit3: firstRank && firstRank <= 3 ? 1 : 0,
    mrr: firstRank ? 1 / firstRank : 0,
    ndcg: idcg ? dcg / idcg : 0,
    found: firstRank ? 1 : 0,
    bothInTop10: needAll ? (seen.size === q.gold.length ? 1 : 0) : null,
    chatsTop3: t3.length ? t3.filter((r) => (r.module || '') === 'chats').length / t3.length : 0,
    secDup5: new Set(top5.map(keyS)).size < top5.length ? 1 : 0,
    docDup5: new Set(top5.map((r) => r.path)).size < top5.length ? 1 : 0,
    n: rows.length,
  };
}

const avg = (a, k) => (a.length ? a.reduce((s, x) => s + (x[k] ?? 0), 0) / a.length : null);
function agg(items) {
  const s = items.map((i) => i.s);
  const two = s.filter((x) => x.bothInTop10 !== null);
  return {
    n: s.length, first: avg(s, 'first'), hit3: avg(s, 'hit3'), mrr10: avg(s, 'mrr'), ndcg10: avg(s, 'ndcg'), notFound10: 1 - avg(s, 'found'),
    chatsTop3: avg(s, 'chatsTop3'), secDup5: avg(s, 'secDup5'), docDup5: avg(s, 'docDup5'),
    ...(two.length ? { bothInTop10: avg(two, 'bothInTop10') } : {}),
  };
}

const variants = ['cli', 'fts-or', 'fts-head5', 'fts-and'];
const perQuery = [];
const warnings = [];
for (const proj of PROJECTS) {
  const qs = JSON.parse(fs.readFileSync(`${BENCH}/${proj}/queries.json`, 'utf8')).filter((q) => q.gold.length);
  const cliPath = `${BENCH}/${proj}/cli-full.json`;
  const cli = fs.existsSync(cliPath) ? Object.fromEntries(JSON.parse(fs.readFileSync(cliPath, 'utf8')).map((o) => [o.id, o])) : {};
  const p = openProject(proj);
  for (const q of qs) {
    const rec = { id: q.id, project: proj, group: q.group, query: q.query, golds: q.gold.length, variants: {} };
    for (const v of variants) {
      let rows;
      if (v === 'cli') {
        const o = cli[q.id];
        if (!o || o.error) { rec.variants[v] = null; continue; }
        if (o.rerank_error) warnings.push(`${q.id}: без rerank (${o.rerank_error})`);
        rows = o.results;
        rec.reranked = !o.rerank_error;
        rec.ms = o.ms;
      } else rows = runFts(p, q.query, v.slice(4), 30).map((r) => ({ path: r.path, text: r.t, heading_chain: r.h, module: r.m }));
      rec.variants[v] = score(q, rows);
    }
    perQuery.push(rec);
  }
}

const summary = { made: new Date().toISOString(), variants: {}, warnings };
for (const v of variants) {
  const items = perQuery.filter((r) => r.variants[v]).map((r) => ({ ...r, s: r.variants[v] }));
  if (!items.length) continue;
  const by = (keyFn) => { const m = {}; for (const it of items) (m[keyFn(it)] ||= []).push(it); return Object.fromEntries(Object.entries(m).map(([k, a]) => [k, agg(a)])); };
  summary.variants[v] = { overall: agg(items), byGroup: by((i) => i.group), byProject: by((i) => i.project) };
}
summary.groups = GROUPS;
summary.queries = perQuery.map((r) => ({ id: r.id, project: r.project, group: r.group, query: r.query, golds: r.golds, reranked: r.reranked ?? null, ms: r.ms ?? null, rank: Object.fromEntries(variants.map((v) => [v, r.variants[v] ? r.variants[v].rank : null])) }));
fs.writeFileSync(OUT, JSON.stringify(summary, null, 1));
const pct = (x) => (x == null ? '—' : (100 * x).toFixed(0) + '%');
for (const v of Object.keys(summary.variants)) {
  const o = summary.variants[v].overall;
  console.log(v.padEnd(10), 'n=' + o.n, 'первым', pct(o.first), 'Hit@3', pct(o.hit3), 'MRR', o.mrr10.toFixed(3), 'nDCG', o.ndcg10.toFixed(3), 'не найден', pct(o.notFound10), 'журналы в топ-3', pct(o.chatsTop3), 'повтор раздела', pct(o.secDup5), 'повтор документа', pct(o.docDup5));
}
if (warnings.length) console.log('предупреждения:', warnings.length);
