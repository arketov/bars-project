// Данные стенда 2.3.1: настоящий граф замороженного корпуса проверки качества 20-10-2026 (66 документов bars-project).
// Источник — docs/knowledge/graph-quality-2026-10-20: index.sqlite.gz (граф explicit-markdown-v4) и results.json.gz (ответы операций).
// Запуск из корня репозитория: node ui/src/spikes/graph-actions/build-fixture.mjs
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from "node:fs";
import { gunzipSync } from "node:zlib";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { DatabaseSync } from "node:sqlite";

const SRC = "docs/knowledge/graph-quality-2026-10-20";
const OUT = "ui/src/spikes/graph-actions/fixture.json";

const dir = mkdtempSync(join(tmpdir(), "bars-graph-actions-"));
const dbPath = join(dir, "index.sqlite");
writeFileSync(dbPath, gunzipSync(readFileSync(join(SRC, "index.sqlite.gz"))));
const db = new DatabaseSync(dbPath, { readOnly: true });
const parse = (s) => JSON.parse(s);

const meta = Object.fromEntries(db.prepare("select key, value from graph_meta").all().map((r) => [r.key, r.value]));
const nodes = new Map(db.prepare("select id, kind, data from graph_nodes").all().map((r) => [r.id, { kind: parse(r.kind), ...parse(r.data) }]));
const evidence = new Map(db.prepare("select id, data from graph_evidence").all().map((r) => [r.id, parse(r.data)]));
const edges = db.prepare("select id, predicate, data from graph_edges").all().map((r) => ({ predicate: parse(r.predicate), ...parse(r.data) }));
const aux = Object.fromEntries(db.prepare("select key, data from graph_aux").all().map((r) => [r.key, parse(r.data)]));
const sources = db.prepare("select path, data from graph_sources").all().map((r) => parse(r.data).meta);
db.close();
rmSync(dir, { recursive: true, force: true });

const results = JSON.parse(gunzipSync(readFileSync(join(SRC, "results.json.gz"))).toString());
const gold = JSON.parse(readFileSync(join(SRC, "gold.json"), "utf8"));
const rows = Object.fromEntries(results.rows.map((r) => [r.id, r]));

// Документы: порядок по пути, индекс — номер в массиве.
sources.sort((a, b) => a.docs_path.localeCompare(b.docs_path));
const docIndex = new Map(sources.map((s, i) => [s.reference.id, i]));
const docByPath = new Map(sources.map((s, i) => [s.docs_path, i]));
const docOf = (id) => {
  let n = nodes.get(id);
  for (let guard = 0; n && guard < 20; guard++) {
    if (n.kind === "document") return docIndex.get(n.reference.id);
    n = n.parent ? nodes.get(n.parent) : null;
  }
  return undefined;
};
const span = (evId) => evidence.get(evId)?.span;

// Цепочка заголовков раздела: от документа к разделу.
const chain = (sectionId) => {
  const out = [];
  let n = nodes.get(sectionId);
  for (let guard = 0; n && n.kind === "section" && guard < 12; guard++) {
    out.unshift(n.label);
    n = n.parent ? nodes.get(n.parent) : null;
  }
  return out;
};

// Чанки: диапазон строк из основания узла, раздел — через covers (раздел → чанк).
const coveredBy = new Map();
for (const e of edges) if (e.predicate === "covers") coveredBy.set(e.to, [...(coveredBy.get(e.to) ?? []), e.from]);
const chunks = [];
for (const n of nodes.values()) {
  if (n.kind !== "chunk") continue;
  const doc = docOf(n.reference.id);
  const s = span(n.evidence[0]);
  if (doc === undefined || !s) continue;
  const sections = (coveredBy.get(n.reference.id) ?? []).map((id) => nodes.get(id)).filter((x) => x?.kind === "section");
  const section = sections.sort((a, b) => (span(a.evidence[0])?.first_line ?? 0) - (span(b.evidence[0])?.first_line ?? 0))[0];
  chunks.push({ id: n.reference.id, doc, first: s.first_line, last: s.last_line, heading: section ? chain(section.reference.id) : [n.label], roles: n.roles });
}
chunks.sort((a, b) => a.doc - b.doc || a.first - b.first);
const chunkIndex = new Map(chunks.map((c, i) => [c.id, i]));
const chunksOfDoc = new Map();
chunks.forEach((c, i) => chunksOfDoc.set(c.doc, [...(chunksOfDoc.get(c.doc) ?? []), i]));
const chunkAtLine = (doc, line) => (chunksOfDoc.get(doc) ?? []).find((i) => chunks[i].first <= line && line <= chunks[i].last) ?? (chunksOfDoc.get(doc) ?? [])[0];

