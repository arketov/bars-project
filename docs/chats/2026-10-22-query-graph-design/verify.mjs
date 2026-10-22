// Проверка проекта вида запроса (пункт 2.2.1) на сохранённом снимке bars-project.
// Запуск из корня: node docs/chats/2026-10-22-query-graph-design/verify.mjs
// Проверяет размер вида при разных числах результатов/режимах/глубине, детерминизм построения, порядок отсечения по бюджету.
// Это проверка правил на готовых рёбрах снимка (близость документов из kNN, не centroid), не качество поиска и не рабочий UI.
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const fixtureText = readFileSync(join(here, "../../../ui/src/spikes/graph-design/fixture.json"), "utf8");
const fixture = JSON.parse(fixtureText);
const D = fixture.docs.length;

const FANOUT = 4; // ближайших на результат по каждому виду
const THRESHOLD = 0.82; // порог режима «по смыслу» стенда 2.1.4
const BUDGET = { nodes: 200, edges: 800 };

// Рёбра документов: ссылки (направленные) и близость (неориентированная пара).
const edges = [];
for (const [a, b] of fixture.links) edges.push({ kind: "markdown_link", a, b, s: 1 });
for (const [a, b, w] of fixture.dsim) edges.push({ kind: "document_similarity", a, b, s: w });
const linkPair = new Set(fixture.links.map(([a, b]) => `${a}>${b}`));

const adjacency = new Map();
const touch = (n, e) => (adjacency.get(n) ?? adjacency.set(n, []).get(n)).push(e);
for (const e of edges) {
  touch(e.a, e);
  touch(e.b, e);
}
const other = (e, n) => (e.a === n ? e.b : e.a);

function kindsOf(mode) {
  return mode === "links" ? ["markdown_link"] : mode === "meaning" ? ["document_similarity"] : ["markdown_link", "document_similarity"];
}
const allowed = (e, kinds) => kinds.includes(e.kind) && (e.kind === "markdown_link" || e.s >= THRESHOLD);

/** Соседи узла по одному виду: результаты первыми, взаимные ссылки, затем сила, затем номер (правило проекта). */
function neighbours(node, kind, resultSet) {
  const seen = new Set();
  const list = [];
  for (const e of adjacency.get(node) ?? []) {
    if (e.kind !== kind || (kind === "document_similarity" && e.s < THRESHOLD)) continue;
    const o = other(e, node);
    if (o === node || seen.has(o)) continue;
    seen.add(o);
    const mutual = kind === "markdown_link" && linkPair.has(`${e.a}>${e.b}`) && linkPair.has(`${e.b}>${e.a}`) ? 1 : 0;
    list.push({ o, s: e.s, inResults: resultSet.has(o) ? 1 : 0, mutual });
  }
  list.sort((x, y) => y.inResults - x.inResults || y.mutual - x.mutual || y.s - x.s || x.o - y.o);
  return list;
}

/** Построение: возвращает порядок включения узлов (круговой обход результатов), рёбра между включёнными. */
function build(results, mode, depth, budget = BUDGET) {
  const kinds = kindsOf(mode);
  const resultSet = new Set(results);
  const order = [...results];
  const included = new Set(results);
  let frontier = results;
  let truncated = false;
  for (let hop = 0; hop < depth; hop++) {
    const queues = frontier.map((n) => kinds.flatMap((k) => neighbours(n, k, resultSet).slice(0, FANOUT).map((x) => x.o)));
    const next = [];
    for (let round = 0; ; round++) {
      let any = false;
      for (const q of queues) {
        if (round >= q.length) continue;
        any = true;
        const n = q[round];
        if (included.has(n)) continue;
        if (included.size + 1 + 1 > budget.nodes) {
          truncated = true;
          continue;
        }
        included.add(n);
        order.push(n);
        next.push(n);
      }
      if (!any) break;
    }
    frontier = next;
  }
  const shown = edges.filter((e) => allowed(e, kinds) && included.has(e.a) && included.has(e.b));
  // Рёбра запроса: по одному на результат; узел запроса — один.
  return { order, nodes: included.size + 1, edges: shown.length + results.length, truncated };
}

// Детерминированный генератор (mulberry32): наборы результатов одинаковы между запусками.
function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
function pick(r, n, seed) {
  const rand = rng(seed + n * 1009);
  const set = new Set();
  while (set.size < n) set.add(Math.floor(rand() * D));
  return [...set];
}
const quantile = (xs, q) => xs.slice().sort((a, b) => a - b)[Math.min(xs.length - 1, Math.floor(q * xs.length))];

const checks = [];
const check = (name, ok, detail) => checks.push({ name, ok, detail });
const table = [];

