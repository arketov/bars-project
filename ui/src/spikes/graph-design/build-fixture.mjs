// Собирает fixture.json стенда дизайна графа из сохранённых снимков исследования 2.1.1 (только bars-project).
// Запуск из корня репозитория: node ui/src/spikes/graph-design/build-fixture.mjs
// Снимок хранит названия, а не пути: путь и модуль восстанавливаются по title из frontmatter текущих документов.
import { readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { gunzipSync } from "node:zlib";
import { join, relative } from "node:path";

const root = process.cwd();
const snapshot = JSON.parse(
  gunzipSync(readFileSync(join(root, "docs/chats/2026-10-21-graph-physics/run-3/snapshots.json.gz"))),
)[0];
const [docView, chunkView] = Object.values(snapshot.views);

function walk(dir, out) {
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    if (name === "node_modules" || name === "target" || name.startsWith(".")) continue;
    if (statSync(full).isDirectory()) walk(full, out);
    else if (name.endsWith(".md")) out.push(full);
  }
  return out;
}
const titles = new Map();
for (const file of walk(join(root, "docs"), []).concat(readdirSync(root).filter((n) => n.endsWith(".md")).map((n) => join(root, n)))) {
  const text = readFileSync(file, "utf8");
  const title = /^title:\s*"?(.+?)"?\s*$/m.exec(text.slice(0, 600))?.[1] ?? /^#\s+(.+)$/m.exec(text)?.[1];
  if (!title) continue;
  const rel = relative(root, file).replaceAll("\\", "/");
  titles.set(title, [...(titles.get(title) ?? []), rel]);
}

const docIndex = new Map();
const docs = docView.nodes.map((node, i) => {
  docIndex.set(node.id, i);
  const paths = titles.get(node.label) ?? [];
  const path = paths.length === 1 ? paths[0] : null; // неоднозначные названия остаются без пути
  const parts = path?.split("/") ?? [];
  const module = !path ? "?" : parts[0] === "docs" && parts.length > 2 ? parts[1] : ".";
  return [node.label, path, module];
});

const chunkIndex = new Map();
const chunks = [];
for (const node of chunkView.nodes.filter((n) => n.kind === "chunk")) {
  chunkIndex.set(node.id, chunks.length);
  chunks.push([node.label, -1]);
}
for (const edge of chunkView.edges.filter((e) => e.predicate === "contains")) {
  // В снимке `contains` записан в обоих направлениях: документ→чанк и чанк→документ.
  const owner = docIndex.get(edge.source) ?? docIndex.get(edge.target);
  const chunk = chunkIndex.get(edge.target) ?? chunkIndex.get(edge.source);
  if (owner !== undefined && chunk !== undefined) chunks[chunk][1] = owner;
}

const pair = (map, a, b, w) => {
  if (a === b || a === undefined || b === undefined) return;
  const key = a < b ? `${a}:${b}` : `${b}:${a}`;
  map.set(key, Math.max(map.get(key) ?? 0, w));
};
// Ссылки — настоящие: Markdown-ссылки из текущих файлов, направление от файла-источника к цели, номера строк сохранены.
// Близость взята из снимка: он не хранит ни направления ссылок, ни их строк.
const pathIndex = new Map();
docs.forEach(([, path], i) => path && pathIndex.set(path, i));
const links = new Map();
const dsim = new Map();
const { posix } = await import("node:path");
docs.forEach(([, path], from) => {
  if (!path) return;
  const lines = readFileSync(join(root, path), "utf8").split(/\r?\n/);
  let fenced = false;
  lines.forEach((line, n) => {
    if (/^\s*(```|~~~)/.test(line)) fenced = !fenced;
    if (fenced) return;
    for (const m of line.matchAll(/\]\(([^)\s#]+\.md)(?:#[^)]*)?\)/g)) {
      if (/^[a-z]+:/i.test(m[1])) continue;
      const target = posix.normalize(posix.join(posix.dirname(path), decodeURI(m[1])));
      const to = pathIndex.get(target);
      if (to === undefined || to === from) continue;
      const key = `${from}:${to}`;
      links.set(key, [from, to, [...(links.get(key)?.[2] ?? []), n + 1]]);
    }
  });
});
for (const e of docView.edges) {
  const a = docIndex.get(e.source), b = docIndex.get(e.target);
  if (e.predicate === "similar_to") pair(dsim, a, b, e.weight);
}
const csim = new Map();
for (const e of chunkView.edges) if (e.predicate === "similar_to") pair(csim, chunkIndex.get(e.source), chunkIndex.get(e.target), e.weight);
const unpack = (map) => [...map].map(([key, w]) => [...key.split(":").map(Number), Math.round(w * 1000) / 1000]);

const fixture = {
  name: "bars-project",
  captured: snapshot.manifest.capturedAt.slice(0, 10),
  note: "Ссылки — настоящие Markdown-ссылки текущих файлов; близость — из снимка исследования 2.1.1 (максимум по чанкам kNN, не centroid); чанки без диапазонов.",
  docs,
  chunks,
  links: [...links.values()],
  dsim: unpack(dsim),
  csim: unpack(csim),
};
writeFileSync(join(root, "ui/src/spikes/graph-design/fixture.json"), JSON.stringify(fixture));
const matched = docs.filter((d) => d[1]).length;
console.log({ docs: docs.length, matched, orphanChunks: chunks.filter((c) => c[1] < 0).length, chunks: chunks.length, links: fixture.links.length, dsim: fixture.dsim.length, csim: fixture.csim.length });
