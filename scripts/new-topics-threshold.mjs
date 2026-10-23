// Калибровка порога дедупликации вопросов для «Новых тем» (IDEA-14, шаг P1).
//   node scripts/new-topics-threshold.mjs prepare <папка>          -> <папка>/texts.json, items.json
//   node scripts/new-topics-threshold.mjs analyze <папка> <режим>.json[,<режим>.json...] [--relabel relabel.json]
// Векторы считает crates/bars-core/examples/bench_questions.rs. Метки — до просмотра косинусов
// (variants.json); исправления очевидных ошибок разметки после просмотра хранятся отдельно (relabel.json).
import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { gunzipSync } from "node:zlib";

import { fileURLToPath } from "node:url";
const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const OVERDRIVE = path.join(ROOT, "crates/bars-core/examples/quality/overdrive.json");
const VARIANTS = path.join(ROOT, "docs/knowledge/new-topics-threshold-2026-10-19/variants.json");

const sha = (file) => crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
const dot = (a, b) => { let s = 0; for (let i = 0; i < a.length; i++) s += a[i] * b[i]; return s; };

function build() {
  const od = JSON.parse(fs.readFileSync(OVERDRIVE, "utf8"));
  const variants = JSON.parse(fs.readFileSync(VARIANTS, "utf8"));
  const items = [];
  const byText = new Map();
  const byId = new Map();
  const add = (text, group, role, split, kind, ref) => {
    if (byText.has(text)) throw new Error(`повтор текста: ${text}`);
    const item = { index: items.length, text, group, role, split, kind, ref };
    items.push(item); byText.set(text, item);
    return item;
  };
  for (const q of od) {
    const item = add(q.query, q.id, "orig", q.split === "tune" ? "dev" : "test", q.category, q.id);
    byId.set(q.id, item);
  }
  for (const v of variants.paraphrases) {
    const base = byId.get(v.of);
    if (!base) throw new Error(`нет вопроса ${v.of}`);
    add(v.text, v.of, "variant", base.split, v.kind, v.of);
  }
  const guards = [];
  for (const g of variants.guards) {
    const side = (value, tag) => {
      if (/^od-\d+$/.test(value)) return byId.get(value);
      return byText.get(value) ?? add(value, `${g.id}${tag}`, "guard", g.split, g.kind, g.id);
    };
    const a = side(g.a, "a"), b = side(g.b, "b");
    guards.push({ id: g.id, split: g.split, kind: g.kind, a: a.index, b: b.index });
  }
  return { items, guards };
}

function prepare(dir) {
  fs.mkdirSync(dir, { recursive: true });
  const { items, guards } = build();
  fs.writeFileSync(path.join(dir, "texts.json"), JSON.stringify(items.map((i) => i.text)));
  fs.writeFileSync(path.join(dir, "items.json"), JSON.stringify({ items, guards }));
  const count = (split, role) => items.filter((i) => i.split === split && i.role === role).length;
  console.log(`текстов ${items.length}; guard-пар ${guards.length}`);
  for (const s of ["dev", "test"]) console.log(s, "orig", count(s, "orig"), "variant", count(s, "variant"), "guard-тексты", count(s, "guard"));
}

// ---- осторожные лексические проверки: число/версия/идентификатор/отрицание ----
const NUMBER_WORDS = {
  0: ["ноль", "нуля"], 1: ["один", "одна", "одно", "одного", "первый", "первая", "первое", "первой", "первого", "первом"],
  2: ["два", "две", "двух", "второй", "вторая", "второе", "второго", "втором", "вторую"],
  3: ["три", "трёх", "трех", "третий", "третья", "третье", "третьей", "третьего", "третьем"],
  4: ["четыре", "четырёх", "четырех", "четвёртый", "четвертый"], 5: ["пять", "пяти", "пятый", "пятой"],
  6: ["шесть", "шести"], 7: ["семь", "семи"], 8: ["восемь", "восьми"], 9: ["девять", "девяти"], 10: ["десять", "десяти"],
};
const WORD_TO_NUMBER = new Map(Object.entries(NUMBER_WORDS).flatMap(([n, ws]) => ws.map((w) => [w, n])));
const NEGATION = new Set(["не", "нет", "без", "нельзя", "ни", "нигде", "никогда", "невозможно"]);
// Небольшой общий список пар противоположностей; полноты не обещает.
const ANTONYMS = [[/^(включ|включа)/, /^(отключ|выключ|отключа|выключа)/], [/^открыт/, /^закрыт/], [/^успешн/, /^(ошибк|неудач|сбо)/], [/^разреш/, /^запрещ|^запрет/]];

