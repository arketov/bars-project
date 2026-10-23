// Архивный replay 4.3.1: модели и удалённый spikes/inference не требуются.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {quantile} from './analyze-inference.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));
const evidence = path.join(root, 'docs/knowledge/inference-2026-10-18');
const read = p => JSON.parse(fs.readFileSync(p, 'utf8').replace(/^\uFEFF/, ''));
const sha256 = p => crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const output = process.argv[2];
assert(output && process.argv.length === 3, 'Использование: node scripts/replay-rerank-evidence.mjs <новый-output.json>');
assert(!fs.existsSync(output), 'Результат уже существует; выберите новый файл');
const checksums = read(path.join(evidence, 'checksums.json'));
for (const [name, expected] of Object.entries(checksums)) {
  assert.equal(path.basename(name), name, 'Имя доказательства выходит из папки');
  assert.equal(sha256(path.join(evidence, name)), expected, `SHA-256: ${name}`);
}

function archiveText(archive, member) {
  // Чтение stdout tar; ничего не распаковывается на диск.
  assert(!member.startsWith('/') && !member.split('/').includes('..'));
  return execFileSync('tar', ['-xOf', path.join(evidence, archive), member], {
    encoding: 'utf8', maxBuffer: 32 * 1024 * 1024,
  });
}
const archiveJson = (archive, member) => JSON.parse(archiveText(archive, member));
function replay(archive, saved) {
  assert(saved.dir.startsWith('spikes/inference/runs/'));
  const member = saved.dir.slice('spikes/inference/runs/'.length);
  const cfg = archiveJson(archive, `${member}/config.json`);
  assert.deepEqual(cfg.config ?? cfg, saved.config, `Конфигурация: ${member}`);
  const complete = archiveJson(archive, `${member}/complete.json`);
  const rows = archiveText(archive, `${member}/rows.jsonl`).trim().split(/\r?\n/).map(JSON.parse);
  const monitor = archiveJson(archive, `${member}.monitor.json`);
  assert.equal(rows.length, saved.n, `Число строк: ${member}`);
  if (complete.observations !== undefined) assert.equal(rows.length, complete.observations);
  assert(complete.rows_blake3, `Нет отметки завершения: ${member}`);
  assert.equal(monitor.exit_code, 0, `Процесс не завершён успешно: ${member}`);
  assert(!monitor.quality_rejected && !monitor.quality_only, `Режим quality-only: ${member}`);
  assert.equal(!monitor.timing_contaminated, saved.timingValid, `Монитор: ${member}`);
  const seen = new Set();
  for (const row of rows) {
    const key = `${row.id}:${row.repeat}`;
    assert(!seen.has(key), `Повтор строки: ${member}/${key}`); seen.add(key);
    assert(Number.isInteger(row.repeat) && row.repeat >= 0 && row.repeat < saved.config.repeats);
    assert(row.seconds > 0 && Number.isFinite(row.seconds));
    assert(Array.isArray(row.hits) && row.hits.length);
    assert.equal(new Set(row.hits.map(h => h.id)).size, row.hits.length);
    assert(row.hits.every(h => Number.isFinite(h.score) && h.score >= 0 && h.score <= 1));
    if (row.logits) {
      assert.equal(row.logits.length, row.hits.length);
      assert(row.logits.every(Number.isFinite));
    }
  }
  const ids = [...new Set(rows.map(r => r.id))];
  assert.equal(rows.length, ids.length * saved.config.repeats, `Неполные повторы: ${member}`);
  if (saved.config.ids.length) assert.deepEqual([...ids].sort(), [...saved.config.ids].sort());
  const median = quantile(rows.map(r => r.seconds), .5);
  const p95 = quantile(rows.map(r => r.seconds), .95);
  assert.equal(median, saved.median, `Median: ${member}`);
  assert.equal(p95, saved.p95, `p95: ${member}`);
  return {name: saved.config.name, archive, member, n: rows.length, questions: ids.length,
    device: saved.config.device, threads: saved.config.threads, batch: saved.config.batch,
    max_length: saved.config.max_length, median, p95, timingValid: saved.timingValid,
    qualitySource: 'saved summary; gold inputs not reconstructed',
    savedLossIds: [...new Set((saved.losses ?? []).map(l => l.id))]};
}

const series = [
  ['gpu-screen.json', 'gpu-screen.tar.gz'],
  ['gpu-combos.json', 'gpu-combos.tar.gz'],
  ['cpu-pilot.json', 'cpu-pilot.tar.gz'],
];
const runs = [];
for (const [summary, archive] of series) {
  for (const run of read(path.join(evidence, summary)).runs) runs.push(replay(archive, run));
}
for (const dataset of read(path.join(evidence, 'full-retrieval.json')).datasets) {
  for (const run of dataset.runs) runs.push(replay('full-retrieval.tar.gz', run));
}
const reduction = read(path.join(evidence, 'work-reduction.json'));
const result = {
  kind: 'archived evidence replay; no new model execution',
  checkedSha256Files: Object.keys(checksums).length,
  checksumManifestSha256: sha256(path.join(evidence, 'checksums.json')),
  runs, observations: runs.reduce((n, r) => n + r.n, 0),
  candidateReduction: {
    source: 'work-reduction.json; saved offline quality simulation, latency unmeasured',
    variants: reduction.variants.map(v => ({spec: v.spec, meanPairs: v.meanPairs,
      savedLossIds: [...new Set(v.losses.map(l => l.id))]})),
  },
};
fs.writeFileSync(output, `${JSON.stringify(result, null, 2)}\n`, {flag: 'wx'});
console.log(`Проверено SHA-256: ${result.checkedSha256Files}; серий: ${runs.length}; строк: ${result.observations}`);
