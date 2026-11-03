// run-cli.mjs [проекты...] — полный конвейер через установленный bars-cli --json, строго по одному запросу
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';

const home = process.env.USERPROFILE + '/.bars/bench/';
const cli = process.env.LOCALAPPDATA + '/bars/bars-cli.exe';
const D = 'C:/Users/Shket/OneDrive/Рабочий стол/';
const PATHS = { 'bars-project': D + 'SELF Repository/bars-project', 'overdrive-bots': D + 'SELF Repository/overdrive/overdrive-bots', RoleGPT: D + 'RoleGPT', agaugu: D + 'agaugu', 'tmc-opovechenie': D + 'tmc-opovechenie' };
const sleep = (ms) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
const projects = process.argv.slice(2).length ? process.argv.slice(2) : Object.keys(PATHS);

function search(proj, q) {
  const r = spawnSync(cli, ['--json', 'search', '--project', PATHS[proj], '--limit', '10', q], { encoding: 'utf8', maxBuffer: 64 << 20, timeout: 180000 });
  if (r.status !== 0) return { error: (r.stderr || '').slice(0, 300) || 'exit ' + r.status };
  try { return JSON.parse(r.stdout); } catch (e) { return { error: 'json: ' + r.stdout.slice(0, 200) }; }
}

for (const proj of projects) {
  const qs = JSON.parse(fs.readFileSync(home + proj + '/queries.json', 'utf8'));
  const out = [];
  const t0 = Date.now();
  for (const q of qs) {
    let res, tries = 0, ms;
    for (;;) {
      const s = Date.now();
      res = search(proj, q.query);
      ms = Date.now() - s;
      tries++;
      const bad = res.error || res.rerank_error || res.error_code;
      if (!bad || tries >= 8) break;
      sleep(8000);
    }
    out.push({ id: q.id, tries, ms, rerank_error: res.rerank_error || null, error: res.error || null, applied: res.applied, results: (res.results || []).map((r) => ({ path: r.path, module: r.module, heading_chain: r.heading_chain, first_line: r.first_line, last_line: r.last_line, score: r.score, score_kind: r.score_kind, text: r.text })) });
  }
  fs.writeFileSync(home + proj + '/cli-full.json', JSON.stringify(out));
  console.log(proj, qs.length, 'запросов за', ((Date.now() - t0) / 1000).toFixed(0), 'с; без rerank:', out.filter((o) => o.rerank_error).length, 'ошибок:', out.filter((o) => o.error).length);
}
