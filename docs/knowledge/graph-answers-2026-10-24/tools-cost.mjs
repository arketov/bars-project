// Стоимость списка инструментов bars-mcp для исследования 4.3.1.
// Вход: tools-list.json — ответ tools/list собранного bars-mcp. Выход: печать сводки.
// Запуск из корня репозитория: node docs/knowledge/graph-answers-2026-10-24/tools-cost.mjs
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const tools = JSON.parse(fs.readFileSync(path.join(here, "tools-list.json"))).tools;
const GRAPH = ["bars_relations", "bars_paths", "bars_impact", "bars_context", "bars_explore", "bars_graph"];
const COMMON = ["project", "format", "modules", "files", "limit", "cursor"];
// Поля операции по tools.md «Фактический контракт v1».
const OWN = {
  bars_relations: ["query", "entity_ref", "predicates", "direction", "polarity", "qualifiers"],
  bars_paths: ["from", "to", "predicates", "path_kind", "max_hops", "max_paths"],
  bars_impact: ["roots", "predicates"],
  bars_context: ["query", "task_profile", "roles", "token_budget"],
  bars_explore: ["topic", "whole_scope", "analysis", "grouping"],
  bars_graph: ["view", "local", "roots", "include_paths", "predicates", "direction", "budgets"],
};
const bytes = (o) => Buffer.byteLength(JSON.stringify(o), "utf8");

function narrow(tool) {
  const keep = new Set([...COMMON, ...OWN[tool.name]]);
  const s = structuredClone(tool.inputSchema);
  s.properties = Object.fromEntries(Object.entries(s.properties).filter(([k]) => keep.has(k)));
  // Оставить только $defs, на которые есть ссылки (с транзитивным замыканием).
  const used = new Set();
  const walk = (v) => {
    if (Array.isArray(v)) return v.forEach(walk);
    if (v && typeof v === "object") for (const [k, x] of Object.entries(v)) {
      if (k === "$ref" && typeof x === "string") {
        const d = x.split("/").pop();
        if (!used.has(d)) { used.add(d); walk(s.$defs?.[d]); }
      } else walk(x);
    }
  };
  walk(s.properties);
  if (s.$defs) s.$defs = Object.fromEntries(Object.entries(s.$defs).filter(([k]) => used.has(k)));
  if (s.required) s.required = s.required.filter((k) => keep.has(k));
  return { ...tool, inputSchema: s };
}

const all = bytes(tools);
const graph = tools.filter((t) => GRAPH.includes(t.name));
const narrowed = graph.map(narrow);
const rest = tools.filter((t) => !GRAPH.includes(t.name));
const row = (name, b, n) => console.log(`${name.padEnd(34)} tools ${String(n).padStart(2)}  bytes ${String(b).padStart(6)}  est tokens ${String(Math.round(b / 3)).padStart(6)}`);
row("current list", all, tools.length);
row("  of them six graph tools", bytes(graph), graph.length);
row("per-operation graph schemas", bytes(rest) + bytes(narrowed), tools.length);
row("  of them six graph tools", bytes(narrowed), narrowed.length);
row("graph tools hidden (OQ-162)", bytes(rest), rest.length);
for (const t of narrowed) console.log(`  ${t.name.padEnd(16)} props ${Object.keys(t.inputSchema.properties).length}  bytes ${bytes(t)}`);
