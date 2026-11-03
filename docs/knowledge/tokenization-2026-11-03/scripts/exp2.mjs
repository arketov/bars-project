// exp2.mjs — две колонки (основа + точная форма), поля названия/имени файла, покрытие слов запроса, триграммы.
import fs from 'node:fs';
import { DatabaseSync } from 'node:sqlite';
import { PROJECTS, loadProject, evaluate, summarize, line, diff, GROUPS, pct } from './lib.mjs';
import * as A from './analyzers.mjs';
await A.initAz();
const data = PROJECTS.map(loadProject);
const wl = A.words;
const exact = (w) => w.toLowerCase().replace(/ё/g, 'е');
const nameOf = (c) => (c.title + ' ' + c.path.split('/').pop().replace(/\.md$/i, '')).replace(/[_\-.]/g, ' ');
const mapW = (t, f) => wl(t).map(f).join(' ');
const stem = A.stemRules;

// колонки: 0 основа заголовка, 1 основа текста, 2 точная форма (заголовок+текст)
function dual(name, w) {
  return A.makeFts({ name, ncols: 3, cols: (c) => [mapW(c.h, stem), mapW(c.t, stem), mapW(c.h + ' ' + c.t, exact)], weights: w,
    qbuild: (q) => A.joinOr([A.orTerms([0, 1], wl(q).map(stem)), A.orTerms([2], wl(q).map(exact))]) });
}
// колонки: 0 название документа и имя файла, 1 заголовки, 2 текст, 3 точная форма всего
function fields(name, w) {
  return A.makeFts({ name, ncols: 4, cols: (c) => [mapW(nameOf(c), stem), mapW(c.h, stem), mapW(c.t, stem), mapW(nameOf(c) + ' ' + c.h + ' ' + c.t, exact)], weights: w,
    qbuild: (q) => A.joinOr([A.orTerms([0, 1, 2], wl(q).map(stem)), A.orTerms([3], wl(q).map(exact))]) });
}
// покрытие: bm25, умноженный на долю различных слов запроса, найденных в куске (степень p)
function withCoverage(v, p, norm = stem, fieldsOf = (c) => nameOf(c) + ' ' + c.h + ' ' + c.t) {
  const tok = new Map();
  return { ...v, name: v.name + ` ×покрытие^${p}`,
    search(ctx, query, limit) {
      const qt = [...new Set(wl(query).filter((x) => !A.STOP.has(x.toLowerCase())).map(norm))];
      const m = ctx.qbuild ? null : null;
      const raw = ctx.d.prepare(`select rowid, bm25(f,${ctx.weights.join(',')}) b from f where f match ? order by b limit 100`).all(this.mk(query, ctx));
      const sc = raw.map((r) => {
        const c = ctx.chunks[r.rowid];
        let ts = tok.get(r.rowid); if (!ts) { ts = new Set(wl(fieldsOf(c)).map(norm)); tok.set(r.rowid, ts); }
        const cov = qt.length ? qt.filter((t) => ts.has(t)).length / qt.length : 1;
        return { c, s: -r.b * Math.pow(Math.max(cov, 0.05), p) };
      });
      sc.sort((a, b) => b.s - a.s);
      return sc.slice(0, limit).map((x) => x.c);
    } };
}
function trigram(name, mode) {
  return { name,
    build(chunks) {
      const d = new DatabaseSync(':memory:');
      d.exec('create virtual table f using fts5(c0,c1,c2, tokenize="trigram")');
      const ins = d.prepare('insert into f(rowid,c0,c1,c2) values (?,?,?,?)');
      d.exec('begin'); chunks.forEach((c, i) => ins.run(i, nameOf(c), c.h, c.t)); d.exec('commit');
      return { d, chunks, weights: [1, 1, 1] };
    },
    search(ctx, query, limit) {
      const terms = [];
      for (const w of wl(query)) {
        const l = exact(w); if (l.length < 3 || A.STOP.has(l)) continue;
        if (mode === 'word') terms.push('"' + l + '"');
        else if (mode === 'trim') terms.push('"' + (l.length >= 7 ? l.slice(0, l.length - 2) : l.length >= 5 ? l.slice(0, l.length - 1) : l) + '"');
        else if (mode === 'grams') for (let i = 0; i + 3 <= l.length; i++) terms.push('"' + l.slice(i, i + 3) + '"');
      }
      const ts = [...new Set(terms)]; if (!ts.length) return [];
      const rows = ctx.d.prepare(`select rowid from f where f match ? order by bm25(f,3,3,1) limit ?`).all(ts.join(' OR '), limit);
      return rows.map((r) => ctx.chunks[r.rowid]);
    } };
}

const V = [];
const base = A.makeFts({ name: 'snowball + правила (база)', ncols: 2, cols: (c) => [mapW(c.h, stem), mapW(c.t, stem)], weights: [1, 1], qbuild: (q) => A.orTerms([0, 1], wl(q).map(stem)) });
V.push(base);
for (const w of [[3, 1, 0.5], [3, 1, 1], [3, 1, 2], [5, 1, 1], [1, 1, 1]]) V.push(dual(`2 колонки w=${w.join(',')}`, w));
for (const w of [[3, 3, 1, 1], [5, 3, 1, 1], [8, 4, 1, 1], [5, 3, 1, 0.5], [5, 3, 1, 2], [2, 2, 1, 1]]) V.push(fields(`поля w=${w.join(',')}`, w));
V.push(trigram('trigram слово целиком', 'word'));
V.push(trigram('trigram слово без хвоста', 'trim'));
V.push(trigram('trigram все триграммы OR', 'grams'));
// покрытие: на базе полей w=5,3,1,1
const fb = fields('поля w=5,3,1,1', [5, 3, 1, 1]);
for (const p of [0.5, 1, 2]) { const v = withCoverage({ ...fb, mk: (q) => A.joinOr([A.orTerms([0, 1, 2], wl(q).map(stem)), A.orTerms([3], wl(q).map(exact))]) }, p); v.build = fb.build; V.push(v); }

const cache = {}, out = {};
let baseRes = null;
for (const v of V) {
  const t0 = Date.now();
  const res = evaluate(data, v, cache);
  const sm = summarize(res);
  if (v === base) baseRes = res;
  const d = baseRes && v !== base ? diff(baseRes, res) : { lost: [], gained: [] };
  out[v.name] = { ...sm, ranks: Object.fromEntries(res.map((r) => [r.id, r.s.rank])), lost: d.lost, gained: d.gained };
  console.log(line(v.name, sm), `−${d.lost.length}/+${d.gained.length}`, ((Date.now() - t0) / 1000).toFixed(0) + 'с');
}
fs.writeFileSync('exp2.json', JSON.stringify(out));
