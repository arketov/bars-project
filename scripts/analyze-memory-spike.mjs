import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import zlib from 'node:zlib';

const root = process.argv[2];
if (!root) throw new Error('usage: node analyze-memory-spike.mjs <evidence-dir>');
const summaries = [];
const rejected = [];
const readBytes = file => file.endsWith('.gz') ? zlib.gunzipSync(fs.readFileSync(file)) : fs.readFileSync(file);
const readJson = file => JSON.parse(readBytes(file).toString('utf8').replace(/^\uFEFF/,''));
for (const name of fs.readdirSync(root).sort()) {
  if (!/-(cpu|auto)-(embed|rerank|both|bounded)-\d+\.jsonl(?:\.gz)?$/.test(name)) continue;
  const bytes = readBytes(path.join(root, name));
  const rows = bytes.toString('utf8').trim().split('\n').map(line => JSON.parse(line));
  const config = rows.find(r => r.kind === 'config');
  const end = rows.at(-1);
  const processFile = path.join(root, name.replace(/\.jsonl(\.gz)?$/, '.process.json$1'));
  const processResult = fs.existsSync(processFile) ? readJson(processFile) : null;
  if (end.kind !== 'end' && end.kind !== 'abort' && processResult?.exit_code !== undefined && processResult.exit_code !== 0) {
    rejected.push({file:name,sha256:crypto.createHash('sha256').update(bytes).digest('hex'),reason:processResult,config:config?.detail});
    continue;
  }
  if (end.kind === 'abort' || (end.kind === 'end' && !end.detail.ok)) {
    rejected.push({file:name,sha256:crypto.createHash('sha256').update(bytes).digest('hex'),reason:end.detail,config:config?.detail});
    continue;
  }
  assert.equal(end.kind, 'end', `incomplete run: ${name}`);
  assert.equal(end.detail.ok, true, `failed run: ${name}`);
  const timestamp=name.replace(/^pair-/,'').split('-')[0];
  const invocationFile = path.join(root, `${name.startsWith('pair-')?'pair-':''}run-${timestamp}.json${name.endsWith('.gz')?'.gz':''}`);
  const invocation = fs.existsSync(invocationFile) ? readJson(invocationFile) : null;
  // The first frozen core runs predate process sidecars; retain that evidence with an explicit flag.
  if (processResult || (invocation && Object.hasOwn(invocation, 'memory_only'))) {
    assert.equal(processResult?.exit_code, 0, `missing or failed process result: ${name}`);
    assert.equal(processResult.timed_out, false, `process timed out: ${name}`);
  }
  assert(config, `missing config: ${name}`);
  assert(rows.every(r => Number.isFinite(r.memory.private_bytes) && r.memory.private_bytes > 0));
  const phases = Object.fromEntries(rows.filter(r => r.kind === 'plateau_end').map(r => [r.phase, r.memory]));
  const timings = rows.filter(r => r.kind === 'timing').map(r => ({ phase:r.phase, ...r.detail }));
  const after = timings.find(r => r.phase === 'rerank_after_reload');
  let scoreDelta = null;
  let warmScoreSpan = null;
  if (config.detail.mode !== 'embed') {
    const warm = timings.filter(r => r.phase === 'rerank');
    assert.equal(warm.length, 3);
    const before = warm.at(-1);
    assert.equal(before.scores.length, 15);
    assert.equal(after.scores.length, 15);
    assert([...before.scores, ...after.scores].every(Number.isFinite));
    scoreDelta = Math.max(...before.scores.map((score, i) => Math.abs(score - after.scores[i])));
    assert(scoreDelta <= 1e-5, `scores changed: ${name}`);
    assert(Math.abs(scoreDelta - after.max_score_delta) < 1e-6);
    warmScoreSpan = Math.max(...warm.flatMap(run => run.scores.map((score, i) => Math.abs(score-before.scores[i]))));
    assert(warmScoreSpan <= 1e-5, `unstable scores within session: ${name}`);
  }
  const samples = rows.filter(r => r.kind === 'sample');
  const gaps = samples.slice(1).map((r,i) => r.elapsed_ms - samples[i].elapsed_ms);
  const retained = phases.rerank_retained?.private_bytes;
  const dropped = phases.rerank_dropped?.private_bytes;
  const gpuFile = path.join(root, name.replace(/\.jsonl(\.gz)?$/, '.gpu.jsonl$1'));
  const gpuRows = fs.existsSync(gpuFile)
    ? readBytes(gpuFile).toString('utf8').trim().split('\n').map(line => JSON.parse(line)) : null;
  const gpuObserved = gpuRows?.filter(r => r.gpu.length);
  const gpuPeak = field => gpuObserved?.length
    ? Math.max(...gpuObserved.map(r => r.gpu.reduce((sum, adapter) => sum + adapter[field], 0))) : null;
  summaries.push({
    file:name, sha256:crypto.createHash('sha256').update(bytes).digest('hex'),
    config:config.detail, pid:config.pid, process_exit_verified:processResult?.exit_code === 0, samples:samples.length,
    wall_ms:end.elapsed_ms, max_sample_gap_ms:Math.max(...gaps),
    sampled_peak_private_bytes:Math.max(...rows.map(r => r.memory.private_bytes)),
    lifetime_peak_commit_bytes:end.memory.lifetime_peak_commit_bytes,
    min_system_available_bytes:Math.min(...rows.map(r => r.memory.system_available_bytes)),
    min_system_free_commit_bytes:Math.min(...rows.map(r => r.memory.system_commit_limit_bytes - r.memory.system_committed_bytes)),
    rerank_released_bytes:retained === undefined ? null : retained - dropped,
    max_score_delta:scoreDelta, warm_score_span:warmScoreSpan, phases, timings,
    external_monitor:gpuRows ? {
      samples:gpuRows.length, gpu_samples:gpuObserved.length,
      timing_valid:invocation?.memory_only !== true && gpuRows.every(r => r.foreign.length === 0),
      peak_dedicated_bytes:gpuPeak('DedicatedUsage'), peak_shared_bytes:gpuPeak('SharedUsage'),
      sha256:crypto.createHash('sha256').update(fs.readFileSync(gpuFile)).digest('hex'),
    } : null,
  });
}
assert(summaries.length, 'no completed runs');
fs.writeFileSync(path.join(root,'summary.json'), JSON.stringify({schema:1,runs:summaries,rejected_runs:rejected}, null, 2)+'\n');
for (const r of rejected) console.log(JSON.stringify({rejected:r}));
for (const r of summaries) {
  const mib = bytes => bytes === undefined || bytes === null ? null : +(bytes/1048576).toFixed(1);
  console.log(JSON.stringify({file:r.file, mode:r.config.mode, device:r.config.accelerator,
    plateau_mib:Object.fromEntries(Object.entries(r.phases).map(([p,m]) => [p,mib(m.private_bytes)])),
    peak_mib:mib(r.sampled_peak_private_bytes), released_mib:mib(r.rerank_released_bytes),delta:r.max_score_delta}));
}
