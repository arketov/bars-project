// Русский Snowball (по описанию snowballstem.org) + грубая замена английскому; выученное соответствие слово→основа
// из самой копии индекса имеет приоритет (оно точное для слов корпуса).
const V = 'аеиоуыэюя';
const isV = (c) => V.includes(c);
function regions(w) {
  let rv = w.length, r1 = w.length, r2 = w.length;
  for (let i = 0; i < w.length; i++) if (isV(w[i])) { rv = i + 1; break; }
  for (let i = 1; i < w.length; i++) if (!isV(w[i]) && isV(w[i - 1])) { r1 = i + 1; break; }
  for (let i = r1 + 1; i < w.length; i++) if (!isV(w[i]) && isV(w[i - 1])) { r2 = i + 1; break; }
  return { rv, r1, r2 };
}
const longest = (w, from, list) => { let best = ''; for (const s of list) if (s.length > best.length && w.endsWith(s) && w.length - s.length >= from) best = s; return best; };
const G1 = ['ившись', 'ывшись', 'ивши', 'ывши', 'ив', 'ыв'];
const G1A = ['авшись', 'авши', 'ав'];
const REFL = ['ся', 'сь'];
const ADJ = ['ими', 'ыми', 'его', 'ого', 'ему', 'ому', 'ее', 'ие', 'ые', 'ое', 'ей', 'ий', 'ый', 'ой', 'ем', 'им', 'ым', 'ом', 'их', 'ых', 'ую', 'юю', 'ая', 'яя', 'ою', 'ею'];
const PART_A = ['ем', 'нн', 'вш', 'ющ', 'щ'];
const PART_B = ['ивш', 'ывш', 'ующ'];
const VERB_A = ['ете', 'йте', 'нно', 'ешь', 'ла', 'на', 'ли', 'ем', 'ло', 'но', 'ет', 'ют', 'ны', 'ть', 'й', 'л', 'н'];
const VERB_B = ['ейте', 'уйте', 'ила', 'ыла', 'ена', 'ите', 'или', 'ыли', 'ило', 'ыло', 'ено', 'ует', 'уют', 'ены', 'ить', 'ыть', 'ишь', 'ей', 'уй', 'ил', 'ыл', 'им', 'ым', 'ен', 'ят', 'ит', 'ыт', 'ую', 'ю'];
const NOUN = ['иями', 'ями', 'ами', 'ией', 'иям', 'ием', 'иях', 'ев', 'ов', 'ие', 'ье', 'еи', 'ии', 'ей', 'ой', 'ий', 'ям', 'ем', 'ам', 'ом', 'ах', 'ях', 'ию', 'ью', 'ия', 'ья', 'а', 'е', 'и', 'й', 'о', 'у', 'ы', 'ь', 'ю', 'я'];
function withPrec(w, rv, listA, listB) {
  // группа A: окончание, перед которым стоит а или я; группа B: без условия
  const b = longest(w, rv, listB);
  let a = '';
  for (const s of listA) if (s.length > a.length && w.endsWith(s) && w.length - s.length >= rv && w.length - s.length > 0 && 'ая'.includes(w[w.length - s.length - 1]) && w.length - s.length - 1 >= rv - 0) a = s;
  if (b.length >= a.length && b) return b;
  return a;
}
export function stemRu(word) {
  let w = word.replace(/ё/g, 'е');
  if (w.length < 2) return w;
  const { rv, r2 } = regions(w);
  const cut = (s) => { w = w.slice(0, w.length - s.length); };
  // шаг 1
  let s = withPrec(w, rv, G1A, G1);
  if (s) cut(s);
  else {
    const rf = longest(w, rv, REFL); if (rf) cut(rf);
    const adj = longest(w, rv, ADJ);
    if (adj) {
      cut(adj);
      const p = withPrec(w, rv, PART_A, PART_B); if (p) cut(p);
    } else {
      const v = withPrec(w, rv, VERB_A, VERB_B);
      if (v) cut(v); else { const n = longest(w, rv, NOUN); if (n) cut(n); }
    }
  }
  // шаг 2
  if (w.endsWith('и') && w.length - 1 >= rv) w = w.slice(0, -1);
  // шаг 3
  if (w.endsWith('ость') && w.length - 4 >= r2) w = w.slice(0, -4);
  else if (w.endsWith('ост') && w.length - 3 >= r2) w = w.slice(0, -3);
  // шаг 4
  if (w.endsWith('нн') && w.length - 1 >= rv) w = w.slice(0, -1);
  else if ((w.endsWith('ейше') && w.length - 4 >= rv)) { w = w.slice(0, -4); if (w.endsWith('нн')) w = w.slice(0, -1); }
  else if (w.endsWith('ейш') && w.length - 3 >= rv) { w = w.slice(0, -3); if (w.endsWith('нн')) w = w.slice(0, -1); }
  else if (w.endsWith('ь') && w.length - 1 >= rv) w = w.slice(0, -1);
  return w;
}
export function stemEn(w) {
  // очень грубо: используется только для слов запроса, которых нет в корпусе
  for (const [suf, rep] of [['ational', 'ate'], ['ization', 'ize'], ['ies', 'i'], ['ing', ''], ['edly', ''], ['ed', ''], ['es', ''], ['s', '']]) {
    if (w.length > suf.length + 2 && w.endsWith(suf)) return w.slice(0, -suf.length) + rep;
  }
  return w;
}
const cyr = (c) => /^[Ѐ-ӿ]$/.test(c);
export const words = (t) => t.split(/[^\p{L}\p{N}]+/u).filter(Boolean);
export function makeStemmer(learned) {
  return (raw) => {
    const out = [];
    for (const wd of words(raw)) {
      const l = wd.toLowerCase();
      if (learned && learned.has(l)) { out.push(learned.get(l)); continue; }
      if ([...l].every((c) => cyr(c) && /\p{L}/u.test(c))) out.push(stemRu(l));
      else if (/^[a-z]+$/.test(l)) out.push(stemEn(l));
      else out.push(l);
    }
    return out;
  };
}