// Ссылки: документ → документ со строками; для уровня чанков — чанк со строкой → чанк раздела-цели или первый чанк цели.
const linkMap = new Map();
const chunkLinks = new Map();
for (const e of edges) {
  if (e.predicate !== "links_to") continue;
  const a = docOf(e.from);
  const b = docOf(e.to);
  const s = span(e.evidence[0]);
  if (a === undefined || b === undefined || a === b || !s) continue;
  const key = `${a}:${b}`;
  linkMap.set(key, [...(linkMap.get(key) ?? []), s.first_line]);
  const from = chunkAtLine(a, s.first_line);
  const target = nodes.get(e.to);
  const anchorLine = target?.kind === "section" ? span(target.evidence[0])?.first_line : undefined;
  const to = anchorLine !== undefined ? chunkAtLine(b, anchorLine) : (chunksOfDoc.get(b) ?? [])[0];
  if (from !== undefined && to !== undefined) chunkLinks.set(`${from}:${to}`, [...(chunkLinks.get(`${from}:${to}`) ?? []), s.first_line]);
}
const links = [...linkMap].map(([k, lines]) => [...k.split(":").map(Number), [...new Set(lines)].sort((x, y) => x - y)]);
const cLinks = [...chunkLinks].map(([k, lines]) => [...k.split(":").map(Number), [...new Set(lines)].sort((x, y) => x - y)]);

// Близость: чанки — как есть (similar_to со score), документы — максимум по парам чанков, k=5 ближайших, cos ≥ 0,5.
const chunkSims = [];
const docBest = new Map();
for (const e of edges) {
  if (e.predicate !== "similar_to" || typeof e.score !== "number") continue;
  const a = chunkIndex.get(e.from);
  const b = chunkIndex.get(e.to);
  if (a === undefined || b === undefined) continue;
  chunkSims.push([a, b, Math.round(e.score * 1000) / 1000]);
  const da = chunks[a].doc;
  const db2 = chunks[b].doc;
  if (da === db2) continue;
  const key = da < db2 ? `${da}:${db2}` : `${db2}:${da}`;
  docBest.set(key, Math.max(docBest.get(key) ?? 0, e.score));
}
const near = new Map();
for (const [key, cos] of docBest) for (const d of key.split(":").map(Number)) near.set(d, [...(near.get(d) ?? []), [key, cos]]);
const keep = new Set();
for (const list of near.values()) for (const [key, cos] of list.sort((x, y) => y[1] - x[1]).slice(0, 5)) if (cos >= 0.5) keep.add(key);
const docSims = [...keep].map((key) => [...key.split(":").map(Number), Math.round(docBest.get(key) * 1000) / 1000]);

// Основания, на которые ссылаются сущности, утверждения, упоминания, аудит и контекст.
const usedEvidence = new Map();
const useEv = (id, pool = evidence) => {
  const ev = pool.get(id);
  if (!ev) return null;
  if (!usedEvidence.has(id)) {
    const doc = docIndex.get(ev.document.id);
    if (doc === undefined) return null;
    usedEvidence.set(id, [doc, ev.span.first_line, ev.span.last_line, ev.quote.length > 420 ? `${ev.quote.slice(0, 419)}…` : ev.quote, ev.method]);
  }
  return id;
};

