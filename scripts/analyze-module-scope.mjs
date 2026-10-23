// Замер выбора модулей (docs/features/agent-tools/module-scope.md): попарное сравнение областей
// поиска A (весь проект), B (модули эталонов), C (модуль первого эталона), D (без chats).
// Node.js, без дополнительных пакетов.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
import { metrics } from './analyze-quality.mjs';

const VARIANTS = { A: 'all', B: 'gold', C: 'first', D: 'nochats' };
// Порог rerank по умолчанию (Т-24); без rerank — без порога.
const THRESHOLD = { 15: 0.1, 0: 0 };
const mean = xs => xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : 0;
const moduleOf = path => {
  const inner = path.replace(/^docs\//, '');
  return inner.includes('/') ? inner.split('/')[0] : '.';
};

export function caseScore(row, threshold) {
  const kept = row.hits.filter(h => h.score >= threshold);
  const top5 = kept.slice(0, 5);
  return {
    ...metrics(kept, row.goldCount),
    chatsTop5: top5.length ? top5.filter(h => moduleOf(h.path) === 'chats').length / top5.length : 0,
  };
}

function loadRows(dir, k) {
  const rows = fs.readFileSync(`${dir}/rows.jsonl`, 'utf8').trim().split('\n').filter(Boolean).map(l => JSON.parse(l));
  return new Map(rows.filter(r => r.mode === 'search' && r.repeat === 0 && r.candidates === 30 && r.k === k).map(r => [r.id, r]));
}

// Разница B−A и т. п. на одних и тех же вопросах.
export function paired(left, right, ids) {
  const keys = ['top1', 'hit3', 'mrr10', 'ndcg10', 'chatsTop5'];
  const out = { n: ids.length };
  for (const key of keys) {
    out[key] = { a: mean(ids.map(id => left.get(id)[key])), b: mean(ids.map(id => right.get(id)[key])) };
    out[key].diff = out[key].b - out[key].a;
  }
  out.hit3Wins = ids.filter(id => right.get(id).hit3 > left.get(id).hit3).length;
  out.hit3Losses = ids.filter(id => right.get(id).hit3 < left.get(id).hit3).length;
  out.mrrWins = ids.filter(id => right.get(id).mrr10 > left.get(id).mrr10 + 1e-9).length;
  out.mrrLosses = ids.filter(id => right.get(id).mrr10 < left.get(id).mrr10 - 1e-9).length;
  return out;
}

function projectScores(cases, root, k) {
  const scores = {};
  for (const [variant, scope] of Object.entries(VARIANTS)) {
    const rows = loadRows(`${root}/${scope}`, k);
    const expected = cases.filter(c => c.gold.length && (scope !== 'nochats' || !c.gold.some(g => moduleOf(g.path) === 'chats')));
    for (const c of expected) assert(rows.has(c.id), `${root}/${scope}: нет строки ${c.id} K=${k}`);
    assert.equal(rows.size, expected.length, `${root}/${scope}: лишние строки K=${k}`);
    scores[variant] = new Map(expected.map(c => [c.id, caseScore({ ...rows.get(c.id), goldCount: c.gold.length }, THRESHOLD[k])]));
  }
  return scores;
}

function compare(all) {
  const positives = all.cases.filter(c => c.gold.length).map(c => c.id);
  const multi = all.cases.filter(c => new Set(c.gold.map(g => moduleOf(g.path))).size > 1).map(c => c.id);
  const nonChats = all.cases.filter(c => c.gold.length && !c.gold.some(g => moduleOf(g.path) === 'chats')).map(c => c.id);
  const s = all.scores;
  const categories = {};
  for (const category of [...new Set(all.cases.map(c => c.category))]) {
    const ids = all.cases.filter(c => c.category === category).map(c => c.id);
    categories[category] = { n: ids.length, ...Object.fromEntries(['A', 'B', 'C'].map(v => [v, { hit3: mean(ids.map(id => s[v].get(id).hit3)), mrr10: mean(ids.map(id => s[v].get(id).mrr10)) }])) };
  }
  return {
    positives: positives.length,
    BvsA: paired(s.A, s.B, positives),
    CvsA: paired(s.A, s.C, positives),
    CvsAmulti: paired(s.A, s.C, multi),
    DvsAnonChats: paired(s.A, s.D, nonChats),
    categories,
  };
}

// Решение по module-scope.md: оба условия на всех вопросах (обоих проектов вместе).
export function decide(result) {
  const b = result.BvsA, c = result.CvsA, d = result.DvsAnonChats;
  const gain = b.hit3.diff >= 0.05 || b.mrr10.diff >= 0.05;
  const safe = c.hit3.a - c.hit3.b <= 0.01;
  return {
    gain, safe, strongWording: gain && safe,
    chatsRevisit: d.hit3.diff >= 0.05 || d.mrr10.diff >= 0.05,
  };
}

function analyze(specs) {
  const out = { threshold: THRESHOLD, projects: {}, pooled: {} };
  for (const k of [15, 0]) {
    const merged = { cases: [], scores: { A: new Map(), B: new Map(), C: new Map(), D: new Map() } };
    for (const { name, casesPath, root } of specs) {
      const cases = JSON.parse(fs.readFileSync(casesPath, 'utf8').replace(/^﻿/, ''));
      const scores = projectScores(cases, root, k);
      (out.projects[name] ??= {})[`K${k}`] = compare({ cases, scores });
      merged.cases.push(...cases);
      for (const v of Object.keys(scores)) for (const [id, sc] of scores[v]) merged.scores[v].set(id, sc);
    }
    const pooled = compare(merged);
    out.pooled[`K${k}`] = { ...pooled, decision: decide(pooled) };
  }
  return out;
}

function selfTest() {
  const hit = (relevant, path, score = .9) => ({ relevant, path, score });
  const sc = caseScore({ hits: [hit([], 'docs/chats/a.md'), hit([0], 'docs/features/x.md'), hit([], 'docs/chats/b.md', .05)], goldCount: 1 }, .1);
  assert.equal(sc.hit3, 1); assert.equal(sc.mrr10, .5); assert.equal(sc.chatsTop5, .5);
  assert.equal(moduleOf('docs/MAIN_IDEA.md'), '.'); assert.equal(moduleOf('docs/adr/0001-x.md'), 'adr');
  const a = new Map([['q1', { top1: 0, hit3: 0, mrr10: .2, ndcg10: 0, chatsTop5: .6 }], ['q2', { top1: 1, hit3: 1, mrr10: 1, ndcg10: 1, chatsTop5: 0 }]]);
  const b = new Map([['q1', { top1: 1, hit3: 1, mrr10: 1, ndcg10: 1, chatsTop5: 0 }], ['q2', { top1: 1, hit3: 1, mrr10: 1, ndcg10: 1, chatsTop5: 0 }]]);
  const p = paired(a, b, ['q1', 'q2']);
  assert.equal(p.hit3.diff, .5); assert.equal(p.hit3Wins, 1); assert.equal(p.hit3Losses, 0);
  const d = decide({ BvsA: p, CvsA: paired(a, a, ['q1', 'q2']), DvsAnonChats: paired(a, a, ['q1']) });
  assert(d.gain && d.safe && d.strongWording && !d.chatsRevisit);
  const lossy = decide({ BvsA: p, CvsA: paired(b, a, ['q1', 'q2']), DvsAnonChats: p });
  assert(!lossy.safe && !lossy.strongWording && lossy.chatsRevisit);
  console.log('module scope analysis: all assertions passed');
}

const main = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;
if (main && process.argv[2] === '--self-test') selfTest();
else if (main) {
  const [output, ...rest] = process.argv.slice(2);
  if (!rest.length) throw Error('node scripts/analyze-module-scope.mjs <summary.json> <name>=<cases.json>=<runs-root> [...]  (runs-root содержит all/ gold/ first/ nochats/)');
  const specs = rest.map(s => { const [name, casesPath, root] = s.split('='); return { name, casesPath, root }; });
  const result = analyze(specs);
  fs.writeFileSync(output, JSON.stringify(result, null, 2) + '\n');
  const line = (label, p) => console.log(`${label} n=${p.n} hit3 ${p.hit3.a.toFixed(3)}→${p.hit3.b.toFixed(3)} (${p.hit3.diff >= 0 ? '+' : ''}${p.hit3.diff.toFixed(3)}, +${p.hit3Wins}/−${p.hit3Losses}) MRR ${p.mrr10.a.toFixed(3)}→${p.mrr10.b.toFixed(3)} chatsTop5 ${p.chatsTop5.a.toFixed(2)}→${p.chatsTop5.b.toFixed(2)}`);
  for (const [scope, data] of [...Object.entries(result.projects), ['pooled', result.pooled]]) {
    for (const k of ['K15', 'K0']) {
      const r = data[k];
      line(`${scope} ${k} B−A`, r.BvsA); line(`${scope} ${k} C−A`, r.CvsA);
      line(`${scope} ${k} C−A multi`, r.CvsAmulti); line(`${scope} ${k} D−A nonchats`, r.DvsAnonChats);
      if (r.decision) console.log(`${scope} ${k} decision ${JSON.stringify(r.decision)}`);
    }
  }
}
