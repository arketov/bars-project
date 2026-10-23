// Пункт 3.8.1 плана: исследование поиска противоречий без генерации текста. Только чтение корпуса.
// export <project-root> <new-output-dir>                — тексты и векторы чанков индекса (read-only SQLite)
// sentences <corpus-dir>                                  — предложения-утверждения корпуса (число, статистика)
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import assert from 'node:assert/strict';
import { DatabaseSync } from 'node:sqlite';

const [command, ...args] = process.argv.slice(2);

if (command === 'export') {
  assert.equal(args.length, 2);
  const [root, out] = args.map(p => path.resolve(p));
  assert(!fs.existsSync(path.join(out, 'snapshot.json.gz')));
  const db = new DatabaseSync(path.join(root, '.bars', 'index.sqlite'), { readOnly: true });
  const rows = db.prepare(`SELECT d.path,d.module,d.title,c.ordinal,c.first_line,c.last_line,c.heading_chain,c.text,v.vector
    FROM chunks c JOIN documents d ON d.id=c.document_id LEFT JOIN vectors v ON v.embed_hash=c.embed_hash
    ORDER BY d.path,c.ordinal`).all();
  db.close();
  const withVec = rows.filter(r => r.vector);
  const dim = withVec[0].vector.byteLength / 4;
  const header = Buffer.alloc(12); header.write('BGV1'); header.writeUInt32LE(withVec.length, 4); header.writeUInt32LE(dim, 8);
  fs.mkdirSync(out, { recursive: true });
  const chunks = withVec.map((r, index) => { const { vector, ...rest } = r; return { index, ...rest }; });
  fs.writeFileSync(path.join(out, 'snapshot.json.gz'), zlib.gzipSync(JSON.stringify({ format: 1, chunks })));
  fs.writeFileSync(path.join(out, 'vectors.bin.gz'), zlib.gzipSync(Buffer.concat([header, ...withVec.map(r => Buffer.from(r.vector))])));
  console.log('rows', rows.length, 'with vectors', withVec.length, 'dim', dim, 'documents', new Set(rows.map(r => r.path)).size);
}

// ---------- общие функции разбора утверждений ----------
const readSnapshot = dir => JSON.parse(zlib.gunzipSync(fs.readFileSync(path.join(dir, 'snapshot.json.gz')))).chunks;
const ownText = t => t.replace(/\nother headers:[\s\S]*$/, '').trim();
const tokens = t => (t.toLowerCase().replace(/ё/g, 'е').match(/[\p{L}\p{N}_]+/gu) || []);
const NEG = new Set(['не', 'нет', 'нельзя', 'ни', 'без', 'запрещено', 'нигде', 'никогда', 'отсутствует', 'отклонен', 'вне']);
const ANT = [['обязател', 'необязател'], ['включ', 'выключ'], ['можно', 'нельзя'], ['разрешен', 'запрещен'], ['добавл', 'удален'], ['принят', 'отклонен'], ['готов', 'неготов'], ['есть', 'нет'], ['да', 'нет'], ['вход', 'вне'], ['сохран', 'удаля'], ['показыва', 'скрыва'], ['видн', 'скрыт'], ['всегда', 'никогда'], ['только', 'также']];
const UNIT = [
  [/^(с|сек|секунд\p{L}*|секунда)$/u, 'сек'], [/^(мс|миллисекунд\p{L}*)$/u, 'мс'], [/^(мин|минут\p{L}*)$/u, 'мин'],
  [/^(ч|час\p{L}*)$/u, 'час'], [/^(дн\p{L}*|день|сутки|суток)$/u, 'дн'], [/^(мб|мбайт\p{L}*|mb|мегабайт\p{L}*)$/u, 'мб'],
  [/^(гб|гбайт\p{L}*|gb|гигабайт\p{L}*)$/u, 'гб'], [/^(кб|кбайт\p{L}*|kb)$/u, 'кб'], [/^%$/, '%'], [/^(слов\p{L}*)$/u, 'слов'],
  [/^(знак\p{L}*|символ\p{L}*)$/u, 'знак'], [/^(байт\p{L}*)$/u, 'байт'], [/^(строк\p{L}*)$/u, 'строк'], [/^(шт|штук\p{L}*)$/u, 'шт'],
  [/^(чанк\p{L}*)$/u, 'чанк'], [/^(документ\p{L}*)$/u, 'док'], [/^(раз|раза)$/u, 'раз'], [/^(пар\p{L}*)$/u, 'пар'], [/^(измерен\p{L}*)$/u, 'изм'],
];
const unitOf = w => { for (const [re, u] of UNIT) if (re.test(w)) return u; return ''; };
const stem = w => (w.length > 5 ? w.slice(0, 5) : w);

