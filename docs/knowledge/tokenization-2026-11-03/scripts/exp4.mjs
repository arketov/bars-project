// exp4.mjs — сборка лучших приёмов: поля + точная форма + стоп-слова + идентификаторы + леммы + исправление опечаток.
// Печатает таблицу по группам и проектам для базы и лучшего варианта и поимённые потери/приобретения (правило приёмки из search-quality.md).
import fs from 'node:fs';
import { PROJECTS, loadProject, evaluate, summarize, line, diff, GROUPS, pct } from './lib.mjs';
import * as A from './analyzers.mjs';
await A.initAz();
const data = PROJECTS.map(loadProject);
const wl = A.words, stem = A.stemRules;
const exact = (w) => w.toLowerCase().replace(/ё/g, 'е');
const nameOf = (c) => (c.title + ' ' + c.path.split('/').pop().replace(/\.md$/i, '')).replace(/[_\-.]/g, ' ');
const mapW = (t, f) => wl(t).map(f).join(' ');
const lem = (w) => A.lemmas(w)[0];

function combo(name, o) {
  const { wE = 0.5, wL = 0.5, stop = false, ident = false, camel = false, lemma = false, fuzzy = false } = o;
  const exactAll = (c) => {
    const t = nameOf(c) + ' ' + c.h + ' ' + c.t;
    let s = mapW(t, exact);
    if (ident) s += ' ' + A.identForms(t).join(' ');
    if (camel) s += ' ' + wl(t).flatMap((w) => A.camelParts(w)).join(' ');
    return s;
  };
  const ncols = lemma ? 5 : 4;
  const weights = [5, 3, 1, wE, ...(lemma ? [wL] : [])];
  const mk = A.makeFts({ name, ncols, weights,
    cols: (c) => [mapW(nameOf(c), stem), mapW(c.h, stem), mapW(c.t, stem), exactAll(c), ...(lemma ? [mapW(nameOf(c) + ' ' + c.h + ' ' + c.t, lem)] : [])],
    qbuild: (q, ctx) => {
      let ws = wl(q); if (stop) ws = ws.filter((x) => !A.STOP.has(x.toLowerCase()));
      if (!ws.length) ws = wl(q);
      let st = ws.map(stem), ex = ws.map(exact);
      if (ident) ex.push(...A.identForms(q));
      if (camel) ex.push(...ws.flatMap((w) => A.camelParts(w)));
      if (fuzzy) {
        const voc = ctx.getVocab(); const extra = [];
        if (!ctx.byLen) { ctx.byLen = new Map(); for (const [t, df] of voc) (ctx.byLen.get(t.length) || ctx.byLen.set(t.length, []).get(t.length)).push([t, df]); }
        for (const t of st) {
          if (voc.has(t) || t.length < 4 || /\d/.test(t)) continue;
          const max = t.length >= 8 ? 2 : 1; const cands = [];
          for (let L = t.length - max; L <= t.length + max; L++) for (const [v, df] of ctx.byLen.get(L) || []) { const dd = A.osa(t, v, max); if (dd <= max) cands.push([dd, -df, v]); }
          cands.sort((x, y) => x[0] - y[0] || x[1] - y[1]);
          st = st.concat(cands.slice(0, 3).map((c) => c[2]));
        }
      }
      const parts = [A.orTerms([0, 1, 2], st), A.orTerms([3], ex)];
      if (lemma) parts.push(A.orTerms([4], ws.map(lem)));
      return A.joinOr(parts);
    } });
  return mk;
}
const plainStem = A.makeFts({ name: 'БАЗА: snowball ru/en как сейчас', ncols: 2, weights: [1, 1], cols: (c) => [mapW(c.h, A.stemPlain), mapW(c.t, A.stemPlain)], qbuild: (q) => A.orTerms([0, 1], wl(q).map(A.stemPlain)) });
const V = [plainStem,
  combo('поля, exact 0.5', {}), combo('поля, exact 0.3', { wE: 0.3 }), combo('поля, exact 0.2', { wE: 0.2 }),
  combo('+ стоп-слова', { stop: true }), combo('+ идентификаторы', { ident: true }), combo('+ camelCase', { camel: true }), combo('+ леммы 0.5', { lemma: true }), combo('+ леммы 0.25', { lemma: true, wL: 0.25 }),
  combo('+ исправление опечаток', { fuzzy: true }),
  combo('всё: стоп+ident+camel+fuzzy', { stop: true, ident: true, camel: true, fuzzy: true }),
  combo('всё + леммы 0.25', { stop: true, ident: true, camel: true, fuzzy: true, lemma: true, wL: 0.25 }),
];
const cache = {}, out = {}; let baseRes = null; const full = {};
for (const v of V) {
  const t0 = Date.now(); const res = evaluate(data, v, cache); const sm = summarize(res);
  if (!baseRes) baseRes = res;
  const d = v === V[0] ? { lost: [], gained: [] } : diff(baseRes, res);
  out[v.name] = { ...sm, ranks: Object.fromEntries(res.map((r) => [r.id, r.s.rank])), lost: d.lost, gained: d.gained };
  console.log(line(v.name, sm), `−${d.lost.length}/+${d.gained.length}`, ((Date.now() - t0) / 1000).toFixed(0) + 'с');
}
fs.writeFileSync('exp4.json', JSON.stringify(out));
console.log('\nПо группам: первым / Hit@3 / MRR — база → ' + V.length + ' вариантов (печать выбранных)');
const pick = ['БАЗА: snowball ru/en как сейчас', 'поля, exact 0.3', 'всё: стоп+ident+camel+fuzzy', 'всё + леммы 0.25'];
for (const g of Object.keys(GROUPS)) {
  console.log(GROUPS[g].padEnd(30), pick.map((k) => { const x = out[k].byGroup[g]; return `${pct(x.first)}/${pct(x.hit3)}/${x.mrr.toFixed(2)}`; }).join('  '));
}
console.log('\nПо проектам');
for (const p of PROJECTS) console.log(p.padEnd(18), pick.map((k) => { const x = out[k].byProject[p]; return `${pct(x.first)}/${pct(x.hit3)}/${x.mrr.toFixed(2)}`; }).join('  '));
