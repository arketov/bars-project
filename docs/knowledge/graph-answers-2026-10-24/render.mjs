// Черновые форматы ответа графа для исследования 4.3.1.
// Вход: замороженные DTO graph-quality-2026-10-20 (results.json.gz, gold-matched.json).
// Выход: drafts.json.gz ({формат: {случай: текст}}) и sizes.json рядом со скриптом.
// Запуск из корня репозитория: node docs/knowledge/graph-answers-2026-10-24/render.mjs
import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const src = path.join(here, "..", "graph-quality-2026-10-20");
const results = JSON.parse(zlib.gunzipSync(fs.readFileSync(path.join(src, "results.json.gz"))));
const gold = JSON.parse(fs.readFileSync(path.join(src, "gold-matched.json"))).cases;

const QUOTE_MAX = 300; // знаков цитаты в строке факта
const B_BUDGET = 16000; // предел текста варианта B, знаков

const OP = { relations: "Relations", paths: "Paths", impact: "Impact", context: "Context", explore: "Explore", graph: "Graph" };

// Текущий вывод bars-mcp: JSON одной строкой между границами (crates/bars-mcp/src/graph.rs).
function renderJ(dto) {
  return `=== GRAPH: ${OP[dto.operation] ?? dto.operation} ===\n${JSON.stringify(dto)}\n=== END GRAPH ===`;
}

function index(dto) {
  const docs = new Map(dto.sources.map((s) => [s.reference.id, s.docs_path]));
  const nodes = new Map(dto.nodes.map((n) => [n.reference.id, n]));
  const ev = new Map(dto.evidence.map((e) => [e.id, e]));
  return { docs, nodes, ev };
}

function quote(text) {
  const t = text.replace(/\s+/g, " ").trim();
  return t.length > QUOTE_MAX ? t.slice(0, QUOTE_MAX - 1) + "…" : t;
}

function where(ix, e) {
  const p = ix.docs.get(e.document.id) ?? "?";
  const { first_line: a, last_line: b } = e.span;
  return a === b ? `${p}:${a}` : `${p}:${a}-${b}`;
}

function evLine(ix, id, indent = "   ") {
  const e = ix.ev.get(id);
  if (!e) return `${indent}(evidence missing)`;
  return `${indent}${where(ix, e)} «${quote(e.quote)}»`;
}

function label(ix, ref) {
  const n = ix.nodes.get(ref.id ?? ref);
  if (!n) return ix.docs.get(ref.id ?? ref) ?? "?";
  return n.reference.kind === "document" ? ix.docs.get(n.reference.id) ?? n.label : `«${n.label}»`;
}

function nodePath(ix, ref, evIds) {
  if (ix.docs.has(ref.id)) return ix.docs.get(ref.id);
  const e = evIds?.length ? ix.ev.get(evIds[0]) : null;
  return e ? where(ix, e) : `${ref.kind}`;
}

// Факты операции: одна строка на элемент, основания строками ниже.
function facts(dto, ix) {
  const out = [];
  let n = 0;
  for (const it of dto.items) {
    n++;
    switch (it.type) {
      case "relation": {
        const a = it.assertion;
        const neg = a.polarity === "negative" ? "NOT " : "";
        const q = Object.entries(a.qualifiers ?? {}).map(([k, v]) => `${k}=${v}`).join(", ");
        out.push(`${n}. ${label(ix, a.from)} ${neg}${a.predicate} ${label(ix, a.to)}${q ? ` [${q}]` : ""} (${a.origin})`);
        for (const id of a.evidence) out.push(evLine(ix, id));
        break;
      }
      case "role_group": {
        out.push(`${n}. role ${it.role}: ${it.evidence.length} evidence`);
        for (const id of it.evidence) out.push(evLine(ix, id));
        break;
      }
      case "topic": {
        const reps = new Set((it.representatives ?? []).map((r) => r.id));
        const members = it.members.map((m) => (ix.docs.get(m.id) ?? m.kind) + (reps.has(m.id) ? "*" : ""));
        out.push(`${n}. topic (${it.members.length} docs, * = representative): ${members.join(", ")}`);
        break;
      }
      case "audit": {
        out.push(`${n}. ${it.kind}: ${nodePath(ix, it.node, it.evidence)} — ${it.detail}`);
        for (const id of it.evidence ?? []) out.push(evLine(ix, id));
        break;
      }
      default:
        out.push(`${n}. ${it.type}: ${JSON.stringify(it).slice(0, 200)}`);
    }
  }
  return out;
}

