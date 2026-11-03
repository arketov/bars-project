// feat.mjs — признаки полнотекстовых сигналов для опытов схемы баллов (03-11-2026). Только node:sqlite, без сборки.
// Для каждого запроса: пул top-N по bm25(5,3,1,0.3) и по этим кандидатам — баллы полей по отдельности, покрытие слов, фраза.
// Стемминг — собственная копия Snowball из search-bench-2026-11-03/scripts/stem.mjs (приближение, не rust-stemmers).
import { DatabaseSync } from 'node:sqlite';
import fs from 'node:fs';
import { stemRu, stemEn } from '../../search-bench-2026-11-03/scripts/stem.mjs';

export const HOME = process.env.USERPROFILE + '/.bars/bench/';
export const PROJECTS = ['bars-project', 'overdrive-bots', 'RoleGPT', 'agaugu', 'tmc-opovechenie'];
export const wl = (t) => t.split(/[^\p{L}\p{N}]+/u).filter(Boolean);
const fold = (w) => w.toLowerCase().replace(/ё/g, 'е');
const isCyr = (w) => /^[\u0400-\u04FF]+$/.test(w);
const isLat = (w) => /^[a-z]+$/i.test(w);
const isCaps = (w) => w.length >= 2 && w === w.toUpperCase() && w !== w.toLowerCase();
export function stem(w) {
  const l = fold(w);
  if (isCaps(w) || l.length < 4 || /\d/.test(l)) return l;
  if (isCyr(l)) return stemRu(l);
  if (isLat(l)) return stemEn(l);
  return l;
}
const identForms = (text) => [...text.matchAll(/[\p{L}\p{N}]+(?:[_.\/\-][\p{L}\p{N}]+)+/gu)].map((m) => m[0].toLowerCase().replace(/[.\/\-]/g, '_'));
const nameOf = (c) => (c.title + ' ' + c.path.split('/').pop().replace(/\.md$/i, '')).replace(/[_\-.]/g, ' ');
const mapW = (t, f) => wl(t).map(f).join(' ');
const q1 = (t) => '"' + t.replace(/"/g, '') + '"';

export function loadProject(proj) {
  const d = new DatabaseSync(HOME + proj + '/index.sqlite', { readOnly: true });
  const chunks = d.prepare('select c.id id, d.path path, d.title title, d.module module, c.heading_chain h, c.text t, c.first_line fl, c.last_line ll from chunks c join documents d on d.id=c.document_id order by c.id').all();
  d.close();
  const queries = JSON.parse(fs.readFileSync(HOME + proj + '/queries.json', 'utf8')).filter((q) => q.gold.length).map((q) => ({ ...q, split: parseInt(q.id.match(/\d+$/)[0], 10) % 2 ? 'tune' : 'test' }));
  const cli = JSON.parse(fs.readFileSync(HOME + proj + '/cli-full.json', 'utf8'));
  return { proj, chunks, queries, cli: Object.fromEntries(cli.map((x) => [x.id, x])) };
}

export function buildFts(chunks) {
  const d = new DatabaseSync(':memory:');
  d.exec('create virtual table f using fts5(c0,c1,c2,c3, tokenize="unicode61 remove_diacritics 2 tokenchars \'_\'")');
  const ins = d.prepare('insert into f(rowid,c0,c1,c2,c3) values (?,?,?,?,?)');
  d.exec('begin');
  chunks.forEach((c, i) => {
    const all = nameOf(c) + ' ' + c.h + ' ' + c.t;
    c.exactStr = ' ' + wl(all).map(fold).join(' ') + ' ' + identForms(all).join(' ') + ' ';
    ins.run(i, mapW(nameOf(c), stem), mapW(c.h, stem), mapW(c.t, stem), c.exactStr.trim());
  });
  d.exec('commit');
  return d;
}
const sets = new WeakMap();
function chunkStems(c) {
  let s = sets.get(c);
  if (!s) { s = new Set(wl(nameOf(c) + ' ' + c.h + ' ' + c.t).map(stem)); sets.set(c, s); }
  return s;
}
// MATCH как в рекомендации tokenization.md: основы в колонках 0-2, точные формы и идентификаторы в колонке 3
export function matchOf(query) {
  const ws = wl(query);
  const st = [...new Set(ws.map(stem))], ex = [...new Set([...ws.map(fold), ...identForms(query)])];
  const parts = [];
  if (st.length) parts.push('{c0 c1 c2} : (' + st.map(q1).join(' OR ') + ')');
  if (ex.length) parts.push('{c3} : (' + ex.map(q1).join(' OR ') + ')');
  return parts.join(' OR ');
}
export function poolFor(d, chunks, query, limit = 100) {
  const m = matchOf(query);
  if (!m) return [];
  const rows = d.prepare('select rowid r, -bm25(f,1,0,0,0) b0, -bm25(f,0,1,0,0) b1, -bm25(f,0,0,1,0) b2, -bm25(f,0,0,0,1) b3, -bm25(f,5,3,1,0.3) bc from f where f match ? order by bm25(f,5,3,1,0.3) limit ?').all(m, limit);
  const ws = wl(query), stems = [...new Set(ws.map(stem))], fw = ws.map(fold);
  const whole = ' ' + fw.join(' ') + ' ';
  return rows.map((r) => {
    const c = chunks[r.r], cs = chunkStems(c);
    const cov = stems.length ? stems.filter((s) => cs.has(s)).length / stems.length : 0;
    let bi = 0;
    for (let i = 0; i + 1 < fw.length; i++) if (c.exactStr.includes(' ' + fw[i] + ' ' + fw[i + 1] + ' ')) bi++;
    const phrase = fw.length > 1 ? (c.exactStr.includes(whole) ? 1 : bi / (fw.length - 1)) : 0;
    return { c, idx: r.r, b0: r.b0, b1: r.b1, b2: r.b2, b3: r.b3, bc: r.bc, cov, phrase };
  });
}
