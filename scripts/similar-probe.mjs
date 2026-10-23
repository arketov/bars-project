// Задержка similar через настоящий CLI/IPC и выборка соседей из готового индекса bge-m3.
// node scripts/similar-probe.mjs <bars-cli.exe> <проект> <out.json>
// Поиск выбирает исходные разделы; замер similar включает запуск CLI, IPC и JSON, без embed.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import { performance } from 'node:perf_hooks';
const [cli, project, out] = process.argv.slice(2);
if (!cli || !project || !out) throw new Error('Нужны CLI, проект, файл результата');
const invoke = args => JSON.parse(execFileSync(cli, [...args, '--project', project, '--json'], { encoding: 'utf8', timeout: 90000, windowsHide: true }));
const queries = ['Как устроены поиск и индексация документации?', 'Как редактировать документ, использовать ссылки и граф?', 'Как выбираются модели и устройства выполнения?'];
const seeds = new Map();
for (const query of queries) for (const hit of invoke(['search', query, '--limit', '4']).results) seeds.set(hit.chunk_id, hit);
const ref = hit => ({ id: hit.chunk_id, path: hit.path, heading: hit.heading_chain, first: hit.first_line, last: hit.last_line, score: hit.score });
const samples = [], elapsed = [];
for (const hit of seeds.values()) {
  const times = []; let reply;
  for (let i = 0; i < 5; i++) {
    const start = performance.now(); reply = invoke(['similar', hit.chunk_id, '--limit', '5']);
    times.push(performance.now() - start);
  }
  elapsed.push(...times);
  samples.push({ source: ref(reply.source), neighbors: reply.results.map(ref), times_ms: times, indexing: reply.indexing, pending: reply.pending });
}
const sorted = elapsed.toSorted((a, b) => a - b);
const percentile = p => sorted[Math.max(0, Math.ceil(sorted.length * p) - 1)];
const thresholds = [0, 0.7, 0.8, 0.85, 0.9].map(threshold => ({ threshold,
  retained: samples.reduce((n, s) => n + s.neighbors.filter(h => h.score >= threshold).length, 0),
  empty_sources: samples.filter(s => !s.neighbors.some(h => h.score >= threshold)).length }));
const result = { date: new Date().toISOString(), project, cli, queries, sources: samples.length, calls: sorted.length,
  min_ms: sorted[0], median_ms: percentile(0.5), p95_ms: percentile(0.95), max_ms: sorted.at(-1), thresholds, samples };
fs.writeFileSync(out, JSON.stringify(result, null, 2) + '\n');
console.log(JSON.stringify({ ...result, samples: undefined, queries: undefined }, null, 2));