// Сущности: имя, документ определения (namespace после «:»), основания.
const entities = [];
const entityIndex = new Map();
for (const n of nodes.values()) {
  if (n.kind !== "entity") continue;
  const path = n.namespace.split(":").pop();
  const doc = docByPath.get(path);
  entityIndex.set(n.reference.id, entities.length);
  entities.push({ label: n.label, doc: doc ?? -1, aliases: n.aliases, evidence: n.evidence.map((id) => useEv(id)).filter(Boolean) });
}
const mentions = [];
const assertions = [];
for (const e of edges) {
  if (e.predicate === "mentions") {
    const doc = docOf(e.from);
    const ent = entityIndex.get(e.to);
    if (doc !== undefined && ent !== undefined) mentions.push([doc, ent, e.evidence.map((id) => useEv(id)).filter(Boolean)]);
  } else if (["requires", "supersedes", "uses", "verifies", "depends_on", "implements"].includes(e.predicate)) {
    const a = entityIndex.get(e.from);
    const b = entityIndex.get(e.to);
    if (a === undefined || b === undefined) continue;
    assertions.push({ from: a, to: b, predicate: e.predicate, polarity: e.polarity, qualifiers: e.qualifiers, origin: e.origin, evidence: e.evidence.map((id) => useEv(id)).filter(Boolean) });
  }
}

// Неразрешённые ссылки (аудит структурного слоя).
const broken = (aux.broken_links ?? []).map((b) => {
  const doc = docOf(b.source);
  const ev = evidence.get(b.evidence);
  return doc === undefined ? null : [doc, b.target, ev?.span.first_line ?? 0, b.reason];
}).filter(Boolean);

// Записанные ответы операций: темы S6, аудит S8, контекст S3.
const replyEvidence = (row) => new Map((row.graph.evidence ?? []).map((ev) => [ev.id, ev]));
const topicsRow = rows["holdout-S6-positive"];
const topics = topicsRow.graph.items.filter((i) => i.type === "topic").map((t) => ({
  label: t.label,
  members: t.members.map((m) => docIndex.get(m.id)).filter((x) => x !== undefined),
  representatives: t.representatives.map((m) => docIndex.get(m.id)).filter((x) => x !== undefined),
}));
const auditRow = rows["holdout-S8-positive"];
const auditPool = replyEvidence(auditRow);
const audit = auditRow.graph.items.filter((i) => i.type === "audit").map((a) => ({
  kind: a.kind,
  doc: a.node ? docIndex.get(a.node.id) ?? docOf(a.node.id) ?? null : null,
  detail: a.detail,
  evidence: a.evidence.map((id) => useEv(id, auditPool) ?? useEv(id)).filter(Boolean),
}));
const context = ["holdout-S3-positive", "holdout-S3-conditional", "dev-S3-positive"].map((id) => {
  const row = rows[id];
  const pool = replyEvidence(row);
  const task = gold.cases.find((c) => c.id === id);
  return {
    id,
    query: task?.query ?? id,
    profile: task?.request?.task_profile ?? "implementation",
    groups: row.graph.items.filter((i) => i.type === "role_group").map((g) => ({ role: g.role, evidence: g.evidence.map((x) => useEv(x, pool) ?? useEv(x)).filter(Boolean) })),
    missing: row.graph.missing_roles,
    used: row.graph.used_bytes,
    budget: task?.request?.token_budget ?? null,
    mode: row.graph.budget_mode,
  };
});

const fixture = {
  name: "bars-project",
  captured: "2026-10-20",
  method: meta.method,
  snapshot: meta.snapshot.slice(0, 16),
  semantic_state: aux.semantic_state,
  warnings: aux.warnings,
  note: "Замороженный корпус проверки качества GraphRAG 20-10-2026: настоящие документы, чанки, разделы, ссылки, близость чанков, сущности, утверждения и основания. Близость документов — максимум по парам чанков, k=5.",
  docs: sources.map((s, i) => [s.title, s.path, s.docs_path, s.module, (chunksOfDoc.get(i) ?? []).length]),
  chunks: chunks.map((c) => [c.doc, c.first, c.last, c.heading, c.roles]),
  links,
  chunkLinks: cLinks,
  docSims,
  chunkSims,
  entities,
  mentions,
  assertions,
  broken,
  topics,
  audit,
  context,
  evidence: Object.fromEntries(usedEvidence),
};
writeFileSync(OUT, JSON.stringify(fixture));
console.log(`docs ${fixture.docs.length}, chunks ${chunks.length}, links ${links.length}, chunkLinks ${cLinks.length}, docSims ${docSims.length}, chunkSims ${chunkSims.length}, entities ${entities.length}, mentions ${mentions.length}, assertions ${assertions.length}, broken ${broken.length}, topics ${topics.length}, audit ${audit.length}, context ${context.length}, evidence ${usedEvidence.size}`);