function candidates(dto, ix) {
  return (dto.candidates ?? []).map((c, i) => {
    const e = c.evidence?.length ? ix.ev.get(c.evidence[0]) : null;
    return `${i + 1}. «${c.label}» (${c.reference.kind})${e ? ` — ${where(ix, e)}` : ""}`;
  });
}

// Подсказка следующего шага для пустого ответа: по операции, без обещания полноты.
function nextStep(dto) {
  const op = dto.operation;
  if (dto.resolution === "not_found") return "Next: the name did not resolve; call bars_rag_search with the question, or retry with one of the candidates.";
  if (op === "impact") return "Next: no documented dependencies were extracted; links are not dependencies. Search the changed subject with bars_rag_search and read the hits.";
  if (op === "paths") return "Next: no path within the limits; check both ends with bars_relations or read both documents.";
  if (op === "explore") return "Next: no candidates found; this is not proof of absence. Search the topic with bars_rag_search.";
  return "Next: call bars_rag_search with the same question.";
}

function header(dto, total) {
  return `=== GRAPH ${dto.operation}: ${total} results ===`;
}

// Вариант A: только факты с основанием в строке, без состояния.
function renderA(dto) {
  const ix = index(dto);
  const lines = [header(dto, dto.items.length)];
  if (dto.items.length) lines.push(...facts(dto, ix));
  else if (dto.candidates?.length) lines.push("Name not resolved; candidates:", ...candidates(dto, ix));
  else lines.push("No results.");
  lines.push("=== END GRAPH ===");
  return lines.join("\n");
}

// Вариант B: A + строка состояния, усечение с продолжением, пустой ответ со следующим шагом, предел текста.
function renderB(dto) {
  const ix = index(dto);
  const status = [];
  const ls = dto.layer_states ?? {};
  status.push(`layers: ${Object.entries(ls).map(([k, v]) => `${k} ${v}`).join(", ") || dto.layer_state}`);
  if (dto.missing_roles?.length) status.push(`missing roles: ${dto.missing_roles.join(", ")}`);
  if (dto.truncated) status.push(`truncated by ${dto.limit_reasons.join(", ") || "limit"}${dto.next_cursor ? "; continue with cursor" : "; narrow modules/files or raise limit"}`);
  const lines = [header(dto, dto.items.length), status.join("; ")];
  if (dto.items.length) {
    const body = facts(dto, ix);
    let used = lines.join("\n").length;
    let shown = 0;
    for (const l of body) {
      if (used + l.length + 1 > B_BUDGET) break;
      lines.push(l);
      used += l.length + 1;
      shown++;
    }
    if (shown < body.length) {
      const left = dto.items.length - body.slice(0, shown).filter((l) => /^\d+\./.test(l)).length;
      lines.push(`… ${left} more results not shown (text limit ${B_BUDGET} chars); narrow modules/files or lower limit.`);
    }
  } else if (dto.candidates?.length) {
    lines.push("Name not resolved; candidates:", ...candidates(dto, ix));
    lines.push(nextStep(dto));
  } else {
    lines.push("No results in scope.", nextStep(dto));
  }
  lines.push("Note: semantic layer is partial; a missing relation does not prove its absence.");
  lines.push("=== END GRAPH ===");
  return lines.join("\n");
}

