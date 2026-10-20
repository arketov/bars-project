// Шаг 16 плана (IDEA-13): исследование способов различать повторы. Только чтение корпуса и временный проект.
// make <snapshot-dir> <new-project-dir>   — размеченный набор «разделы + копии разных видов»
// export <project-root> <new-output-dir>  — тексты и векторы чанков временного проекта (read-only SQLite)
// analyze <bench-export-dir> <corpus-snapshot-dir> <new-output-dir> — метрики, пороги, ошибки, задержка
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import assert from 'node:assert/strict';
import { DatabaseSync } from 'node:sqlite';

const [command, ...args] = process.argv.slice(2);
const readSnapshot = dir => JSON.parse(zlib.gunzipSync(fs.readFileSync(path.join(dir, 'snapshot.json.gz'))));
const readVectors = dir => {
  const b = zlib.gunzipSync(fs.readFileSync(path.join(dir, 'vectors.bin.gz')));
  const n = b.readUInt32LE(4), d = b.readUInt32LE(8);
  const f = new Float32Array(b.buffer.slice(b.byteOffset + 12, b.byteOffset + 12 + n * d * 4));
  const out = [];
  for (let i = 0; i < n; i++) {
    const v = f.subarray(i * d, (i + 1) * d);
    let s = 0; for (const x of v) s += x * x; s = Math.sqrt(s);
    out.push(v.map(x => x / s));
  }
  return out;
};
const ownText = t => t.replace(/\nother headers:[\s\S]*$/, '').trim();
const tokens = t => (t.toLowerCase().replace(/ё/g, 'е').match(/[\p{L}\p{N}_]+/gu) || []);
const dot = (a, b) => { let s = 0; for (let i = 0; i < a.length; i++) s += a[i] * b[i]; return s; };

// ---------- make ----------
const PARAPHRASE = {
  1: 'Решение принято 15-10-2026. Раньше документация bars делилась по частям приложения: `indexing`, `storage`, `integrations`, `design`. Автору это неудобно: он привык читать ТЗ по фичам, как в Gramax. Причины решений и ход работы оставались в PLAN.md, коммитах и QUESTIONS.md, а неудачные попытки не фиксировались вовсе. Кроме того, автор хочет, чтобы bars рекомендовал такую раскладку другим проектам, и сначала проверить её на самом bars.',
  6: 'Секреты хранит крейт `keyring-core` (семейство `keyring` 4.x): `windows-native-keyring-store` на Windows и `apple-native-keyring-store` для Keychain на macOS. Запись создаётся с сервисом `bars` и пользователем из `api_key_name` в настройках. Обращение к хранилищу происходит лишь при первом чтении ключа (`OsSecrets`), так что воркер без внешних провайдеров его не открывает. На прочих системах bars ключи не держит — остаётся только `api_key_env`. Ключ перечитывается при каждом запросе к провайдеру.',
  11: 'Команда `bars-cli hook session-start` принимает от клиента JSON через stdin и достаёт из него `cwd` — рабочую папку сессии; формат одинаков в Claude Code и Codex. По `cwd` ищется корень проекта; если проект не зарегистрирован, но в нём есть документация, он регистрируется и ставится в очередь проверки актуальности (Т-6, Т-10). Без `cwd` берётся текущая папка, без `source` — `startup`. Поиск корня и регистрацию выполняет воркер (метод `session_start`), не CLI. CLI не ждёт индексации и сразу завершается, чтобы не тормозить запуск сессии; ошибки уходят в лог воркера, код выхода всегда `0`, ведь сбой bars не должен ломать сессию агента.',
  16: 'Автор решил 15-10-2026 (OQ-17): установщик под Windows собирает Tauri bundler, подпись не ставится; новая версия заменяет установленную сама, ручные действия не нужны. Формат — NSIS (причины отказа от MSI описаны в knowledge/installer.md); решения агента записаны в OQ-64.',
  26: 'Порядок работ по GGUF. Сначала изолированный Rust-спайк с зафиксированными версиями wrapper и backend, собранный рядом с bars-core/ORT. Затем проверка размерности, нормы, повторяемости и независимости текстов на CPU и Vulkan. Потом сравнение Qwen с bge-m3 на замороженном корпусе этапа 9: отдельный индекс, полный retrieval, MRR/Hit, задержка и память отдельного процесса; векторы разных моделей в одном индексе не сравнивать. Nomic v2 MoE проверить на llama.cpp, а поддержку Т-17 сохранить, пока замена не доказана. После успешного спайка — ADR, каталог и скачивание, loader, fingerprint, устройства, регрессии и документация.',
  31: 'Состояние на 18-10-2026: сделано и проверено в рабочей копии. Границы удаляемых данных подтвердил автор (OQ-83). Относится к требованиям Т-5, Т-6, Т-19. Прежняя отмена регистрации остаётся для CLI и переноса проекта. Установленное приложение не обновляли; приёмка в нативном Tauri и на macOS — отдельная задача.',
  36: 'Автор видит, что проценты растут сверху вниз, и порядок кажется ему случайным (ISS-02). Решение автора OQ-77 внедрено 18-10-2026; автотесты и проверка во встроенном браузере пройдены. Затронут обычный поиск и выдача без rerank при его сбое; окно, MCP и CLI обязаны отдавать единый порядок. Ускорение моделей и умолчания C/K — другие задачи. ISS-11 убирает английские подписи, а не переводит поиск на английский (НФТ-4).',
};
const CLASSES = {
  exact: [0, 2, 3, 4, 5], format: [7, 8, 9, 10, 12], partial: [13, 14, 15, 17, 18],
  near: [19, 20, 21, 22, 23], number: [24, 25, 27, 28, 29], negation: [30, 32, 33, 34, 35],
  paraphrase: [1, 6, 11, 16, 26, 31, 36], contained: [37, 38, 39],
};
const BOILER = 'Ограничения проверки. Результат получен на одной машине разработчика и не проверялся на других устройствах. Это повод для повторной проверки, а не доказанный дефект. Перед исправлением воспроизведите замер на своей машине и сохраните сырые данные рядом с выводом. Если числа расходятся с этим описанием более чем на десять процентов, запишите расхождение в журнал сессии.';
const sentences = t => t.split(/(?<=[.!?])\s+/).filter(Boolean);