function tokens(text) { return text.match(/[\p{L}\p{N}_.\-]+/gu) ?? []; }
function isSpecial(token) {
  const t = token.replace(/^[.\-]+|[.\-]+$/g, "");
  if (!t) return false;
  if (/\d/.test(t)) return true;
  if (/[_]/.test(t)) return true;
  if (/[A-Za-z]/.test(t) && /[.]/.test(t)) return true;
  if (/^[A-Za-z]+[A-Z][A-Za-z]*$/.test(t) && /[a-z]/.test(t)) return true;
  if (/^[A-Z]{2,}$/.test(t)) return true;
  if (/^[A-Za-z]+-[A-Za-z0-9]+(-[A-Za-z0-9]+)*$/.test(t)) return true;
  return false;
}
function features(text) {
  const raw = tokens(text);
  const special = new Set(), numbers = [], lower = raw.map((t) => t.toLowerCase());
  for (const t of raw) if (isSpecial(t)) special.add(t.replace(/^[.\-]+|[.\-]+$/g, ""));
  for (const t of lower) { if (WORD_TO_NUMBER.has(t)) numbers.push(WORD_TO_NUMBER.get(t)); }
  const negations = lower.filter((t) => NEGATION.has(t)).length;
  return { special, numbers: numbers.sort().join(","), negations, lower };
}
const sameSet = (a, b) => a.size === b.size && [...a].every((x) => b.has(x));
// Компоненты проверки: id — идентификаторы и цифры; numw — числительные словами (только если они есть
// в обоих текстах); ant — короткий список противоположностей; neg — чётность отрицаний.
export const PARTS_ALL = ["id", "numw", "ant", "neg"];
// Правило включения (задано до просмотра test): компонент входит, если на dev ловит больше ловушек, чем
// блокирует настоящих перефразов. neg: ловит 3 дополнительных ловушки ценой 5 перефразов — не входит.
export let DEFAULT_PARTS = ["id", "numw", "ant"];
export function setDefaultParts(parts) { DEFAULT_PARTS = parts; }
// Непустой массив — проверка видит смысловое отличие и запрещает автоматическое слияние.
export function guardDiffers(textA, textB, parts = DEFAULT_PARTS) {
  const a = features(textA), b = features(textB);
  const why = [];
  // у перефраза идентификатор может исчезнуть только вместе с вопросом: пустое множество не сравниваем
  if (parts.includes("id") && (a.special.size || b.special.size) && !sameSet(a.special, b.special)) why.push("identifier");
  if (parts.includes("numw") && a.numbers && b.numbers && a.numbers !== b.numbers) why.push("number");
  if (parts.includes("neg") && (a.negations & 1) !== (b.negations & 1)) why.push("negation");
  if (parts.includes("ant")) for (const [x, y] of ANTONYMS) {
    const ax = a.lower.some((t) => x.test(t)), ay = a.lower.some((t) => y.test(t));
    const bx = b.lower.some((t) => x.test(t)), by = b.lower.some((t) => y.test(t));
    if ((ax && !ay && by && !bx) || (ay && !ax && bx && !by)) why.push("antonym");
  }
  return why;
}

// ---- анализ ----
const quantile = (sorted, q) => sorted.length ? sorted[Math.min(sorted.length - 1, Math.max(0, Math.ceil(q * sorted.length) - 1))] : null;
const round = (x, d = 4) => x === null || x === undefined ? null : Math.round(x * 10 ** d) / 10 ** d;

function mulberry32(seed) { return () => { seed |= 0; seed = (seed + 0x6d2b79f5) | 0; let t = Math.imul(seed ^ (seed >>> 15), 1 | seed); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; }; }

