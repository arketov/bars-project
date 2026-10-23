import fs from 'node:fs';
import path from 'node:path';
import {gunzipSync} from 'node:zlib';
import {pathToFileURL} from 'node:url';

export function summarize(rows, mode, precision) {
  const one = phase => {
    const found = rows.filter(r => r.phase === phase && r.kind !== 'sample');
    if (found.length !== 1) throw Error(`Expected one ${phase}`);
    return found[0];
  };
  if (!one('end').detail.ok) throw Error('Incomplete lifecycle');
  const runtime = one('warm').detail;
  if (runtime.embed.actual !== 'directml' || runtime.embed.precision !== precision || runtime.rerank.actual !== 'directml' || runtime.rerank.precision !== 'fp16') throw Error('Unexpected runtime');
  const start = one('reindex_start'), done = one('reindex_done'), queriesStart = one('queries_start');
  if (start.detail.mode !== mode || done.elapsed_ms < start.elapsed_ms) throw Error('Invalid indexing interval');
  if (mode === 'serial' && queriesStart.elapsed_ms < done.elapsed_ms) throw Error('Serial queries overlap indexing');
  const queries = rows.filter(r => r.kind === 'search');
  if (queries.length !== 100 || queries.some(r => !r.detail.ok || !r.detail.stable || r.detail.rerank_error || r.detail.score_kind !== 'rerank' || !Number.isFinite(r.detail.ms) || r.detail.ms <= 0)) throw Error('Missing, changed or failed queries');
  const overlapping = queries.filter(r => r.detail.indexing).length;
  if ((mode === 'serial' && overlapping !== 0) || (mode === 'overlap' && overlapping !== 100)) throw Error('Unexpected query/index overlap');
  if (queries.some(r => r.elapsed_ms < queriesStart.elapsed_ms || (mode === 'serial' && r.phase !== 'after_index') || (mode === 'overlap' && r.phase !== 'during_index'))) throw Error('Query phase mismatch');
  const peaks = rows.map(r => r.memory?.lifetime_peak_commit_bytes).filter(v => v !== undefined);
  if (!peaks.length || peaks.some(v => !Number.isFinite(v) || v <= 0)) throw Error('Missing memory counters');
  const ms = queries.map(r => r.detail.ms).sort((a,b) => a-b);
  return {mode, precision, queries:100, overlapping, private_peak_bytes:Math.max(...peaks), median_ms:(ms[49]+ms[50])/2, p95_ms:ms[94], reindex_ms:done.detail.ms};
}

export function compare(directory) {
  const read = name => JSON.parse(fs.readFileSync(path.join(directory,name),'utf8'));
  if (!read('result.json').ok) throw Error('Runner did not complete both modes');
  const provenance = read('provenance.json');
  if (JSON.stringify(read('serial-corpus.json')) !== JSON.stringify(read('overlap-corpus.json'))) throw Error('Corpora differ');
  const summaries = ['serial','overlap'].map(mode => {
    if (read(`${mode}-process.json`).exit_code !== 0) throw Error('Child failed');
    const rows = gunzipSync(fs.readFileSync(path.join(directory,`${mode}-memory.jsonl.gz`))).toString('utf8').trim().split(/\r?\n/).map(JSON.parse);
    return summarize(rows,mode,provenance.precision);
  });
  return {provenance, summaries, overlap_minus_serial_peak_bytes:summaries[1].private_peak_bytes-summaries[0].private_peak_bytes, limitation:'One process per mode under shared system load; no speed or universal memory-budget acceptance'};
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const result = compare(process.argv[2]);
  fs.writeFileSync(process.argv[3],JSON.stringify(result,null,2)+'\n',{flag:'wx'});
}
