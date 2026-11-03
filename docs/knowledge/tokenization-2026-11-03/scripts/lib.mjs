// lib.mjs — общие части опытов токенизации: загрузка копий индексов, эталон, метрики (те же, что в бенче 03-11-2026).
// Пакеты morphology/snowball ставятся ТОЛЬКО во временную папку: переменная TOK_NPM указывает на неё.
import { createRequire } from 'node:module';
import { DatabaseSync } from 'node:sqlite';
import fs from 'node:fs';

export const req = createRequire((process.env.TOK_NPM || '.') + '/');
export const HOME = process.env.USERPROFILE + '/.bars/bench/';
export const PROJECTS = ['bars-project', 'overdrive-bots', 'RoleGPT', 'agaugu', 'tmc-opovechenie'];
export const GROUPS = {
  term: 'короткие термины', number: 'номера и даты', identifier: 'идентификаторы', question: 'вопросы обычным языком',
  bag: 'длинный набор слов', 'agent-bag': 'настоящие запросы агентов', typo: 'опечатки', morphology: 'словоформы',
  synonym: 'синонимы и перефразировка', mixed: 'смешанные ru/en', where: '«где описано»', twopart: 'ответ в двух кусках раздела',
  'guide-vs-journals': 'руководство среди журналов',
};
const N = (x) => x.split(/\s+/).join(' ');

export function loadProject(proj) {
  const d = new DatabaseSync(HOME + proj + '/index.sqlite', { readOnly: true });
  const chunks = d.prepare('select c.id id, d.path path, d.title title, d.module module, c.heading_chain h, c.text t from chunks c join documents d on d.id=c.document_id order by c.id').all();
  d.close();
  const queries = JSON.parse(fs.readFileSync(HOME + proj + '/queries.json', 'utf8')).filter((q) => q.gold.length).map((q) => ({ ...q, split: parseInt(q.id.match(/\d+$/)[0], 10) % 2 ? 'tune' : 'test' }));
  return { proj, chunks, queries };
}

function relevant(gold, row) {
  const t = N(row.text);
  const hits = [];
  gold.forEach((g, i) => { if (g.path === row.path && t.includes(N(g.evidence))) hits.push(i); });
  return hits;
}
export function score(q, rows) {
  const top = rows.slice(0, 10);
  const needAll = q.group === 'twopart';
  const seen = new Set();
  let firstRank = 0;
  const gains = [];
  top.forEach((r, i) => {
    const h = relevant(q.gold, r);
    let gain = 0;
    for (const gi of h) { const key = needAll ? gi : 'any'; if (!seen.has(key)) { seen.add(key); gain = 1; } }
    if (h.length && !firstRank) firstRank = i + 1;
    gains.push(gain);
  });
  const G = needAll ? q.gold.length : 1;
  let dcg = 0, idcg = 0;
  gains.forEach((g, i) => { dcg += g / Math.log2(i + 2); });
  for (let i = 0; i < Math.min(G, 10); i++) idcg += 1 / Math.log2(i + 2);
  return { rank: firstRank, first: firstRank === 1 ? 1 : 0, hit3: firstRank && firstRank <= 3 ? 1 : 0, mrr: firstRank ? 1 / firstRank : 0, ndcg: idcg ? dcg / idcg : 0, found: firstRank ? 1 : 0 };
}
const avg = (a, k) => (a.length ? a.reduce((s, x) => s + x[k], 0) / a.length : 0);
export const agg = (s) => ({ n: s.length, first: avg(s, 'first'), hit3: avg(s, 'hit3'), mrr: avg(s, 'mrr'), ndcg: avg(s, 'ndcg'), found: avg(s, 'found') });

// Запуск варианта: variant = { name, build(chunks) → ctx, search(ctx, query, limit) → rows[{path,text,h,module}] }
export function evaluate(projData, variant, ctxCache = {}) {
  const out = [];
  for (const p of projData) {
    const ctx = (ctxCache[p.proj + '|' + variant.name] ||= variant.build(p.chunks, p));
    for (const q of p.queries) {
      let rows = [];
      try { rows = variant.search(ctx, q.query, 30, q); } catch (e) { rows = []; if (process.env.TOK_DEBUG) console.error(variant.name, q.id, e.message); }
      out.push({ id: q.id, project: p.proj, group: q.group, split: q.split, query: q.query, s: score(q, rows.map((r) => ({ path: r.path, text: r.t }))) });
    }
  }
  return out;
}
export function summarize(res) {
  const by = (f) => { const m = {}; for (const r of res) (m[f(r)] ||= []).push(r.s); return Object.fromEntries(Object.entries(m).map(([k, v]) => [k, agg(v)])); };
  return { overall: agg(res.map((r) => r.s)), byGroup: by((r) => r.group), byProject: by((r) => r.project), bySplit: by((r) => r.split) };
}
export const pct = (x) => (100 * x).toFixed(0);
export function line(name, sm) {
  const o = sm.overall;
  return `${name.padEnd(28)} первым ${pct(o.first).padStart(3)}  Hit@3 ${pct(o.hit3).padStart(3)}  MRR ${o.mrr.toFixed(3)}  nDCG ${o.ndcg.toFixed(3)}  не найден ${pct(1 - o.found).padStart(3)}  (tune ${pct(sm.bySplit.tune.first)}/${pct(sm.bySplit.tune.hit3)}, test ${pct(sm.bySplit.test.first)}/${pct(sm.bySplit.test.hit3)})`;
}
// поимённые потери/приобретения относительно базового варианта
export function diff(base, res) {
  const b = Object.fromEntries(base.map((r) => [r.id, r.s.rank || 99]));
  const lost = [], gained = [];
  for (const r of res) { const x = r.s.rank || 99; const y = b[r.id]; if (x > y && (y <= 3 || x > 3)) lost.push(r.id); else if (x < y && (x <= 3 || y > 3)) gained.push(r.id); }
  return { lost, gained };
}
