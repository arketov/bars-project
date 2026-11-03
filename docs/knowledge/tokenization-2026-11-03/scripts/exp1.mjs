// exp1.mjs — токенизация: варианты стемминга, правил, полей, лемм, триграмм на 168 запросах бенча (только FTS, на копиях индексов).
import fs from 'node:fs';
import { PROJECTS, loadProject, evaluate, summarize, line, diff } from './lib.mjs';
import * as A from './analyzers.mjs';
await A.initAz();
const data = PROJECTS.map(loadProject);
const nameOf = (c) => (c.title + ' ' + c.path.split('/').pop().replace(/\.md$/i, '')).replace(/[_\-.]/g, ' ');
const wl = A.words;
const mapW = (t, f) => wl(t).map(f).join(' ');

// сборщик варианта по функции нормализации слов
function simple(name, norm, { w = [1, 1], stop = false, ident = false } = {}) {
  const idx = (t) => mapW(t, norm) + (ident ? ' ' + A.identForms(t).join(' ') : '');
  const qt = (q) => {
    let ws = wl(q); if (stop) ws = ws.filter((x) => !A.STOP.has(x.toLowerCase()));
    const t = ws.map(norm); if (ident) t.push(...A.identForms(q)); return t;
  };
  return A.makeFts({ name, ncols: 2, cols: (c) => [idx(c.h), idx(c.t)], weights: w, qbuild: (q) => A.orTerms([0, 1], qt(q)) });
}
const V = [];
V.push(simple('exact (без стемминга)', (w) => w.toLowerCase().replace(/ё/g, 'е')));
V.push(simple('snowball ru/en (как сейчас)', A.stemPlain));
V.push(simple('snowball, заголовок x5', A.stemPlain, { w: [5, 1] }));
V.push(simple('snowball + правила', A.stemRules));
V.push(simple('snowball + правила, x5', A.stemRules, { w: [5, 1] }));
V.push(simple('snowball + стоп-слова', A.stemPlain, { stop: true }));
V.push(simple('snowball + идентификаторы', A.stemPlain, { ident: true }));
for (const n of [5, 6, 7]) V.push(simple(`префикс ${n}`, A.stemPrefix(n)));
V.push(simple('лемма (первый разбор)', (w) => A.lemmas(w)[0]));
V.push(simple('лемма+snowball ru/en', (w) => A.lemmas(w)[0] === w.toLowerCase() ? A.stemPlain(w) : A.lemmas(w)[0]));

const cache = {};
const base = {};
const out = {};
for (const v of V) {
  const t0 = Date.now();
  const res = evaluate(data, v, cache);
  const sm = summarize(res);
  out[v.name] = { ...sm, res: res.map((r) => ({ id: r.id, rank: r.s.rank })) };
  if (v.name.startsWith('snowball ru/en')) base.res = res;
  console.log(line(v.name, sm), ((Date.now() - t0) / 1000).toFixed(0) + 'с');
}
fs.writeFileSync('exp1.json', JSON.stringify(out));