function pairsOf(items, guards, vectors, relabel) {
  // Пара групп из guards: любая пара текстов этих двух групп — тоже «ловушка» (перефраз исходного вопроса
  // тоже не должен слиться с его подправленной копией).
  const guardGroups = new Map();
  for (const g of guards) { const a = items[g.a].group, b = items[g.b].group; guardGroups.set(`${a}|${b}`, g); guardGroups.set(`${b}|${a}`, g); }
  const forced = new Set((relabel?.same ?? []).map(([a, b]) => `${Math.min(a, b)}-${Math.max(a, b)}`));
  const out = [];
  for (let i = 0; i < items.length; i++) for (let j = i + 1; j < items.length; j++) {
    if (items[i].split !== items[j].split) continue;
    const key = `${i}-${j}`;
    const cos = dot(vectors[i], vectors[j]);
    const guard = guardGroups.get(`${items[i].group}|${items[j].group}`);
    let label, category;
    if (guard) { label = "different"; category = `guard:${guard.kind}`; }
    else if (items[i].group === items[j].group || forced.has(key)) { label = "same"; category = items[j].kind === "keywords" || items[i].kind === "keywords" ? "same:keywords" : "same:paraphrase"; }
    else { label = "different"; category = "different:ordinary"; }
    out.push({ i, j, split: items[i].split, label, category, cos });
  }
  return out;
}

function curve(pairs, split, thresholds, items) {
  const rows = [];
  for (const t of thresholds) {
    const escapes = {}; let same = 0, merged = 0, ordinary = 0, ordinaryMerged = 0, ordinaryMergedAfterCheck = 0, guard = 0, guardMerged = 0, guardMergedAfterCheck = 0, sameBlocked = 0;
    for (const p of pairs) {
      if (p.split !== split) continue;
      const over = p.cos >= t;
      if (p.label === "same") { same++; if (over) { merged++; if (guardDiffers(items[p.i].text, items[p.j].text).length) sameBlocked++; } }
      else if (p.category === "different:ordinary") { ordinary++; if (over) { ordinaryMerged++; if (!guardDiffers(items[p.i].text, items[p.j].text).length) ordinaryMergedAfterCheck++; } }
      else { guard++; if (over) { guardMerged++; if (!guardDiffers(items[p.i].text, items[p.j].text).length) { guardMergedAfterCheck++; const k = p.category.slice(6); escapes[k] = (escapes[k] ?? 0) + 1; } } }
    }
    rows.push({ threshold: round(t, 2), same, mergedSame: merged, recall: round(merged / same), sameBlockedByCheck: sameBlocked, recallWithChecks: round((merged - sameBlocked) / same), ordinary, falseMerges: ordinaryMerged, falseMergesAfterCheck: ordinaryMergedAfterCheck, guard, guardMerged, guardMergedAfterCheck, escapes });
  }
  return rows;
}

export function wilson(k, n, z = 1.96) { if (!n) return null; const p = k / n, d = 1 + z * z / n, c = p + z * z / (2 * n), m = z * Math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)); return [round((c - m) / d, 3), round((c + m) / d, 3)]; }
function stats(pairs, split, category) {
  const v = pairs.filter((p) => p.split === split && (typeof category === "string" ? p.category === category : category(p))).map((p) => p.cos).sort((a, b) => a - b);
  return { n: v.length, min: round(v[0]), p05: round(quantile(v, 0.05)), p50: round(quantile(v, 0.5)), p95: round(quantile(v, 0.95)), p99: round(quantile(v, 0.99)), max: round(v[v.length - 1]) };
}

// Жадное объединение с устойчивым представителем против цепочки (single-link).
function clustering(items, vectors, split, threshold, seed, mode, withChecks) {
  const order = items.filter((i) => i.split === split).map((i) => i.index);
  const rnd = mulberry32(seed);
  for (let k = order.length - 1; k > 0; k--) { const r = Math.floor(rnd() * (k + 1)); [order[k], order[r]] = [order[r], order[k]]; }
  const clusters = []; // {rep, members}
  for (const idx of order) {
    let best = null, bestCos = -1;
    for (const c of clusters) {
      const s = mode === "single-link" ? Math.max(...c.members.map((m) => dot(vectors[idx], vectors[m]))) : dot(vectors[idx], vectors[c.rep]);
      if (withChecks && c.members.some((m) => guardDiffers(items[idx].text, items[m].text).length)) continue;
      if (s >= threshold && s > bestCos) { best = c; bestCos = s; }
    }
    if (best) best.members.push(idx); else clusters.push({ rep: idx, members: [idx] });
  }
  let falseJoins = 0;
  for (const c of clusters) for (const m of c.members) if (items[m].group !== items[c.rep].group) falseJoins++;
  const groups = new Map();
  for (const c of clusters) for (const m of c.members) { const g = items[m].group; groups.set(g, (groups.get(g) ?? new Set()).add(c.rep)); }
  const splitGroups = [...groups.values()].filter((s) => s.size > 1).length;
  return { clusters: clusters.length, falseJoins, groupsSplit: splitGroups };
}