function variant(cls, i, src, picks) {
  const t = ownText(src.text);
  const body = t.replace(/^#+ .*$/gm, '').trim();
  switch (cls) {
    case 'exact': return body;
    case 'format': return body.toLowerCase().replace(/\*\*/g, '').replace(/[«»]/g, '"').replace(/\s+/g, ' ');
    case 'near': {
      const w = body.split(' ');
      [Math.floor(w.length * 0.2), Math.floor(w.length * 0.5), Math.floor(w.length * 0.8)].forEach(k => { w[k] = 'какой-то'; });
      return w.join(' ');
    }
    case 'number': {
      let done = false;
      return body.replace(/\b\d+\b/g, m => { if (done) return m; done = true; return String(Number(m) * 2 + 1); });
    }
    case 'negation': {
      let done = false;
      return body.replace(/(^|[^\p{L}])не(?=[^\p{L}])/u, (m, p) => { done = true; return p; }).replace(/ {2}/g, ' ');
    }
    case 'partial': {
      const s = sentences(body);
      const half = s.slice(Math.ceil(s.length / 2));
      const intro = sentences(ownText(picks[60 + (i % 8)].text).replace(/^#+ .*$/gm, ''))[0] || '';
      const outro = sentences(ownText(picks[50 + (i % 8)].text).replace(/^#+ .*$/gm, '')).slice(-1)[0] || '';
      return [intro, ...half, outro].join(' ');
    }
    case 'contained': {
      const a = ownText(picks[55 + (i % 3)].text).replace(/^#+ .*$/gm, '').trim();
      const b = ownText(picks[65 + (i % 3)].text).replace(/^#+ .*$/gm, '').trim();
      return [a, body, b].join('\n\n');
    }
    case 'paraphrase': return PARAPHRASE[i];
  }
}

if (command === 'make') {
  assert.equal(args.length, 2);
  const [snapDir, out] = args.map(p => path.resolve(p));
  assert(!fs.existsSync(out), 'output must be new');
  const snap = readSnapshot(snapDir);
  const c = snap.chunks.filter(x => x.text.length > 450 && x.text.length < 1100 && (x.text.match(/\|/g) || []).length < 6 &&
    /[0-9]/.test(x.text) && /(^|[^а-я])не([^а-я]|$)/i.test(x.text) && !x.path.includes('/chats/'));
  const seen = new Set(), picks = [];
  for (const x of c) { if (seen.has(x.path)) continue; seen.add(x.path); picks.push(x); }
  assert(picks.length >= 71);
  fs.mkdirSync(path.join(out, 'docs', 'orig'), { recursive: true });
  fs.mkdirSync(path.join(out, 'docs', 'var'), { recursive: true });
  const put = (rel, title, heading, body) => fs.writeFileSync(path.join(out, 'docs', rel),
    `---\ntitle: ${title}\n---\n\n# ${title}\n\n## ${heading}\n\n${body}\n`);
  const labels = [];
  for (let i = 0; i < 40; i++) {
    const text = ownText(picks[i].text).replace(/^#+ .*$/gm, '').trim();
    put(`orig/o${String(i).padStart(2, '0')}.md`, `Оригинал ${i}`, `Раздел оригинала ${i}`, text);
    labels.push({ file: `orig/o${String(i).padStart(2, '0')}.md`, group: `g${i}`, role: 'orig', cls: 'orig', src: picks[i].path });
  }
  for (const [cls, ids] of Object.entries(CLASSES)) {
    for (const i of ids) {
      const body = variant(cls, i, picks[i], picks);
      const f = `var/${cls}-${String(i).padStart(2, '0')}.md`;
      put(f, `Копия ${cls} ${i}`, `Раздел копии ${cls} ${i}`, body);
      labels.push({ file: f, group: `g${i}`, role: 'copy', cls });
    }
  }
  for (let k = 0; k < 8; k++) {
    const body = ownText(picks[40 + k].text).replace(/^#+ .*$/gm, '').trim();
    const f = `var/template-body-${k}.md`;
    put(f, `Шаблон с телом ${k}`, `Раздел шаблона ${k}`, `${BOILER}\n\n${body}`);
    labels.push({ file: f, group: `tb${k}`, role: 'template-body', cls: 'template-body', template: 'T' });
  }
  for (let k = 0; k < 6; k++) {
    const f = `var/boiler-${k}.md`;
    put(f, `Шаблон ${k}`, `Раздел только шаблона ${k}`, BOILER);
    labels.push({ file: f, group: 'boiler', role: 'boiler', cls: 'boiler', template: 'B' });
  }
  fs.writeFileSync(path.join(out, 'labels.json'), JSON.stringify({ boiler: BOILER, labels }, null, 2));
  console.log('files', labels.length);
}

// ---------- export ----------
if (command === 'export') {
  assert.equal(args.length, 2);
  const [root, out] = args.map(p => path.resolve(p));
  assert(!fs.existsSync(out));
  const db = new DatabaseSync(path.join(root, '.bars', 'index.sqlite'), { readOnly: true });
  const rows = db.prepare(`SELECT d.path,d.module,d.title,c.ordinal,c.first_line,c.last_line,c.heading_chain,c.text,v.vector
    FROM chunks c JOIN documents d ON d.id=c.document_id LEFT JOIN vectors v ON v.embed_hash=c.embed_hash
    ORDER BY d.path,c.ordinal`).all();
  db.close();
  const dim = rows[0].vector.byteLength / 4;
  const header = Buffer.alloc(12); header.write('BGV1'); header.writeUInt32LE(rows.length, 4); header.writeUInt32LE(dim, 8);
  fs.mkdirSync(out, { recursive: true });
  const chunks = rows.map((r, index) => { const { vector, ...rest } = r; return { index, ...rest }; });
  fs.writeFileSync(path.join(out, 'snapshot.json.gz'), zlib.gzipSync(JSON.stringify({ format: 1, chunks })));
  fs.writeFileSync(path.join(out, 'vectors.bin.gz'), zlib.gzipSync(Buffer.concat([header, ...rows.map(r => Buffer.from(r.vector))])));
  console.log('chunks', rows.length, 'dim', dim);
}


// ---------- analyze ----------
const fnv = str => { let h = 0x811c9dc5; for (let i = 0; i < str.length; i++) { h ^= str.charCodeAt(i); h = Math.imul(h, 0x01000193); } return h >>> 0; };
const shingles = (tok, k) => { const out = new Set(); for (let i = 0; i + k <= tok.length; i++) out.add(fnv(tok.slice(i, i + k).join(' '))); if (!out.size && tok.length) out.add(fnv(tok.join(' '))); return out; };
const winnow = (tok, k, w) => { // Schleimer 2003: минимальный хеш в окне w, правый при равенстве
  const h = []; for (let i = 0; i + k <= tok.length; i++) h.push(fnv(tok.slice(i, i + k).join(' ')));
  const out = new Set(); if (h.length <= w) { h.forEach(x => out.add(x)); return out; }
  for (let i = 0; i + w <= h.length; i++) { let m = i; for (let j = i; j < i + w; j++) if (h[j] <= h[m]) m = j; out.add(h[m]); }
  return out;
};
const inter = (a, b) => { let n = 0; const [s, l] = a.size < b.size ? [a, b] : [b, a]; for (const x of s) if (l.has(x)) n++; return n; };
const NEG = /^(не|нет|нельзя|ни|без|запрещено|нигде|никогда|отсутствует|отклонён|отклонен)$/;
const numbers = tok => tok.filter(t => /\d/.test(t)).sort().join(',');
const negs = tok => tok.filter(t => NEG.test(t)).length;
const bodyOf = t => ownText(t).replace(/^#+ .*$/gm, '').trim();

function metrics(items, vecs, a, b) {
  const A = items[a], B = items[b];
  const eq = A.norm === B.norm;
  const k5 = inter(A.s5, B.s5), k3 = inter(A.s3, B.s3), kw = inter(A.w, B.w);
  const m = Math.min(A.s5.size, B.s5.size);
  return {
    a, b, cos: dot(vecs[a], vecs[b]), exact: eq,
    c5: m ? k5 / m : 0, j5: k5 / (A.s5.size + B.s5.size - k5 || 1),
    c3: Math.min(A.s3.size, B.s3.size) ? k3 / Math.min(A.s3.size, B.s3.size) : 0,
    cw: Math.min(A.w.size, B.w.size) ? kw / Math.min(A.w.size, B.w.size) : 0,
    numDiff: numbers(A.tok) !== numbers(B.tok), negDiff: negs(A.tok) !== negs(B.tok),
  };
}
function prepare(chunks) {
  return chunks.map(c => { const tok = tokens(bodyOf(c.text)); return { tok, norm: tok.join(' '), s5: shingles(tok, 5), s3: shingles(tok, 3), w: winnow(tok, 5, 4) }; });
}
const stat = xs => { const s = [...xs].sort((x, y) => x - y); const q = p => s[Math.min(s.length - 1, Math.floor(p * s.length))]; return { n: s.length, min: s[0], p50: q(.5), p95: q(.95), max: s[s.length - 1] }; };

if (command === 'analyze') {
  assert.equal(args.length, 3);
  const [benchDir, corpusDir, out] = args.map(p => path.resolve(p));
  assert(!fs.existsSync(out)); fs.mkdirSync(out, { recursive: true });
  const labelFile = JSON.parse(fs.readFileSync(path.join(path.dirname(benchDir), 'labels.json'), 'utf8')).labels;
  const bench = readSnapshot(benchDir).chunks, bv = readVectors(benchDir);
  const lab = new Map(labelFile.map(l => [`docs/${l.file}`, l]));
  const bi = prepare(bench);
  const rows = [];
  for (let a = 0; a < bench.length; a++) for (let b = a + 1; b < bench.length; b++) {
    const la = lab.get(bench[a].path), lb = lab.get(bench[b].path);
    const m = metrics(bi, bv, a, b);
    let truth = 'negative';
    if (la.group === lb.group && la.group !== 'boiler') truth = [la.cls, lb.cls].find(c => c !== 'orig');
    else if (la.group === 'boiler' && lb.group === 'boiler') truth = 'boiler';
    else if (la.template === 'T' && lb.template === 'T') truth = 'template-pair';
    else if ((la.template && lb.template)) truth = 'template-pair';
    rows.push({ ...m, truth, fa: bench[a].path, fb: bench[b].path });
  }
  const byTruth = {};
  for (const r of rows) (byTruth[r.truth] ||= []).push(r);
  const summary = {};
  for (const [t, rs] of Object.entries(byTruth)) summary[t] = {
    n: rs.length, cos: stat(rs.map(r => r.cos)), c5: stat(rs.map(r => r.c5)), j5: stat(rs.map(r => r.j5)),
    c3: stat(rs.map(r => r.c3)), cw: stat(rs.map(r => r.cw)),
    exact: rs.filter(r => r.exact).length, numDiff: rs.filter(r => r.numDiff).length, negDiff: rs.filter(r => r.negDiff).length,
  };
  // реальный корпус: ложные срабатывания
  const corpus = readSnapshot(corpusDir).chunks, cv = readVectors(corpusDir);
  const ci = prepare(corpus);
  const t0 = performance.now();
  const real = [];
  for (let a = 0; a < corpus.length; a++) for (let b = a + 1; b < corpus.length; b++) {
    const m = metrics(ci, cv, a, b);
    if (m.exact || m.c5 >= 0.1 || m.cos >= 0.85) real.push({ ...m, fa: `${corpus[a].path}:${corpus[a].first_line}`, fb: `${corpus[b].path}:${corpus[b].first_line}`, la: ci[a].tok.length, lb: ci[b].tok.length });
  }
  const tAll = performance.now() - t0;
  real.sort((x, y) => y.c5 - x.c5 || y.cos - x.cos);
  fs.writeFileSync(path.join(out, 'bench-pairs.json'), JSON.stringify(rows.map(r => ({ ...r, cos: +r.cos.toFixed(4), c5: +r.c5.toFixed(3), j5: +r.j5.toFixed(3), c3: +r.c3.toFixed(3), cw: +r.cw.toFixed(3) }))));
  fs.writeFileSync(path.join(out, 'summary.json'), JSON.stringify(summary, null, 2));
  fs.writeFileSync(path.join(out, 'corpus-candidates.json'), JSON.stringify(real.slice(0, 200).map(r => ({ ...r, cos: +r.cos.toFixed(4), c5: +r.c5.toFixed(3), j5: +r.j5.toFixed(3), c3: +r.c3.toFixed(3), cw: +r.cw.toFixed(3) })), null, 2));
  console.log(JSON.stringify(summary, (k, v) => typeof v === 'number' ? +v.toFixed(3) : v, 1));

  console.log('corpus', corpus.length, 'candidates', real.length, 'allpairs ms', tAll.toFixed(0));

  // ---- перебор порогов на размеченном наборе ----
  // пары двух «частичных»/«содержащих» копий делят куски текста из-за пересечения выбранных источников (см. knowledge): это не отрицательные
  for (const r of rows) {
    const la = lab.get(r.fa), lb = lab.get(r.fb);
    if (r.truth === 'negative' && ['partial', 'contained'].includes(la.cls) && ['partial', 'contained'].includes(lb.cls)) r.truth = 'shared-source';
  }
  const dfCap = 4;
  const df = new Map();
  for (const it of bi) for (const h of it.s5) df.set(h, (df.get(h) || 0) + 1);
  const sweep = { near: [], contained: [], partial: [], semantic: [], template: {} };
  const eligible = rows.filter(r => r.truth !== 'shared-source');
  for (const th of [0.4, 0.5, 0.55, 0.6, 0.65, 0.7, 0.8]) {
    const hit = eligible.filter(r => r.j5 >= th);
    sweep.near.push({ th, tp: hit.filter(r => ['exact', 'format', 'near', 'number', 'negation'].includes(r.truth)).length, of: 25, // exact + format + near + number + negation по 5
      fpByTruth: Object.fromEntries(Object.entries(hit.reduce((a, r) => { if (!['exact', 'format', 'near', 'number', 'negation'].includes(r.truth)) a[r.truth] = (a[r.truth] || 0) + 1; return a; }, {}))) });
  }
  for (const th of [0.2, 0.25, 0.3, 0.4, 0.5]) {
    const hit = eligible.filter(r => r.c5 >= th && r.j5 < 0.6);
    sweep.partial.push({ th, tpPartialContained: hit.filter(r => ['partial', 'contained'].includes(r.truth)).length, of: 8,
      other: Object.fromEntries(Object.entries(hit.reduce((a, r) => { if (!['partial', 'contained'].includes(r.truth)) a[r.truth] = (a[r.truth] || 0) + 1; return a; }, {}))) });
  }
  for (const th of [0.9, 0.95]) {
    const hit = eligible.filter(r => r.c5 >= th && r.j5 < 0.6);
    sweep.contained.push({ th, tp: hit.filter(r => r.truth === 'contained').length, of: 3, other: Object.fromEntries(Object.entries(hit.filter(r => r.truth !== 'contained').reduce((a, r) => { a[r.truth] = (a[r.truth] || 0) + 1; return a; }, {}))) });
  }
  const realPass = (cosTh, c3Th) => real.filter(x => x.cos >= cosTh && x.c3 >= c3Th).length;
  for (const cosTh of [0.75, 0.8, 0.85, 0.88, 0.9]) for (const c3Th of [0, 0.05, 0.08, 0.1, 0.15]) {
    const hit = eligible.filter(r => r.cos >= cosTh && r.c3 >= c3Th && r.c5 < 0.25);
    sweep.semantic.push({ cosTh, c3Th, paraphraseTP: hit.filter(r => r.truth === 'paraphrase').length, of: 7,
      benchFP: hit.filter(r => r.truth === 'negative' || r.truth === 'template-pair').length, realCorpusPairs: realPass(cosTh, c3Th) });
  }
  // подавление частых шаблонных кусков: shingle, встречающийся в >= dfCap разделах
  const supp = bench.map((_, i) => new Set([...bi[i].s5].filter(h => df.get(h) < dfCap)));
  const tp = { 'template-pair': [], boiler: [] };
  for (const r of rows.filter(r => ['template-pair', 'boiler'].includes(r.truth))) {
    const k = inter(supp[r.a], supp[r.b]), m = Math.min(supp[r.a].size, supp[r.b].size);
    tp[r.truth].push(m ? k / m : 0);
  }
  sweep.template = { dfCap, c5AfterSuppression: { 'template-pair': stat(tp['template-pair']), boiler: stat(tp.boiler) },
    c5BeforeSuppression: { 'template-pair': stat(rows.filter(r => r.truth === 'template-pair').map(r => r.c5)), boiler: stat(rows.filter(r => r.truth === 'boiler').map(r => r.c5)) } };
  // различия чисел и отрицаний у копий (признаки для сравнения)
  sweep.diffFlags = Object.fromEntries(['exact', 'format', 'near', 'number', 'negation'].map(t => {
    const rs = rows.filter(r => r.truth === t); return [t, { n: rs.length, numDiff: rs.filter(r => r.numDiff).length, negDiff: rs.filter(r => r.negDiff).length }];
  }));
  fs.writeFileSync(path.join(out, 'sweep.json'), JSON.stringify(sweep, null, 2));
  console.log(JSON.stringify(sweep, (k, v) => typeof v === 'number' ? +v.toFixed(3) : v));

  // ---- задержка кандидатов на увеличенном корпусе ----
  const scale = [];
  const base = corpus.map(c => tokens(bodyOf(c.text)));
  const rnd = (() => { let x = 12345; return () => (x = (Math.imul(x, 1103515245) + 12345) >>> 0) / 4294967296; })();
  for (const N of [569, 5000, 20000]) {
    const docs = [];
    for (let i = 0; i < N; i++) {
      const src = base[i % base.length];
      docs.push(i < base.length ? src : src.map(w => (rnd() < 0.12 ? `x${(rnd() * 1e9) | 0}` : w)));
    }
    const t1 = performance.now();
    const sets = docs.map(t => shingles(t, 5));
    const t2 = performance.now();
    const post = new Map();
    sets.forEach((s, i) => { for (const h of s) { let l = post.get(h); if (!l) post.set(h, l = []); l.push(i); } });
    const cnt = new Map();
    for (const l of post.values()) { if (l.length < 2 || l.length > 20) continue; for (let x = 0; x < l.length; x++) for (let y = x + 1; y < l.length; y++) { const key = l[x] * 100003 + l[y]; cnt.set(key, (cnt.get(key) || 0) + 1); } }
    const t3 = performance.now();
    let cand = 0; for (const [key, k] of cnt) { const a = Math.floor(key / 100003), b = key % 100003; if (k / Math.min(sets[a].size, sets[b].size) >= 0.25 && k >= 10) cand++; }
    const t4 = performance.now();
    scale.push({ N, shingleMs: +(t2 - t1).toFixed(0), indexAndCountMs: +(t3 - t2).toFixed(0), filterMs: +(t4 - t3).toFixed(0), totalMs: +(t4 - t1).toFixed(0), candidatePairs: cand, distinctPairsCounted: cnt.size, shingleTotal: sets.reduce((a, s) => a + s.size, 0) });
  }
  // векторный перебор всех пар на реальных векторах
  const v0 = performance.now(); let acc = 0;
  for (let a = 0; a < cv.length; a++) for (let b = a + 1; b < cv.length; b++) acc += dot(cv[a], cv[b]) > 0.9 ? 1 : 0;
  const vms = performance.now() - v0, pairs = cv.length * (cv.length - 1) / 2;
  scale.push({ vectorAllPairs: { N: cv.length, pairs, ms: +vms.toFixed(0), extrapolated5000Ms: +(vms * (5000 * 4999 / 2) / pairs).toFixed(0), extrapolated20000Ms: +(vms * (20000 * 19999 / 2) / pairs).toFixed(0) } });
  fs.writeFileSync(path.join(out, 'latency.json'), JSON.stringify(scale, null, 2));
  console.log(JSON.stringify(scale));
}
