// Шаг 3.8.2 плана (IDEA-13): независимая разметка и пороги дублей на настоящих индексах.
// Только чтение: индекс копируется во временную папку и открывается read-only.
//   pairs <index.sqlite> <out-dir> <name>        — метрики всех кандидатов проекта (повторяет ядро duplicates.rs)
//   sample <out-dir> <seed> <name:n,...>          — стратифицированная выборка для слепой разметки
//   score <out-dir> <labels.json>                 — точность, полнота и пороговые кривые по итоговой разметке
// Тексты чужих проектов остаются в временной папке; в репозиторий идут только пути, строки и метки.
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { DatabaseSync } from 'node:sqlite';

const [command, ...args] = process.argv.slice(2);
const gz = (file, value) => fs.writeFileSync(file, zlib.gzipSync(JSON.stringify(value)));
const ungz = file => JSON.parse(zlib.gunzipSync(fs.readFileSync(file)));

// ---- те же правила, что в crates/bars-core/src/search/duplicates.rs ----
const MIN_WORDS = 15, SHINGLE = 5, LOOSE = 3, TEMPLATE_DF = 4, LOOSE_DF_CAP = 24, MIN_SHARED = 10;
const NEAR = 0.6, CONTAINED = 0.9, PARTIAL = 0.25, SEM_COS = 0.8, SEM_LOOSE = 0.05;
const NEGATIONS = new Set(['не', 'нет', 'нельзя', 'ни', 'без', 'запрещено', 'нигде', 'никогда', 'отсутствует', 'отклонен']);