function analyze(dir, modes, relabelFile) {
  const { items, guards } = JSON.parse(fs.readFileSync(path.join(dir, "items.json"), "utf8"));
  const relabel = relabelFile ? JSON.parse(fs.readFileSync(relabelFile, "utf8")) : null;
  const loaded = modes.map((file) => { const raw = fs.readFileSync(path.join(dir, file)); return { file, ...JSON.parse(file.endsWith(".gz") ? gunzipSync(raw).toString("utf8") : raw.toString("utf8")) }; });
  const report = { modes: {} };
  const thresholds = Array.from({ length: 46 }, (_, k) => 0.5 + k * 0.01);
  // шум устройства/точности относительно первого режима
  const base = loaded[0];
  for (const m of loaded) {
    let maxDiff = 0, sum = 0, n = 0;
    for (let i = 0; i < items.length; i++) for (let j = i + 1; j < items.length; j += 7) { const d = Math.abs(dot(base.vectors[i], base.vectors[j]) - dot(m.vectors[i], m.vectors[j])); maxDiff = Math.max(maxDiff, d); sum += d; n++; }
    report.modes[m.mode] = { fingerprint: m.fingerprint, execution: m.execution, seconds: round(m.seconds, 2), noiseVsFirst: { maxAbs: round(maxDiff), meanAbs: round(sum / n, 6), pairs: n } };
  }
  for (const m of loaded) {
    const pairs = pairsOf(items, guards, m.vectors, relabel);
    const entry = report.modes[m.mode];
    entry.counts = {};
    for (const split of ["dev", "test"]) {
      entry.counts[split] = {};
      for (const p of pairs) if (p.split === split) entry.counts[split][p.category] = (entry.counts[split][p.category] ?? 0) + 1;
    }
    entry.stats = {};
    for (const split of ["dev", "test"]) {
      entry.stats[split] = {
        same_paraphrase: stats(pairs, split, "same:paraphrase"), same_keywords: stats(pairs, split, "same:keywords"),
        different_ordinary: stats(pairs, split, "different:ordinary"),
        guard_negation: stats(pairs, split, "guard:negation"), guard_number: stats(pairs, split, "guard:number"),
        guard_identifier: stats(pairs, split, "guard:identifier"), guard_entity: stats(pairs, split, "guard:entity"),
      };
    }
    entry.curve = { dev: curve(pairs, "dev", thresholds, items), test: curve(pairs, "test", thresholds, items) };
    // для разбора: самые близкие обычные различные пары и самые далёкие одинаковые
    const textOf = (p) => ({ cos: round(p.cos), a: items[p.i].text, b: items[p.j].text, groups: [items[p.i].group, items[p.j].group], indexes: [p.i, p.j] });
    entry.review = {};
    for (const split of ["dev", "test"]) {
      entry.review[split] = {
        closestDifferent: pairs.filter((p) => p.split === split && p.category === "different:ordinary").sort((a, b) => b.cos - a.cos).slice(0, 12).map(textOf),
        farthestSame: pairs.filter((p) => p.split === split && p.label === "same").sort((a, b) => a.cos - b.cos).slice(0, 12).map(textOf),
        guards: pairs.filter((p) => p.split === split && p.category.startsWith("guard")).sort((a, b) => b.cos - a.cos).map((p) => ({ ...textOf(p), kind: p.category, check: guardDiffers(items[p.i].text, items[p.j].text) })),
      };
    }
    // Правило выбора (заранее): максимум dev-косинуса обычных различных пар, вверх до 0,01, плюс запас.
    const devMax = Math.max(...pairs.filter((p) => p.split === "dev" && p.category === "different:ordinary").map((p) => p.cos));
    const noise = report.modes[m.mode].noiseVsFirst.maxAbs;
    const margin = Math.max(0.02, round(2 * Math.max(...loaded.map((x) => report.modes[x.mode].noiseVsFirst.maxAbs)), 2));
    const chosen = round(Math.ceil(devMax * 100) / 100 + margin, 2);
    entry.selection = { rule: "ceil(max dev cos ordinary-different, 0.01) + max(0.02, 2 * max noise)", devMaxDifferent: round(devMax), margin, chosen };
    const tp = curve(pairs, "dev", [chosen], items)[0], tq = curve(pairs, "test", [chosen], items)[0];
    entry.pooledAtChosen = { same: tp.same + tq.same, merged: tp.mergedSame + tq.mergedSame, recallWilson95: wilson(tp.mergedSame + tq.mergedSame, tp.same + tq.same), ordinaryPairs: tp.ordinary + tq.ordinary, falseMerges: tp.falseMerges + tq.falseMerges, falseMergeRateUpper95: round(3 / (tp.ordinary + tq.ordinary), 5) };
    entry.atChosen = { dev: curve(pairs, "dev", [chosen], items)[0], test: curve(pairs, "test", [chosen], items)[0] };
    entry.clustering = {};
    for (const mode of ["representative", "single-link", "representative+checks", "single-link+checks"]) {
      entry.clustering[mode] = {};
      for (const split of ["dev", "test"]) {
        const rows = [];
        for (let seed = 1; seed <= 20; seed++) rows.push(clustering(items, m.vectors, split, chosen, seed, mode.replace("+checks", ""), mode.endsWith("+checks")));
        entry.clustering[mode][split] = { seeds: 20, maxFalseJoins: Math.max(...rows.map((r) => r.falseJoins)), meanFalseJoins: round(rows.reduce((s, r) => s + r.falseJoins, 0) / rows.length, 2), maxGroupsSplit: Math.max(...rows.map((r) => r.groupsSplit)), meanGroupsSplit: round(rows.reduce((s, r) => s + r.groupsSplit, 0) / rows.length, 2), clusters: rows[0].clusters, groups: new Set(items.filter((i) => i.split === split).map((i) => i.group)).size };
      }
    }
  }
  report.inputs = { overdrive: sha(OVERDRIVE), variants: sha(VARIANTS), script: sha(path.join(HERE, "new-topics-threshold.mjs")), relabel: relabel ? sha(relabelFile) : null };
  fs.writeFileSync(path.join(dir, "analysis.json"), JSON.stringify(report, null, 1));
  console.log(JSON.stringify({ modes: Object.fromEntries(Object.entries(report.modes).map(([k, v]) => [k, { noise: v.noiseVsFirst, counts: v.counts, stats: v.stats, selection: v.selection, atChosen: v.atChosen, clustering: v.clustering }])) }, null, 1));
}

