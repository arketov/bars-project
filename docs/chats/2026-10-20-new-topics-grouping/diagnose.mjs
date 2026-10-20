// Read-only investigation of ISS-19; no model loading or changes to the live store.
import fs from 'node:fs';
import zlib from 'node:zlib';
import crypto from 'node:crypto';
import { DatabaseSync } from 'node:sqlite';
import { guardDiffers } from '../../../scripts/new-topics-threshold.mjs';

const root = 'C:/Users/arketov/Desktop/cyberProject/ai-mutiplayer-bots';
const texts = [
  'Аудит журналов возврата в боксы и предыдущей попытки входа результаты сессий',
  'журналы возврата боксы',
];
const db = new DatabaseSync(`${root}/.bars/new-topics.sqlite3`, { readOnly: true });
const rows = texts.map(text => db.prepare('SELECT * FROM variants WHERE text=? ORDER BY id LIMIT 1').get(text));
if (rows.some(row => !row)) throw Error('Original variants not found');
const observations = rows.map(row => db.prepare('SELECT source,reason,at,params,index_stamp FROM observations WHERE variant_id=?').all(row.id));
db.close();
function decode(bytes, offset = 0, count = (bytes.byteLength - offset) / 4) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  return Array.from({ length: count }, (_, i) => view.getFloat32(offset + i * 4, true));
}
const dot = (a, b) => a.reduce((sum, value, i) => sum + value * b[i], 0);
const vectors = rows.map(row => decode(row.vector));
const norms = vectors.map(v => Math.sqrt(dot(v, v)));
const similarity = dot(...vectors);
let rustDot = 0;
for (let i = 0; i < vectors[0].length; i++) rustDot = Math.fround(rustDot + Math.fround(vectors[0][i] * vectors[1][i]));
const pair = {
  root, variants: rows.map(({ vector, ...row }, i) => ({ ...row,
    vector_bytes: vector.length, vector_sha256: crypto.createHash('sha256').update(vector).digest('hex'),
    norm: norms[i], observations: observations[i] })),
  dot: similarity, rust_f32_dot: rustDot, cosine: similarity / (norms[0] * norms[1]),
  same_scope: rows[0].scope === rows[1].scope,
  same_space: rows[0].space === rows[1].space,
  lexical_differences: guardDiffers(...texts), threshold: 0.79,
  merge: rustDot >= 0.79 && !guardDiffers(...texts).length,
};

const base = 'docs/knowledge/new-topics-threshold-2026-10-19';
const { items, guards } = JSON.parse(fs.readFileSync(`${base}/items.json`, 'utf8'));
const bytes = zlib.gunzipSync(fs.readFileSync(`${base}/vectors-auto.bin.gz`));
if (bytes.toString('ascii', 0, 4) !== 'BGV1') throw Error('Unexpected vector format');
const count = bytes.readUInt32LE(4), dim = bytes.readUInt32LE(8);
if (count !== items.length || bytes.length !== 12 + count * dim * 4) throw Error('Invalid vector size');
const calibrationVectors = items.map((_, i) => decode(bytes, 12 + i * dim * 4, dim));
const trapGroups = new Set();
for (const g of guards) {
  const a = items[g.a].group, b = items[g.b].group;
  trapGroups.add(`${a}|${b}`); trapGroups.add(`${b}|${a}`);
}
const thresholds = [0.79, 0.72, 0.7104, 0.71, 0.70, 0.68];
const calibration = thresholds.map(threshold => {
  const result = { threshold, same: 0, merged_same: 0, keywords: 0, merged_keywords: 0,
    ordinary_different: 0, false_ordinary_after_guards: [], traps: 0, false_traps_after_guards: 0 };
  for (let i = 0; i < count; i++) for (let j = i + 1; j < count; j++) {
    const a = items[i], b = items[j];
    if (a.split !== b.split) continue;
    const trap = trapGroups.has(`${a.group}|${b.group}`);
    const same = !trap && a.group === b.group;
    const keywords = same && (a.kind === 'keywords' || b.kind === 'keywords');
    const cosine = dot(calibrationVectors[i], calibrationVectors[j]);
    const merge = cosine >= threshold && !guardDiffers(a.text, b.text).length;
    if (same) { result.same++; result.merged_same += +merge; }
    if (keywords) { result.keywords++; result.merged_keywords += +merge; }
    if (trap) { result.traps++; result.false_traps_after_guards += +merge; }
    if (!same && !trap) {
      result.ordinary_different++;
      if (merge) result.false_ordinary_after_guards.push({ a: a.text, b: b.text, cosine });
    }
  }
  return result;
});
const policy = fs.readFileSync('crates/bars-core/src/new_topics/store.rs', 'utf8');
const currentThreshold = Number(policy.match(/model\.contains\("bge-m3"\)\.then_some\(([\d.]+)\)/)?.[1]);
if (!Number.isFinite(currentThreshold)) throw Error('Current threshold not found');
console.log(JSON.stringify({ captured_at: new Date().toISOString(), pair,
  current_policy: { threshold: currentThreshold,
    merge: rustDot >= currentThreshold && !guardDiffers(...texts).length }, calibration }, null, 2));