// числа с единицей; даты, версии, идентификаторы и номера строк не считаются
export function numbersOf(text) {
  const t = text.toLowerCase().replace(/ё/g, 'е');
  const out = [];
  const re = /(?<![\p{L}\p{N}_.\-:/#`])(\d+(?:[.,]\d+)?)(?![\p{N}]|[-:/.]\d|\p{L}?-\d)\s*(%|[\p{L}]{1,14})?/gu;
  let m;
  while ((m = re.exec(t))) {
    const value = m[1].replace(',', '.');
    const before = (t.slice(0, m.index).match(/[\p{L}]{3,}/gu) || []).slice(-2).map(stem);
    out.push({ value: String(Number(value)), unit: m[2] ? unitOf(m[2]) : '', key: before.join(' ') });
  }
  return out;
}
const ID_RE = /(?:Т|НФТ|OQ|ISS|IDEA|ADR|ADR-|В|Д)-\d+(?:\.\d+)*/giu;
const idsOf = text => [...new Set((text.match(ID_RE) || []).map(x => x.toUpperCase().replace('T', 'Т')))];

function statementsOf(chunk, ci) {
  const body = ownText(chunk.text).replace(/^#+ .*$/gm, '');
  const out = [];
  for (const line of body.split(/\n+/)) {
    const cleaned = line.replace(/^\s*(?:[-*]|\d+\.)\s+/, '').replace(/\|/g, ' ').replace(/[*_`]/g, '');
    for (const text of cleaned.split(/(?<=[.!?;])\s+(?=\p{Lu}|[«"(])/u)) {
      const tok = tokens(text);
      if (tok.length < 5) continue;
      out.push({ ci, text: text.trim(), tok });
    }
  }
  return out;
}
const HISTORY = /журнал|раунд|история|ход работы|что сделано/i;
const scopeOf = c => (c.module === 'chats' || c.module === 'adr' ? 'history' : 'current');

export function buildStatements(chunks) {
  const all = [];
  chunks.forEach((c, ci) => { if (c.text.split(/\s+/).length >= 15) all.push(...statementsOf(c, ci)); });
  const df = new Map();
  for (const s of all) {
    s.stems = [...new Set(s.tok.filter(w => !NEG.has(w) && !/^\d+$/.test(w)).map(stem))];
    s.nums = numbersOf(s.text);
    s.ids = idsOf(s.text);
    s.neg = s.tok.filter(w => NEG.has(w)).length;
    for (const w of s.stems) df.set(w, (df.get(w) || 0) + 1);
  }
  const n = all.length;
  const idf = w => Math.log(n / (df.get(w) || 1));
  for (const s of all) s.w = new Map(s.stems.map(w => [w, idf(w)]));
  return { all, df, idf };
}

// ---------- кандидаты пар утверждений ----------
const wj = (a, b) => { // idf-Жаккар
  let inter = 0, uni = 0;
  for (const [w, x] of a.w) { if (b.w.has(w)) inter += x; uni += x; }
  for (const [w, x] of b.w) if (!a.w.has(w)) uni += x;
  return uni ? inter / uni : 0;
};

// условие расхождения чисел: в обоих утверждениях есть числа одной единицы, и наборы не пересекаются
export function numberConflict(a, b) {
  const byUnit = n => { const m = new Map(); for (const x of n.nums) { if (!x.unit) continue; (m.get(x.unit) || m.set(x.unit, new Set()).get(x.unit)).add(x.value); } return m; };
  const A = byUnit(a), B = byUnit(b), diff = [];
  for (const [u, sa] of A) {
    const sb = B.get(u); if (!sb) continue;
    if ([...sa].every(x => !sb.has(x))) diff.push({ unit: u, a: [...sa], b: [...sb] });
  }
  return diff;
}
// замена значения: у похожих утверждений в каждом есть число, которого нет в другом (добавление числа — не замена)
export function swapConflict(a, b) {
  const va = new Set(a.nums.map(x => x.value)), vb = new Set(b.nums.map(x => x.value));
  const onlyA = [...va].filter(x => !vb.has(x)), onlyB = [...vb].filter(x => !va.has(x));
  return onlyA.length && onlyB.length ? [{ a: onlyA, b: onlyB }] : [];
}
// слот «название величины + число»: те же два слова перед числом, значения не пересекаются
export function slotConflict(a, b) {
  const byKey = n => { const m = new Map(); for (const x of n.nums) { if (!x.key) continue; (m.get(x.key) || m.set(x.key, new Set()).get(x.key)).add(x.value); } return m; };
  const A = byKey(a), B = byKey(b), diff = [];
  for (const [k, sa] of A) { const sb = B.get(k); if (!sb) continue; if ([...sa].every(x => !sb.has(x))) diff.push({ key: k, a: [...sa], b: [...sb] }); }
  return diff;
}
export function negationConflict(a, b) { return (a.neg === 0) !== (b.neg === 0); }
export function antonymConflict(a, b) {
  const hit = [];
  const has = (s, p) => s.tok.some(w => w.startsWith(p));
  for (const [x, y] of ANT) {
    const xa = has(a, x), ya = has(a, y), xb = has(b, x), yb = has(b, y);
    if ((xa && !ya && yb && !xb) || (ya && !xa && xb && !yb)) hit.push(`${x}/${y}`);
  }
  return hit;
}

export function candidatePairs(S, chunks, { minShared = 2, rareDf = 40, minSim = 0.4 } = {}) {
  const inv = new Map();
  S.all.forEach((s, i) => {
    for (const w of s.stems) { if ((S.df.get(w) || 0) > rareDf) continue; (inv.get(w) || inv.set(w, []).get(w)).push(i); }
    for (const id of s.ids) (inv.get(id) || inv.set(id, []).get(id)).push(i);
  });
  const counts = new Map();
  for (const list of inv.values()) {
    if (list.length > 400) continue;
    for (let x = 0; x < list.length; x++) for (let y = x + 1; y < list.length; y++) {
      const k = list[x] * 100000 + list[y]; counts.set(k, (counts.get(k) || 0) + 1);
    }
  }
  const pairs = [];
  for (const [k, shared] of counts) {
    if (shared < minShared) continue;
    const i = Math.floor(k / 100000), j = k % 100000, a = S.all[i], b = S.all[j];
    if (a.ci === b.ci) continue;
    if (a.tok.join(' ') === b.tok.join(' ')) continue;
    const sim = wj(a, b);
    if (sim < minSim) continue;
    pairs.push({ i, j, sim, shared });
  }
  return pairs;
}

export function classify(S, pairs) {
  const out = [];
  for (const p of pairs) {
    const a = S.all[p.i], b = S.all[p.j];
    const num = numberConflict(a, b), slot = slotConflict(a, b), swap = swapConflict(a, b), neg = negationConflict(a, b), ant = antonymConflict(a, b);
    if (!num.length && !slot.length && !swap.length && !neg && !ant.length) continue;
    out.push({ ...p, num, slot, swap, neg, ant });
  }
  return out;
}

if (command === 'run') {
  const [corpus, outdir] = args.map(p => path.resolve(p));
  const chunks = readSnapshot(corpus);
  const S = buildStatements(chunks);
  fs.mkdirSync(outdir, { recursive: true });
  const t0 = Date.now();
  const pairs = candidatePairs(S, chunks);
  const flagged = classify(S, pairs);
  const stat = { chunks: chunks.length, statements: S.all.length, withNumbers: S.all.filter(s => s.nums.some(n => n.unit)).length, withIds: S.all.filter(s => s.ids.length).length,
    candidatePairs: pairs.length, flagged: flagged.length, ms: Date.now() - t0,
    byKind: { number: flagged.filter(f => f.num.length).length, slot: flagged.filter(f => f.slot.length).length, swap: flagged.filter(f => f.swap.length).length, negation: flagged.filter(f => f.neg).length, antonym: flagged.filter(f => f.ant.length).length } };
  const scope = f => { const a = chunks[S.all[f.i].ci], b = chunks[S.all[f.j].ci]; return scopeOf(a) === 'current' && scopeOf(b) === 'current' ? 'current' : 'history'; };
  stat.byScope = { current: flagged.filter(f => scope(f) === 'current').length, history: flagged.filter(f => scope(f) === 'history').length };
  console.log(JSON.stringify(stat, null, 2));
  fs.writeFileSync(path.join(outdir, 'run-stat.json'), JSON.stringify(stat, null, 2));
}

export function view(S, chunks, f) {
  const side = i => { const s = S.all[i], c = chunks[s.ci]; return { path: c.path, lines: `${c.first_line}-${c.last_line}`, heading: c.heading_chain, text: s.text }; };
  return { sim: +f.sim.toFixed(3), kinds: [f.num.length && 'number', f.slot.length && 'slot', f.swap.length && 'swap', f.neg && 'negation', f.ant.length && 'antonym'].filter(Boolean), num: f.num, slot: f.slot, swap: f.swap, ant: f.ant, a: side(f.i), b: side(f.j) };
}
if (command === 'dump') {
  const [corpus, outfile, ...opts] = args;
  const chunks = readSnapshot(path.resolve(corpus));
  const S = buildStatements(chunks);
  const flagged = classify(S, candidatePairs(S, chunks)).sort((x, y) => y.sim - x.sim);
  fs.writeFileSync(path.resolve(outfile), JSON.stringify(flagged.map(f => view(S, chunks, f)), null, 1));
  console.log('written', flagged.length);
}

// ---------- история git: настоящие правки утверждений ----------
export function mkStatement(S, text) {
  const tok = tokens(text);
  const s = { text, tok, stems: [...new Set(tok.filter(w => !NEG.has(w) && !/^\d+$/.test(w)).map(stem))], nums: numbersOf(text), ids: idsOf(text), neg: tok.filter(w => NEG.has(w)).length };
  const n = S.all.length;
  s.w = new Map(s.stems.map(w => [w, Math.log(n / (S.df.get(w) || 1))]));
  return s;
}
const lineStatements = line => line.replace(/^\s*(?:[-*]|\d+\.)\s+/, '').replace(/\|/g, ' ').replace(/[*_`]/g, '').split(/(?<=[.!?;])\s+(?=\p{Lu}|[«"(])/u).map(x => x.trim()).filter(x => tokens(x).length >= 5);

if (command === 'history') {
  const { execFileSync } = await import('node:child_process');
  const [corpus, outfile] = args;
  const S = buildStatements(readSnapshot(path.resolve(corpus)));
  const log = execFileSync('git', ['log', '--no-merges', '-U0', '--format=@@C %h %ad', '--date=short', '--', 'docs', 'PLAN.md', 'QUESTIONS.md', ':!docs/chats', ':!docs/knowledge/*/*'], { maxBuffer: 1 << 29, encoding: 'utf8' });
  const edits = [];
  let commit = '', file = '', minus = [], plus = [];
  const flush = () => {
    if (minus.length && plus.length) {
      const olds = minus.flatMap(lineStatements), news = plus.flatMap(lineStatements);
      for (const o of olds) {
        const so = mkStatement(S, o); let best = null;
        for (const n of news) { const sn = mkStatement(S, n); const sim = wj(so, sn); if (!best || sim > best.sim) best = { sim, n, sn }; }
        if (best && best.sim >= 0.5 && best.sim < 1) edits.push({ commit, file, old: o, new: best.n, sim: +best.sim.toFixed(3), num: numberConflict(so, best.sn), slot: slotConflict(so, best.sn), swap: swapConflict(so, best.sn), neg: negationConflict(so, best.sn), ant: antonymConflict(so, best.sn),
          numsOld: so.nums.map(x => x.value + x.unit), numsNew: best.sn.nums.map(x => x.value + x.unit) });
      }
    }
    minus = []; plus = [];
  };
  for (const line of log.split('\n')) {
    if (line.startsWith('@@C ')) { flush(); commit = line.slice(4); continue; }
    if (line.startsWith('+++ b/')) { flush(); file = line.slice(6); continue; }
    if (line.startsWith('--- ') || line.startsWith('diff ') || line.startsWith('index ')) continue;
    if (line.startsWith('@@')) { flush(); continue; }
    if (line.startsWith('-')) minus.push(line.slice(1)); else if (line.startsWith('+')) plus.push(line.slice(1));
  }
  flush();
  fs.writeFileSync(path.resolve(outfile), JSON.stringify(edits, null, 1));
  const total = edits.length, anyNum = edits.filter(e => e.numsOld.join() !== e.numsNew.join()).length;
  console.log({ total, numberConflict: edits.filter(e => e.num.length).length, slot: edits.filter(e => e.slot.length).length, swap: edits.filter(e => e.swap.length).length, anyNumberOrSlot: edits.filter(e => e.num.length || e.slot.length).length, negation: edits.filter(e => e.neg).length, antonym: edits.filter(e => e.ant.length).length, numbersChanged: anyNum });
}

// ---------- наборы для оценки ----------
const fnvStr = str => { let h = 0x811c9dc5; for (let i = 0; i < str.length; i++) { h ^= str.charCodeAt(i); h = Math.imul(h, 0x01000193); } return h >>> 0; };
const NUM_RE = /(?<![\p{L}\p{N}_.\-:/#`])(\d+(?:[.,]\d+)?)(?![\p{N}]|[-:/.]\d|\p{L}?-\d)(\s*)(%|[\p{L}]{1,14})?/gu;
// заменяет первое число с единицей на другое (засеянное расхождение) или переписывает формат (контроль)
function mutate(text, mode) {
  NUM_RE.lastIndex = 0;
  let m;
  while ((m = NUM_RE.exec(text))) {
    const unit = m[3] ? unitOf(m[3].toLowerCase()) : '';
    if (!unit) continue;
    const v = Number(m[1].replace(',', '.'));
    if (mode === 'conflict') {
      const nv = v >= 10 ? v * 2 : v + 3;
      const repl = String(nv).replace('.', m[1].includes(',') ? ',' : '.');
      return text.slice(0, m.index) + repl + m[2] + (m[3] || '') + text.slice(m.index + m[0].length);
    }
    const dot = m[1].includes(',') ? m[1].replace(',', '.') : m[1];
    const unitFull = unit === 'сек' && m[3] === 'с' ? 'секунд' : m[3] || '';
    return text.slice(0, m.index) + dot + m[2] + unitFull + text.slice(m.index + m[0].length);
  }
  return null;
}
if (command === 'evalset') {
  const [corpus, outdir] = args.map(p => path.resolve(p));
  const chunks = readSnapshot(corpus), S = buildStatements(chunks);
  fs.mkdirSync(outdir, { recursive: true });
  // 1. ручные пары
  const hand = JSON.parse(fs.readFileSync(path.join(path.dirname(corpus), 'hand-pairs.json'), 'utf8')).pairs;
  fs.writeFileSync(path.join(outdir, 'hand.in.json'), JSON.stringify(hand.map(({ id, label, kind, a, b }) => ({ id, label, kind, a, b }))));
  // 2. засеянные: утверждения текущего корпуса с числом и единицей
  const pool = S.all.filter(s => scopeOf(chunks[s.ci]) === 'current' && s.tok.length >= 7 && s.tok.length <= 40 && s.nums.some(n => n.unit) && !/[{}<>]|\.(md|rs|json|svelte)\b/.test(s.text))
    .sort((x, y) => fnvStr(x.text) - fnvStr(y.text));
  const seeded = [];
  const take = (mode, offset) => { let n = 0; for (const s of pool.slice(offset)) { const t = mutate(s.text, mode); if (!t || t === s.text) continue; seeded.push({ id: `${mode === 'conflict' ? 'S' : 'C'}${String(n + 1).padStart(2, '0')}`, label: mode === 'conflict' ? 'conflict' : 'compatible', kind: mode === 'conflict' ? 'number-seeded' : 'format-control', a: s.text, b: t, ci: s.ci }); if (++n >= 40) break; } };
  take('conflict', 0); take('control', 200);
  fs.writeFileSync(path.join(outdir, 'seeded.in.json'), JSON.stringify(seeded));
  // 3. настоящие кандидаты корпуса (пары по теме до проверки условий расхождения)
  const pairs = candidatePairs(S, chunks).map(p => { const f = classify(S, [p])[0]; const v = view(S, chunks, f || { ...p, num: [], slot: [], swap: [], neg: false, ant: [] }); return { id: `R${p.i}-${p.j}`, flagged: !!f, kinds: v.kinds, sim: v.sim, a: S.all[p.i].text, b: S.all[p.j].text, pa: v.a.path, pb: v.b.path }; });
  fs.writeFileSync(path.join(outdir, 'real.in.json'), JSON.stringify(pairs));
  console.log({ hand: hand.length, seeded: seeded.length, pool: pool.length, real: pairs.length });
}

// ---------- отчёт: правила и NLI на наборах ----------
if (command === 'report') {
  const [corpus, dir] = args.map(p => path.resolve(p));
  const S = buildStatements(readSnapshot(corpus));
  const load = n => { const f = path.join(dir, n); return fs.existsSync(f) ? JSON.parse(fs.readFileSync(f, 'utf8')) : null; };
  const out = {};
  for (const set of ['hand', 'seeded']) {
    const items = load(`${set}.in.json`), nli = load(`${set}.nli.json`);
    const rows = items.map((p, k) => {
      const a = mkStatement(S, p.a), b = mkStatement(S, p.b);
      const r = { id: p.id, label: p.label, kind: p.kind, sim: +wj(a, b).toFixed(3), num: numberConflict(a, b).length > 0, slot: slotConflict(a, b).length > 0, swap: swapConflict(a, b).length > 0, neg: negationConflict(a, b), ant: antonymConflict(a, b).length > 0 };
      r.rule = r.num || r.slot || r.swap || r.neg || r.ant;
      r.ruleGated = r.rule && r.sim >= 0.4;
      if (nli) { r.pc = +nli.res[k].pc.toFixed(3); }
      return r;
    });
    const sum = (flag) => { const c = rows.filter(r => r.label === 'conflict'), k = rows.filter(r => r.label === 'compatible'); return { recall: `${c.filter(flag).length}/${c.length}`, falsePositive: `${k.filter(flag).length}/${k.length}` }; };
    out[set] = { rules: sum(r => r.rule), rulesGatedSim04: sum(r => r.ruleGated), numberOnly: sum(r => r.num || r.slot || r.swap) };
    if (nli) for (const th of [0.5, 0.8, 0.9]) out[set][`nli>=${th}`] = sum(r => r.pc >= th);
    out[set].rows = rows;
  }
  const real = load('real.in.json'), rn = load('real.nli.json');
  if (real && rn) {
    const rows = real.map((p, k) => ({ id: p.id, flagged: p.flagged, kinds: p.kinds, sim: p.sim, pc: +rn.res[k].pc.toFixed(3), a: p.a, b: p.b, pa: p.pa, pb: p.pb }));
    out.real = { pairs: rows.length, ruleFlagged: rows.filter(r => r.flagged).length, nliMs: rn.ms,
      nliAbove: Object.fromEntries([0.5, 0.8, 0.9, 0.95].map(th => [th, rows.filter(r => r.pc >= th).length])),
      nliAboveAmongRuleFlagged: Object.fromEntries([0.5, 0.8, 0.9].map(th => [th, rows.filter(r => r.flagged && r.pc >= th).length])) };
    fs.writeFileSync(path.join(dir, 'real.rows.json'), JSON.stringify(rows.sort((x, y) => y.pc - x.pc), null, 1));
  }
  fs.writeFileSync(path.join(dir, 'report.json'), JSON.stringify(out, null, 1));
  const { hand, seeded, ...rest } = out;
  console.log(JSON.stringify({ hand: { ...hand, rows: undefined }, seeded: { ...seeded, rows: undefined }, ...rest }, null, 1));
}