for (const R of [3, 5, 10, 20]) {
  for (const mode of ["links", "meaning", "all"]) {
    for (const depth of [0, 1, 2]) {
      const nodes = [];
      const edgeCounts = [];
      let cut = 0;
      for (let seed = 1; seed <= 200; seed++) {
        const r = build(pick(null, R, seed), mode, depth, { nodes: 1e9, edges: 1e9 });
        nodes.push(r.nodes);
        edgeCounts.push(r.edges);
        if (r.nodes > BUDGET.nodes || r.edges > BUDGET.edges) cut++;
      }
      table.push({ results: R, mode, depth, nodes: [quantile(nodes, 0), quantile(nodes, 0.5), quantile(nodes, 0.95), quantile(nodes, 1)], edges: [quantile(edgeCounts, 0), quantile(edgeCounts, 0.5), quantile(edgeCounts, 0.95), quantile(edgeCounts, 1)], overBudget: cut / 200 });
    }
  }
}

// Детерминизм: два построения дают одинаковый порядок включения.
{
  const a = build(pick(null, 10, 7), "all", 2);
  const b = build(pick(null, 10, 7), "all", 2);
  check("построение детерминировано", JSON.stringify(a) === JSON.stringify(b), `${a.nodes} узлов`);
}
// Глубина 0: только запрос и результаты, рёбра запроса плюс связи между результатами.
{
  const r = build(pick(null, 10, 3), "all", 0);
  check("глубина 0 не добавляет узлов", r.nodes === 11, `${r.nodes}`);
}
// Круговой обход: при усечении у каждого результата сначала по одному соседу, затем по второму.
{
  const results = pick(null, 10, 11);
  const full = build(results, "links", 1, { nodes: 1e9, edges: 1e9 });
  const tight = build(results, "links", 1, { nodes: 10 + 1 + 10, edges: 1e9 });
  check("при усечении добавлено ровно допустимое число соседей", tight.nodes <= 21 && tight.truncated === (full.nodes > 21), `${tight.nodes} из ${full.nodes}`);
  const firsts = new Set(results.flatMap((n) => (neighbours(n, "markdown_link", new Set(results)).slice(0, 1).map((x) => x.o))));
  const taken = new Set(tight.order);
  const firstsTaken = [...firsts].filter((n) => taken.has(n)).length;
  check("первые соседи берутся раньше вторых", firstsTaken >= Math.min(firsts.size, tight.order.length - results.length) || firsts.size === 0, `первых ${firsts.size}, взято ${firstsTaken}, соседей ${tight.order.length - results.length}`);
}
// Порог «по смыслу» не меняет силу оставшихся рёбер (отбор, не пересчёт).
{
  const kept = edges.filter((e) => e.kind === "document_similarity" && e.s >= THRESHOLD);
  check("сила допущенной близости равна исходной", kept.every((e) => fixture.dsim.some(([a, b, w]) => a === e.a && b === e.b && w === e.s)), `${kept.length} пар`);
}
// Хабы не получают приоритета: порядок соседей зависит от результатов, взаимности, силы и номера, а не от степени.
{
  const hub = [...adjacency.entries()].sort((x, y) => y[1].length - x[1].length)[0][0];
  // Результат — сосед хаба, у которого достаточно ссылочных соседей, чтобы хаб мог оказаться в усечённом списке.
  const start = (adjacency.get(hub) ?? []).map((e) => other(e, hub)).find((n) => neighbours(n, "markdown_link", new Set()).length > FANOUT && neighbours(n, "markdown_link", new Set()).some((x) => x.o === hub));
  const results = [start];
  const list = neighbours(results[0], "markdown_link", new Set(results));
  const hubPos = list.findIndex((x) => x.o === hub);
  const expectedPos = list.slice().sort((x, y) => y.inResults - x.inResults || y.mutual - x.mutual || x.o - y.o).findIndex((x) => x.o === hub);
  check("порядок соседей не зависит от степени узла-хаба", hubPos === expectedPos, `позиция хаба ${hubPos}`);
}

const failed = checks.filter((c) => !c.ok);
console.log(JSON.stringify({ table: table.filter((t) => t.depth > 0 || t.mode === "all"), checks }, null, 1));
writeFileSync(
  join(here, "verification.json"),
  JSON.stringify(
    {
      input: { file: "ui/src/spikes/graph-design/fixture.json", sha256: createHash("sha256").update(fixtureText).digest("hex"), documents: D, links: fixture.links.length, similarityPairs: fixture.dsim.length },
      parameters: { fanout: FANOUT, threshold: THRESHOLD, budget: BUDGET, sets: "200 детерминированных наборов результатов на строку", columns: "узлы и рёбра: минимум, медиана, 95-й процентиль, максимум" },
      table,
      checks,
    },
    null,
    1,
  ),
);
if (failed.length) {
  console.error("НЕ ПРОЙДЕНО:", failed.map((c) => c.name));
  process.exit(1);
}