function ownText(text) {
  const mark = '\nother headers:\n';
  const at = text.lastIndexOf(mark);
  if (at >= 0) {
    const tail = text.slice(at + mark.length);
    if (tail.split('\n').every(l => l.trim() === '' || l.trimStart().startsWith('#'))) text = text.slice(0, at);
  }
  let fenced = false;
  const out = [];
  for (const line of text.split('\n')) {
    const t = line.trimStart();
    if (t.startsWith('```') || t.startsWith('~~~')) fenced = !fenced;
    const hashes = (t.match(/^#+/) || [''])[0].length;
    const heading = !fenced && hashes >= 1 && hashes <= 6 && t[hashes] === ' ';
    if (!heading) out.push(line);
  }
  return out.join('\n');
}
const words = t => (t.toLowerCase().replace(/ё/g, 'е').match(/[\p{L}\p{N}_]+/gu) || []);
const shingles = (w, k) => { const s = new Set(); for (let i = 0; i + k <= w.length; i++) s.add(w.slice(i, i + k).join(' ')); return s; };

function sharedCounts(sets, skip, cap) {
  const postings = new Map();
  sets.forEach((set, i) => { for (const h of set) if (!skip(h)) { let l = postings.get(h); if (!l) postings.set(h, l = []); l.push(i); } });
  const counts = new Map();
  for (const l of postings.values()) {
    if (l.length < 2 || l.length > cap) continue;
    for (let p = 0; p < l.length; p++) for (let q = p + 1; q < l.length; q++) {
      const key = l[p] * 100000 + l[q]; counts.set(key, (counts.get(key) || 0) + 1);
    }
  }
  return counts;
}

if (command === 'pairs') {
  const [dbFile, outDir, name] = args;
  fs.mkdirSync(outDir, { recursive: true });
  const copy = path.join(outDir, `${name}-index.sqlite`);
  fs.copyFileSync(dbFile, copy);
  const db = new DatabaseSync(copy, { readOnly: true });
  const rows = db.prepare(`SELECT c.id, d.path, d.module, c.heading_chain, c.first_line, c.last_line, c.text, c.embed_hash
    FROM chunks c JOIN documents d ON d.id = c.document_id ORDER BY d.path, c.ordinal`).all();
  const vec = db.prepare('SELECT vector FROM vectors WHERE embed_hash = ?');
  const items = []; let skipped = 0;
  for (const r of rows) {
    const body = ownText(r.text), w = words(body);
    if (w.length < MIN_WORDS) { skipped++; continue; }
    items.push({ path: r.path, module: r.module, heading: r.heading_chain, first: Number(r.first_line), last: Number(r.last_line),
      text: body.trim(), w, embed: r.embed_hash, s5: shingles(w, SHINGLE), s3: shingles(w, LOOSE) });
  }
  const n = items.length;
  const df5 = new Map(), df3 = new Map();
  for (const it of items) { for (const h of it.s5) df5.set(h, (df5.get(h) || 0) + 1); for (const h of it.s3) df3.set(h, (df3.get(h) || 0) + 1); }
  // точные группы
  const byText = new Map();
  items.forEach((it, i) => { const k = it.w.join(' '); (byText.get(k) || byText.set(k, []).get(k)).push(i); });
  const groupOf = new Map(); let g = 0;
  const exactGroups = [...byText.values()].filter(l => l.length > 1);
  for (const l of exactGroups) { l.forEach(i => groupOf.set(i, g)); g++; }
  const sameGroup = (a, b) => groupOf.has(a) && groupOf.get(a) === groupOf.get(b);
  const strict = sharedCounts(items.map(i => i.s5), h => df5.get(h) >= TEMPLATE_DF, Infinity);
  const raw = sharedCounts(items.map(i => i.s5), () => false, Infinity);
  const loose = sharedCounts(items.map(i => i.s3), h => df3.get(h) >= TEMPLATE_DF, LOOSE_DF_CAP);
  // векторы
  const dim = 1024;
  const V = new Float32Array(n * dim); const has = new Uint8Array(n);
  items.forEach((it, i) => {
    const row = vec.get(it.embed); if (!row) return;
    const b = row.vector; const f = new Float32Array(b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength));
    if (f.length !== dim) throw new Error(`dim ${f.length}`);
    let s = 0; for (const x of f) s += x * x; s = Math.sqrt(s) || 1;
    for (let k = 0; k < dim; k++) V[i * dim + k] = f[k] / s; has[i] = 1;
  });
  const cos = (a, b) => { let s = 0; const oa = a * dim, ob = b * dim; for (let k = 0; k < dim; k++) s += V[oa + k] * V[ob + k]; return s; };
  const pairs = [];
  const seen = new Set();
  const add = (a, b) => {
    const key = a * 100000 + b; if (seen.has(key)) return; seen.add(key);
    const s5 = strict.get(key) || 0, r5 = raw.get(key) || 0, s3 = loose.get(key) || 0;
    const l5 = items[a].s5.size, r5n = items[b].s5.size, m5 = Math.min(l5, r5n);
    const c5 = s5 / m5, j5 = s5 / (l5 + r5n - s5), c5raw = r5 / m5;
    const m3 = Math.min(items[a].s3.size, items[b].s3.size), c3 = s3 / m3;
    const c = has[a] && has[b] ? cos(a, b) : null;
    pairs.push({ a, b, shared5: s5, c5: +c5.toFixed(4), j5: +j5.toFixed(4), c5raw: +c5raw.toFixed(4), c3: +c3.toFixed(4), cos: c === null ? null : +c.toFixed(4),
      exact: sameGroup(a, b) });
  };
  for (const key of strict.keys()) { const a = Math.floor(key / 100000), b = key % 100000; if ((strict.get(key) >= 3)) add(a, b); }
  for (const key of raw.keys()) { const a = Math.floor(key / 100000), b = key % 100000; if (raw.get(key) >= MIN_SHARED) add(a, b); }
  for (let a = 0; a < n; a++) if (has[a]) for (let b = a + 1; b < n; b++) if (has[b] && cos(a, b) >= 0.72) add(a, b);
  // класс по правилам продукта
  for (const p of pairs) {
    let kind = null;
    if (p.exact) kind = 'exact';
    else {
      if (p.shared5 >= MIN_SHARED) kind = p.j5 >= NEAR ? 'near' : p.c5 >= CONTAINED ? 'contained' : p.c5 >= PARTIAL ? 'partial' : null;
      if (!kind && p.cos !== null && p.cos >= SEM_COS && p.c3 >= SEM_LOOSE && p.c5 < PARTIAL) kind = 'semantic';
    }
    p.kind = kind;
  }
  const meta = items.map(({ path, module, heading, first, last, w }) => ({ path, module, heading, first, last, words: w.length }));
  const counts = {}; for (const p of pairs) if (p.kind) counts[p.kind] = (counts[p.kind] || 0) + 1;
  counts.exactGroups = exactGroups.length;
  gz(path.join(outDir, `${name}-pairs.json.gz`), { name, chunks: n, skipped, meta, pairs, counts });
  gz(path.join(outDir, `${name}-texts.json.gz`), items.map(i => i.text));
  db.close();
  fs.rmSync(copy);
  console.log(JSON.stringify({ name, chunks: n, skipped, pairs: pairs.length, counts }));
} else if (command === 'sample') {
  // стратифицированная выборка: см. STRATA
  const [outDir, seedArg, spec] = args;
  let seed = Number(seedArg) >>> 0;
  const rnd = () => { seed = (seed + 0x6D2B79F5) >>> 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
  const shuffle = a => { for (let i = a.length - 1; i > 0; i--) { const j = Math.floor(rnd() * (i + 1)); [a[i], a[j]] = [a[j], a[i]]; } return a; };
  const STRATA = [
    ['exact', p => p.kind === 'exact'],
    ['near', p => p.kind === 'near'],
    ['contained', p => p.kind === 'contained'],
    ['partial_hi', p => p.kind === 'partial' && p.c5 >= 0.5],
    ['partial_lo', p => p.kind === 'partial' && p.c5 < 0.5],
    ['semantic_hi', p => p.kind === 'semantic' && p.cos >= 0.88],
    ['semantic_mid', p => p.kind === 'semantic' && p.cos >= 0.84 && p.cos < 0.88],
    ['semantic_lo', p => p.kind === 'semantic' && p.cos < 0.84],
    ['miss_near', p => !p.kind && p.shared5 >= MIN_SHARED && p.j5 >= 0.4 && p.j5 < NEAR],
    ['miss_partial', p => !p.kind && p.shared5 >= 5 && p.c5 >= 0.12 && p.c5 < PARTIAL],
    ['miss_semantic_gate', p => !p.kind && p.cos !== null && p.cos >= SEM_COS && p.c3 < SEM_LOOSE && p.c5 < PARTIAL],
    ['miss_semantic_cos', p => !p.kind && p.cos !== null && p.cos >= 0.74 && p.cos < SEM_COS && p.c3 >= SEM_LOOSE && p.c5 < PARTIAL],
    ['template', p => !p.kind && p.c5raw >= PARTIAL && p.c5 < PARTIAL],
  ];
  const quota = {}; for (const part of spec.split(',')) { const [k, v] = part.split(':'); quota[k] = Number(v); }
  const sample = [];
  const universe = {};
  for (const [project, wanted] of Object.entries(quota)) {
    const data = ungz(path.join(outDir, `${project}-pairs.json.gz`));
    const texts = ungz(path.join(outDir, `${project}-texts.json.gz`));
    const strataPairs = STRATA.map(([name, f]) => [name, data.pairs.filter(f)]);
    universe[project] = Object.fromEntries(strataPairs.map(([n, l]) => [n, l.length]));
    // доли по стратам: минимум 4 на страту, остальное поровну между непустыми
    const nonEmpty = strataPairs.filter(([, l]) => l.length);
    const each = Math.max(4, Math.floor(wanted / nonEmpty.length));
    for (const [name, list] of nonEmpty) {
      const pick = shuffle([...list]).slice(0, Math.min(each, list.length));
      for (const p of pick) sample.push({ project, stratum: name, stratumSize: list.length, taken: pick.length, p, data, texts });
    }
  }
  shuffle(sample);
  const pub = [], key = [];
  sample.forEach((s, i) => {
    const id = `P${String(i + 1).padStart(3, '0')}`;
    let { a, b } = s.p; if (rnd() < 0.5) [a, b] = [b, a];
    const side = x => ({ path: s.data.meta[x].path, heading: s.data.meta[x].heading, lines: `${s.data.meta[x].first}-${s.data.meta[x].last}`, text: s.texts[x] });
    pub.push({ id, project: s.project, first: side(a), second: side(b) });
    key.push({ id, project: s.project, stratum: s.stratum, stratumSize: s.stratumSize, taken: s.taken, kind: s.p.kind, a, b,
      first: { path: s.data.meta[a].path, lines: `${s.data.meta[a].first}-${s.data.meta[a].last}` },
      second: { path: s.data.meta[b].path, lines: `${s.data.meta[b].first}-${s.data.meta[b].last}` },
      c5: s.p.c5, j5: s.p.j5, c5raw: s.p.c5raw, c3: s.p.c3, cos: s.p.cos, shared5: s.p.shared5 });
  });
  const CUT = 1000, BATCH = 30;
  const show = s => `**${s.path}** (строки ${s.lines}) — ${s.heading}\n\n${s.text.length > CUT ? s.text.slice(0, CUT) + ` …[обрезано: показано ${CUT} из ${s.text.length} знаков]` : s.text}`;
  for (let b = 0; b * BATCH < pub.length; b++) {
    const part = pub.slice(b * BATCH, (b + 1) * BATCH).map(p => `## ${p.id}\n\n### Раздел A\n\n${show(p.first)}\n\n### Раздел B\n\n${show(p.second)}\n`);
    fs.writeFileSync(path.join(outDir, `batch-${b + 1}.md`), part.join('\n---\n\n'));
  }
  fs.writeFileSync(path.join(outDir, 'sample-blind.json'), JSON.stringify(pub, null, 1));
  fs.writeFileSync(path.join(outDir, 'sample-key.json'), JSON.stringify({ universe, key }, null, 1));
  console.log(JSON.stringify({ pairs: key.length, universe }));
} else if (command === 'agree') {
  // agree <labels-dir> <A-prefix> <B-prefix>: согласие двух разметчиков (доля и κ Коэна) по трём полям
  const [dir, pa, pb] = args;
  const load = p => Object.fromEntries([1, 2, 3, 4].flatMap(n => JSON.parse(fs.readFileSync(path.join(dir, `labels-${p}-${n}.json`), 'utf8'))).map(x => [x.id, x]));
  const A = load(pa), B = load(pb);
  const ids = Object.keys(A).filter(id => B[id]);
  const out = { pairs: ids.length };
  for (const field of ['verdict', 'form', 'coverage']) {
    const cats = [...new Set(ids.flatMap(id => [A[id][field], B[id][field]]))];
    const agree = ids.filter(id => A[id][field] === B[id][field]).length / ids.length;
    const pe = cats.reduce((s, c) => s + (ids.filter(id => A[id][field] === c).length / ids.length) * (ids.filter(id => B[id][field] === c).length / ids.length), 0);
    out[field] = { agree: +agree.toFixed(3), kappa: +((agree - pe) / (1 - pe)).toFixed(3),
      matrix: Object.fromEntries(cats.map(c => [c, Object.fromEntries(cats.map(d => [d, ids.filter(id => A[id][field] === c && B[id][field] === d).length]))])) };
  }
  out.disagree = ids.filter(id => A[id].verdict !== B[id].verdict);
  console.log(JSON.stringify(out, null, 1));
} else if (command === 'score') {
  // score <out-dir> <labels-final.json>: точность видов, оценка полноты по весам страт, пороговые кривые
  const [outDir, finalFile] = args;
  const { universe, key } = JSON.parse(fs.readFileSync(path.join(outDir, 'sample-key.json'), 'utf8'));
  const lab = Object.fromEntries(JSON.parse(fs.readFileSync(finalFile, 'utf8')).map(x => [x.id, x]));
  const rows = key.map(k => ({ ...k, ...lab[k.id], w: k.stratumSize / k.taken, dup: lab[k.id].verdict === 'dup' }));
  const wilson = (x, n) => { if (!n) return [null, null]; const z = 1.96, p = x / n, d = 1 + z * z / n, c = p + z * z / (2 * n), m = z * Math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)); return [+((c - m) / d).toFixed(2), +((c + m) / d).toFixed(2)]; };
  const byStratum = {};
  for (const r of rows) {
    const s = byStratum[`${r.project}/${r.stratum}`] ||= { n: 0, dup: 0, legit: 0, template: 0, different: 0, size: r.stratumSize };
    s.n++; s[r.verdict]++;
  }
  const kinds = {};
  for (const r of rows) {
    const k = r.kind || 'нет_вида';
    const s = kinds[k] ||= { n: 0, dup: 0, legit: 0, template: 0, different: 0, wDup: 0, wAll: 0 };
    s.n++; s[r.verdict]++; s.wAll += r.w; if (r.dup) s.wDup += r.w;
  }
  for (const s of Object.values(kinds)) { s.precision = +(s.dup / s.n).toFixed(2); s.ci = wilson(s.dup, s.n); s.estDupPairs = +s.wDup.toFixed(0); s.estPairs = +s.wAll.toFixed(0); }
  // полнота: оценка числа настоящих дублей по всему охвату выборки и доля из них, которую находят правила продукта
  const totalDup = rows.reduce((s, r) => s + (r.dup ? r.w : 0), 0);
  const foundDup = rows.reduce((s, r) => s + (r.dup && r.kind ? r.w : 0), 0);
  const sameFile = r => r.first.path === r.second.path;
  const ovl = r => r.verdict === 'dup' || r.verdict === 'legit';
  const stat = (pred, truth) => {
    let tp = 0, fp = 0, fn = 0, nTp = 0, nFp = 0;
    for (const r of rows) { const y = pred(r), t = truth(r); if (y && t) { tp += r.w; nTp++; } else if (y) { fp += r.w; nFp++; } else if (t) fn += r.w; }
    return { precision: +(tp / (tp + fp || 1)).toFixed(2), recall: +(tp / (tp + fn || 1)).toFixed(2), sampleTp: nTp, sampleFp: nFp, ci: wilson(nTp, nTp + nFp) };
  };
  const sweep = (name, pred) => ({ rule: name, predicted: Math.round(rows.reduce((s, r) => s + (pred(r) ? r.w : 0), 0)), dup: stat(pred, r => r.dup), overlap: stat(pred, ovl) });
  const rules = [];
  const sem = (t, gate, nosame) => r => r.cos !== null && r.cos >= t && r.c5 < 0.25 && (gate === null || r.c3 >= gate) && !(nosame && sameFile(r));
  for (const t of [0.74, 0.78, 0.8, 0.82, 0.84, 0.86, 0.88, 0.9]) {
    rules.push(sweep(`смысловой: cos ≥ ${t}, c3 ≥ 0,05 (как в продукте)`, sem(t, 0.05, false)));
    rules.push(sweep(`смысловой: cos ≥ ${t}, c3 ≥ 0,05, без пар внутри одного файла`, sem(t, 0.05, true)));
    rules.push(sweep(`смысловой: cos ≥ ${t}, без лексического шлюза`, sem(t, null, false)));
  }
  for (const g of [0.1, 0.15, 0.2]) rules.push(sweep(`смысловой: cos ≥ 0,8, c3 ≥ ${g}, без пар внутри одного файла`, sem(0.8, g, true)));
  for (const t of [0.1, 0.15, 0.2, 0.25, 0.3, 0.4, 0.5, 0.6]) rules.push(sweep(`частичный: c5 ≥ ${t}, 10 общих окон`, r => r.shared5 >= 10 && r.c5 >= t));
  for (const t of [0.12, 0.15, 0.25]) rules.push(sweep(`частичный: c5 ≥ ${t}, 5 общих окон`, r => r.shared5 >= 5 && r.c5 >= t));
  rules.push(sweep('частичный: c5 ≥ 0,25 без пар внутри одного файла', r => r.shared5 >= 10 && r.c5 >= 0.25 && !sameFile(r)));
  const sameFileStats = { n: rows.filter(sameFile).length, dup: rows.filter(r => sameFile(r) && r.dup).length, legit: rows.filter(r => sameFile(r) && r.verdict === 'legit').length };
  console.log(JSON.stringify({ n: rows.length, universe, byStratum, kinds, sameFileStats, estimated: { totalDupPairs: Math.round(totalDup), foundByProduct: Math.round(foundDup), recall: +(foundDup / totalDup).toFixed(2) }, rules }, null, 1));
} else {
  console.error('commands: pairs | sample | agree | score');
  process.exit(2);
}
