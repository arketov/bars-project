// size.mjs — размер и время построения FTS-индекса: база (2 колонки) против полей+точной формы (4 колонки), триграммы. Файлы во временной папке.
import { DatabaseSync } from 'node:sqlite'; import fs from 'node:fs';
import { loadProject } from './lib.mjs'; import * as A from './analyzers.mjs';
const dir = process.argv[2]; const wl = A.words;
const nameOf = (c) => (c.title + ' ' + c.path.split('/').pop().replace(/\.md$/i, '')).replace(/[_\-.]/g, ' ');
const m = (t, f) => wl(t).map(f).join(' '); const ex = (w) => w.toLowerCase().replace(/ё/g, 'е');
for (const proj of ['bars-project', 'RoleGPT', 'agaugu']) {
  const { chunks } = loadProject(proj);
  const textMB = chunks.reduce((s, c) => s + Buffer.byteLength(c.t) + Buffer.byteLength(c.h), 0) / 1e6;
  const mk = (name, cols, n, tok) => {
    const p = `${dir}/${name}.db`; try { fs.unlinkSync(p); } catch {}
    const d = new DatabaseSync(p); const t0 = Date.now();
    d.exec(`create virtual table f using fts5(${Array.from({ length: n }, (_, i) => 'c' + i)}, tokenize="${tok}")`);
    const ins = d.prepare(`insert into f(rowid,${Array.from({ length: n }, (_, i) => 'c' + i)}) values (?,${Array.from({ length: n }, () => '?')})`);
    d.exec('begin'); chunks.forEach((c, i) => ins.run(i, ...cols(c))); d.exec('commit'); d.exec("insert into f(f) values('optimize')"); d.close();
    const ms = Date.now() - t0; const sz = fs.statSync(p).size / 1e6; fs.unlinkSync(p); return `${name}: ${sz.toFixed(1)} МБ, ${ms} мс`;
  };
  const u = "unicode61 remove_diacritics 2 tokenchars '_'";
  console.log(proj, `текст ${textMB.toFixed(1)} МБ, чанков ${chunks.length}`);
  console.log(' ', mk('база-2к', (c) => [m(c.h, A.stemPlain), m(c.t, A.stemPlain)], 2, u));
  console.log(' ', mk('поля-4к', (c) => [m(nameOf(c), A.stemRules), m(c.h, A.stemRules), m(c.t, A.stemRules), m(nameOf(c) + ' ' + c.h + ' ' + c.t, ex)], 4, u));
  console.log(' ', mk('trigram-3к', (c) => [nameOf(c), c.h, c.t], 3, 'trigram'));
}