function checks(dir) {
  const { items, guards } = JSON.parse(fs.readFileSync(path.join(dir, "items.json"), "utf8"));
  console.log("Перефразы, которые проверка сочла разными:");
  for (const v of items.filter((i) => i.role === "variant")) {
    const o = items.find((i) => i.group === v.group && i.role === "orig");
    const why = guardDiffers(o.text, v.text);
    if (why.length) console.log(` [${why}] ${o.text} | ${v.text}`);
  }
  console.log("Ловушки, которые проверка пропустила:");
  for (const g of guards) if (!guardDiffers(items[g.a].text, items[g.b].text).length) console.log(` ${g.id} ${g.kind}: ${items[g.a].text} | ${items[g.b].text}`);
}

function components(dir) {
  const { items, guards } = JSON.parse(fs.readFileSync(path.join(dir, "items.json"), "utf8"));
  const sets = [["id"], ["id", "numw"], ["id", "numw", "ant"], ["id", "numw", "ant", "neg"], ["neg"], ["ant"], ["numw"]];
  const rows = [];
  for (const split of ["dev", "test"]) for (const parts of sets) {
    const para = items.filter((i) => i.role === "variant" && i.split === split);
    const blocked = para.filter((v) => guardDiffers(items.find((i) => i.group === v.group && i.role === "orig").text, v.text, parts).length).length;
    const gs = guards.filter((g) => g.split === split);
    const caught = {};
    for (const g of gs) { caught[g.kind] ??= [0, 0]; caught[g.kind][1]++; if (guardDiffers(items[g.a].text, items[g.b].text, parts).length) caught[g.kind][0]++; }
    rows.push({ split, parts: parts.join("+"), paraphrasesBlocked: `${blocked}/${para.length}`, guardsCaught: Object.entries(caught).map(([k, [c, n]]) => `${k} ${c}/${n}`).join(", ") });
  }
  console.table(rows);
}

const [cmd, dir, modes, ...rest] = process.argv.slice(2);
if (cmd === "prepare") prepare(dir);
else if (cmd === "checks") checks(dir);
else if (cmd === "components") components(dir);
else if (cmd === "analyze") {
  const i = rest.indexOf("--relabel");
  analyze(dir, modes.split(","), i >= 0 ? rest[i + 1] : null);
} else if (cmd !== undefined) { console.error("prepare|analyze"); process.exit(2); }
