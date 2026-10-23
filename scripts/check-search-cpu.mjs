// Проверки снимка и защит CPU-стенда; модели не загружаются.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import zlib from 'node:zlib';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {DatabaseSync} from 'node:sqlite';

const [exe, evidence, output] = process.argv.slice(2);
if (!exe || !evidence || !output) throw Error('check-search-cpu <exe> <evidence-dir> <new-result.json>');
if (fs.existsSync(output)) throw Error('Результат уже существует');
const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'bars-search-cpu-check-'));
const checks = [];
const sha = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
function invoke(name, args, success, message) {
  const child = spawnSync(exe, args, {encoding: 'utf8', windowsHide: true, timeout: 60000});
  const ok = !child.error && (success ? child.status === 0 : child.status !== 0)
    && (!message || child.stderr.includes(message));
  checks.push({name, ok, exit: child.status, stderr: child.stderr?.trim(), error: child.error?.message});
  if (!ok) throw Error(`Не пройдена проверка: ${name}`);
}
function changed(name, change, message) {
  const copy = structuredClone(corpus);
  change(copy);
  const file = path.join(temp, `${name}.json`);
  fs.writeFileSync(file, JSON.stringify(copy));
  invoke(name, ['validate', file], false, message);
}
let corpus;
try {
  const bytes = zlib.gunzipSync(fs.readFileSync(path.join(evidence, 'corpus.json.gz')));
  corpus = JSON.parse(bytes);
  const frozen = path.join(temp, 'corpus.json');
  fs.writeFileSync(frozen, bytes);
  invoke('frozen-corpus', ['validate', frozen], true);
  changed('tampered-source', c => { c.documents[0].source += '\nподмена'; }, 'повреждён снимок');
  changed('insufficient-count', c => { c.selected = 4999; }, 'ровно 5000');
  changed('duplicate-path', c => { c.documents.push(structuredClone(c.documents[0])); }, 'повторён путь');
  changed('out-of-range', c => { c.documents[0].selected = 1000000; }, 'неверное число');
  const seed = path.join(temp, 'seed.sqlite');
  fs.writeFileSync(seed, zlib.gunzipSync(fs.readFileSync(path.join(evidence, 'seed-vectors.sqlite.gz'))));
  const inputs = JSON.parse(fs.readFileSync(path.join(evidence, 'inputs.json')));
  inputs.seeds = [seed];
  const cfg = path.join(temp, 'inputs.json');
  fs.writeFileSync(cfg, JSON.stringify(inputs));
  const planPath = path.join(temp, 'plan.json');
  invoke('compatible-cache', ['cache-plan', frozen, cfg, planPath], true);
  const planBytes = fs.readFileSync(planPath);
  const plan = JSON.parse(planBytes);
  const chunks = plan.documents.flatMap(d => d.chunks);
  const valid = plan.chunks === 5000 && chunks.length === 5000
    && new Set(chunks.map(c => c.embed_hash)).size === 5000
    && plan.reused + plan.pending.length === 5000;
  checks.push({name: 'unique-natural-chunks-and-cache-accounting', ok: valid});
  if (!valid) throw Error('Нарушен состав корпуса');
  invoke('refuse-output-overwrite', ['cache-plan', frozen, cfg, planPath], false);
  const unchanged = sha(fs.readFileSync(planPath)) === sha(planBytes);
  checks.push({name: 'existing-output-unchanged', ok: unchanged});
  if (!unchanged) throw Error('Стенд изменил существующий результат');
  const badSeed = path.join(temp, 'bad-seed.sqlite');
  fs.copyFileSync(seed, badSeed);
  const db = new DatabaseSync(badSeed);
  db.prepare('UPDATE meta SET value=? WHERE key=?').run('2560', 'model_dim');
  db.close();
  inputs.seeds = [badSeed];
  fs.writeFileSync(cfg, JSON.stringify(inputs));
  invoke('reject-incompatible-model', ['cache-plan', frozen, cfg, path.join(temp, 'bad-plan.json')], false, 'несовместимая модель');
  const forbidden = fs.existsSync(path.join(temp, 'bad-plan.json'));
  checks.push({name: 'no-plan-after-rejected-model', ok: !forbidden});
  if (forbidden) throw Error('Стенд сохранил ложную сводку');
  fs.writeFileSync(output, JSON.stringify({
    status: 'passed-no-model', checks, chunks: plan.chunks, documents: plan.documents.length,
    reused: plan.reused, pending: plan.pending.length, queries: inputs.queries.length,
    corpusSha256: sha(bytes), exeSha256: sha(fs.readFileSync(exe)),
    limitation: 'No model inference, full index, latency or IPC acceptance in this check',
  }, null, 2) + '\n', {flag: 'wx'});
  console.log(JSON.stringify({checks: checks.length, chunks: plan.chunks, reused: plan.reused, pending: plan.pending.length}));
} finally {
  const resolved = path.resolve(temp);
  if (path.dirname(resolved) !== path.resolve(os.tmpdir()) || !path.basename(resolved).startsWith('bars-search-cpu-check-')) {
    throw Error('Неверный каталог уборки');
  }
  fs.rmSync(resolved, {recursive: true, force: true});
}
