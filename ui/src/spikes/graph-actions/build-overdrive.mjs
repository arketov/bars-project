// Read-only Markdown snapshot for the entry mockup; no model or user index.
import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve, relative, dirname } from 'node:path';
import { createHash } from 'node:crypto';

const root = resolve(process.argv[2] ?? '../cyberProject/ai-mutiplayer-bots/docs');
const walk = dir => readdirSync(dir, { withFileTypes: true }).flatMap(e => e.isDirectory() ? walk(resolve(dir, e.name)) : e.name.endsWith('.md') ? [resolve(dir, e.name)] : []);
// A bounded feature corpus, not a claim of the entire project's coverage.
const files = walk(resolve(root, 'features')).filter(p => !/[/\\]index\.md$/i.test(p)).sort();
const docs = [], chunks = [], evidence = {}, texts = [], hashes = [];
const paths = new Map(files.map((p, i) => [p.toLowerCase(), i]));
for (const [i, file] of files.entries()) {
  const text = readFileSync(file, 'utf8');
  const lines = text.split(/\r?\n/);
  const path = relative(root, file).replaceAll('\\', '/');
  const title = /^title:\s*["']?(.+?)["']?\s*$/m.exec(text)?.[1] ?? /^# (.+)$/m.exec(text)?.[1] ?? path;
  docs.push([title, `docs/${path}`, path, 'features', 0]);
  evidence[`doc-${i}`] = [i, 1, Math.min(lines.length, 24), lines.slice(0, 24).join('\n'), 'Markdown'];
  hashes.push([path, createHash('sha256').update(text).digest('hex')]);
  texts.push(lines);
}
const links = [], pairs = new Map();
texts.forEach((lines, from) => {
  let fenced = false;
  lines.forEach((line, n) => {
    if (/^\s*(```|~~~)/.test(line)) { fenced = !fenced; return; }
    if (fenced) return;
    for (const m of line.matchAll(/\[[^\]]+\]\(([^)]+)\)/g)) {
      const target = m[1].split('#')[0];
      if (!target || /^(https?:|mailto:)/.test(target)) continue;
      const to = paths.get(resolve(dirname(files[from]), decodeURIComponent(target)).toLowerCase());
      if (to === undefined || to === from) continue;
      const key = `${from}:${to}`;
      if (!pairs.has(key)) { const edge = [from, to, []]; pairs.set(key, edge); links.push(edge); }
      pairs.get(key)[2].push(n + 1);
      evidence[`link-${from}-${to}-${n + 1}`] = [from, n + 1, n + 1, line, 'Markdown'];
    }
  });
});
const fixture = {
  name: 'overdrive-bots · документы фич', captured: '26-10-2026', method: 'markdown-links-mockup-v1',
  snapshot: createHash('sha256').update(JSON.stringify(hashes)).digest('hex'), semantic_state: 'not_built',
  warnings: ['Только документы фич; близость и смысловые отношения не загружены'],
  docs, chunks, links, chunkLinks: [], docSims: [], chunkSims: [], entities: [], mentions: [], assertions: [], broken: [], topics: [], audit: [], context: [], evidence,
  provenance: { scope: 'features/**/*.md, кроме Index.md', sources: hashes, links: 'Относительные Markdown-ссылки внутри выбранного корпуса; это не полный индекс ссылок проекта' },
};
writeFileSync('ui/src/spikes/graph-actions/overdrive.json', JSON.stringify(fixture));
console.log(JSON.stringify({ documents: docs.length, links: links.length, snapshot: fixture.snapshot }));