// Вариант C: компактный JSON без эха запроса и хешей, основание внутри элемента.
function renderC(dto) {
  const ix = index(dto);
  const evOut = (id) => {
    const e = ix.ev.get(id);
    return e ? { at: where(ix, e), quote: quote(e.quote) } : null;
  };
  const items = dto.items.map((it) => {
    if (it.type === "relation") {
      const a = it.assertion;
      return { type: "relation", from: label(ix, a.from), predicate: a.predicate, polarity: a.polarity, to: label(ix, a.to), qualifiers: a.qualifiers, evidence: a.evidence.map(evOut) };
    }
    if (it.type === "role_group") return { type: "role_group", role: it.role, evidence: it.evidence.map(evOut) };
    if (it.type === "topic") return { type: "topic", members: it.members.map((m) => ix.docs.get(m.id)), representatives: (it.representatives ?? []).map((m) => ix.docs.get(m.id)) };
    if (it.type === "audit") return { type: "audit", kind: it.kind, at: nodePath(ix, it.node, it.evidence), detail: it.detail, evidence: (it.evidence ?? []).map(evOut) };
    return it;
  });
  const out = { operation: dto.operation, layers: dto.layer_states, truncated: dto.truncated, limit_reasons: dto.limit_reasons, next_cursor: dto.next_cursor, missing_roles: dto.missing_roles, resolution: dto.resolution, items };
  if (dto.candidates?.length) out.candidates = dto.candidates.map((c) => c.label);
  return `=== GRAPH ${dto.operation} ===\n${JSON.stringify(out)}\n=== END GRAPH ===`;
}

// Mix: основные совпадения поиска плюс связанные пакеты.
function hitsText(v) {
  return v.hits.map((h, i) => `${i + 1}. ${h.path.replace(/^docs\//, "")}:${h.first_line}-${h.last_line}\n${h.text.trim()}`).join("\n");
}
function mixJ(v) {
  return `=== SEARCH mix ===\n${JSON.stringify({ hits: v.hits, related: v.related })}\n=== END SEARCH ===`;
}
function mixWith(v, render) {
  return `=== SEARCH mix: ${v.hits.length} results ===\n${hitsText(v)}\n=== RELATED ===\n${render(v.related)}\n=== END SEARCH ===`;
}

const formats = { J: renderJ, A: renderA, B: renderB, C: renderC };
const cases = [];
for (const row of results.rows) {
  const g = gold.find((c) => c.id === row.id);
  cases.push({ id: row.id, scenario: g.scenario, operation: row.graph.operation, query: g.query, dto: row.graph });
}
// Аналог З10 аудита 23-10: mix на вопрос dev-S1-positive.
const mixRow = results.rows.find((r) => r.id === "dev-S1-positive").variants.find((v) => v.name === "mix");

const sizes = [];
const bytes = (s) => Buffer.byteLength(s, "utf8");
const drafts = Object.fromEntries(Object.keys(formats).map((f) => [f, {}]));
for (const c of cases) {
  const row = { id: c.id, scenario: c.scenario, operation: c.operation, items: c.dto.items.length };
  for (const [f, fn] of Object.entries(formats)) {
    const text = fn(c.dto);
    drafts[f][c.id] = text;
    row[f] = { chars: text.length, bytes: bytes(text) };
  }
  sizes.push(row);
}
{
  const row = { id: "audit-Z10-mix", scenario: "mix", operation: "search", items: mixRow.hits.length };
  const mixFormats = { J: mixJ, A: (v) => mixWith(v, renderA), B: (v) => mixWith(v, renderB), C: (v) => mixWith(v, renderC) };
  for (const [f, fn] of Object.entries(mixFormats)) {
    const text = fn(mixRow);
    drafts[f][row.id] = text;
    row[f] = { chars: text.length, bytes: bytes(text) };
  }
  sizes.push(row);
}
fs.writeFileSync(path.join(here, "drafts.json.gz"), zlib.gzipSync(JSON.stringify(drafts), { level: 9 }));
fs.writeFileSync(path.join(here, "sizes.json"), JSON.stringify(sizes, null, 1) + "\n");

const sum = (f, k) => sizes.reduce((a, r) => a + r[f][k], 0);
const med = (f) => {
  const v = sizes.map((r) => r[f].chars).sort((a, b) => a - b);
  return v[Math.floor(v.length / 2)];
};
const max = (f) => Math.max(...sizes.map((r) => r[f].chars));
for (const f of Object.keys(formats)) console.log(f, "sum chars", sum(f, "chars"), "median", med(f), "max", max(f), "est tokens (bytes/3)", Math.round(sum(f, "bytes") / 3));
