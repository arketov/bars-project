// analyzers.mjs — токенизаторы и построители MATCH для опытов. Всё на node, без сборки Rust.
import { DatabaseSync } from 'node:sqlite';
import { req } from './lib.mjs';
const snow = req('snowball-stemmers');
const Az = req('az');
export const ru = snow.newStemmer('russian'), en = snow.newStemmer('english');
export let azReady = false;
export function initAz() { return new Promise((res) => Az.Morph.init(() => { azReady = true; res(); })); }

export const words = (t) => t.split(/[^\p{L}\p{N}]+/u).filter(Boolean);
const isCyr = (w) => /^[\u0400-\u04FF]+$/.test(w);
const isLat = (w) => /^[a-z]+$/i.test(w);
const fold = (w) => w.toLowerCase().replace(/ё/g, 'е');
const isCaps = (w) => w.length >= 2 && w === w.toUpperCase() && w !== w.toLowerCase();

// стемминг как в продукте: кириллица → русский Snowball, латиница → английский, остальное как есть
export function stemPlain(w) {
  const l = fold(w);
  if (isCyr(l)) return ru.stem(l);
  if (isLat(l)) return en.stem(l);
  return l;
}
// правила: слова ПРОПИСНЫМИ, короче 4 знаков, числа, смесь букв и цифр не трогаем
export function stemRules(w) {
  const l = fold(w);
  if (isCaps(w) || l.length < 4 || /\d/.test(l)) return l;
  return stemPlain(w);
}
export function stemPrefix(n) { return (w) => { const l = fold(w); return isCyr(l) || isLat(l) ? l.slice(0, Math.max(n, 1)) : l; }; }
const lemmaCache = new Map();
export function lemmas(w, all = false) {
  const l = fold(w);
  if (!isCyr(l) || l.length < 2) return [l];
  const k = (all ? 'a' : 'f') + l;
  if (lemmaCache.has(k)) return lemmaCache.get(k);
  const ps = Az.Morph(l);
  let r;
  if (!ps.length) r = [l];
  else if (all) r = [...new Set(ps.map((p) => p.normalize().word.replace(/ё/g, 'е')))];
  else r = [ps[0].normalize().word.replace(/ё/g, 'е')];
  lemmaCache.set(k, r);
  return r;
}
// составные слова: идентификаторы с _ . / - и camelCase
export function identForms(text) {
  const out = [];
  for (const m of text.matchAll(/[\p{L}\p{N}]+(?:[_.\/\-][\p{L}\p{N}]+)+/gu)) out.push(m[0].toLowerCase().replace(/[.\/\-]/g, '_'));
  return out;
}
export function camelParts(w) {
  const parts = w.match(/\p{Lu}+(?!\p{Ll})|\p{Lu}?\p{Ll}+|\p{N}+/gu) || [];
  return parts.length > 1 ? parts.map((p) => p.toLowerCase()) : [];
}
export const STOP = new Set(('и в во не что он на я с со как а то все она так его но да ты к у же вы за бы по только ее мне было вот от меня еще нет о из ему теперь когда даже ну вдруг ли если уже или ни быть был него до вас нибудь опять уж вам ведь там потом себя ничего ей может они тут где есть надо ней для мы тебя их чем была сам чтоб без будто чего раз тоже себе под будет ж тогда кто этот того потому этого какой совсем ним здесь этом один почти мой тем чтобы нее сейчас были куда зачем всех никогда можно при наконец два об другой хоть после над больше тот через эти нас про всего них какая много разве три эту моя впрочем хорошо свою этой перед иногда лучше чуть том нельзя такой им более всегда конечно всю между ' +
  'the a an and or of to in on for with is are was were be been it this that these those as at by from how what why when where which who do does did can could should would not no if then than but so such into about over under between ' +
  'как какие каким какой какая какого какую каких кто чем где когда зачем почему можно нужно надо это эти').split(' '));

// === построитель FTS5-варианта ===
// cols(chunk) → массив строк (уже токенизированных, через пробел); qbuild(query) → [{cols:[индексы], terms:[…]}]; weights — веса bm25.
export function makeFts({ name, cols, qbuild, weights, tokenize = "unicode61 remove_diacritics 2 tokenchars '_'", ncols }) {
  return {
    name,
    build(chunks) {
      const d = new DatabaseSync(':memory:');
      const cn = Array.from({ length: ncols }, (_, i) => 'c' + i);
      d.exec(`create virtual table f using fts5(${cn.join(',')}, tokenize="${tokenize}")`);
      const ins = d.prepare(`insert into f(rowid,${cn.join(',')}) values (?,${cn.map(() => '?').join(',')})`);
      d.exec('begin');
      chunks.forEach((c, i) => ins.run(i, ...cols(c)));
      d.exec('commit');
      let vocab = null;
      return { d, chunks, getVocab() { if (!vocab) { d.exec("create virtual table v using fts5vocab(f,'row')"); vocab = new Map(d.prepare('select term, doc from v').all().map((r) => [r.term, r.doc])); } return vocab; }, weights };
    },
    search(ctx, query, limit, q) {
      const m = qbuild(query, ctx);
      if (!m) return [];
      const w = ctx.weights.join(',');
      const rows = ctx.d.prepare(`select rowid from f where f match ? order by bm25(f,${w}) limit ?`).all(m, limit);
      return rows.map((r) => ctx.chunks[r.rowid]);
    },
  };
}
const quote = (t) => '"' + t.replace(/"/g, '') + '"';
export function orTerms(colIdx, terms, nc) {
  const ts = [...new Set(terms.filter((t) => t && t.length >= 1))];
  if (!ts.length) return null;
  const cs = colIdx.map((i) => 'c' + i).join(' ');
  return `{${cs}} : (${ts.map(quote).join(' OR ')})`;
}
export const joinOr = (parts) => { const p = parts.filter(Boolean); return p.length ? p.join(' OR ') : null; };

// расстояние Дамерау—Левенштейна (OSA) с отсечкой
export function osa(a, b, max) {
  const m = a.length, n = b.length; if (Math.abs(m - n) > max) return max + 1;
  let p2 = null, p1 = Array.from({ length: n + 1 }, (_, j) => j);
  for (let i = 1; i <= m; i++) {
    const cur = [i]; let rowMin = i;
    for (let j = 1; j <= n; j++) {
      let v = Math.min(p1[j] + 1, cur[j - 1] + 1, p1[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
      if (i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) v = Math.min(v, p2[j - 2] + 1);
      cur.push(v); if (v < rowMin) rowMin = v;
    }
    if (rowMin > max) return max + 1;
    p2 = p1; p1 = cur;
  }
  return p1[n];
}
