// Шаг 2.3.7: повторная проверка запросов без ответа по снимку корпуса (docs/knowledge/search-quality.md,
// «Пределы»): слова запроса не должны вместе встречаться ни в одном разделе снимка. Для запросов
// `negative` с фразой — фраза не должна встречаться нигде.
//   node check-negatives.mjs <корень-снимка> <набор.json> [порог покрытия, 0,6]
import fs from 'node:fs';
import path from 'node:path';

const [root, setFile, thresholdArg] = process.argv.slice(2);
const threshold = Number(thresholdArg ?? 0.6);
const cases = JSON.parse(fs.readFileSync(setFile, 'utf8').replace(/^﻿/, ''));
const sections = [];
(function walk(dir) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p);
    else if (e.name.toLowerCase().endsWith('.md')) {
      const rel = path.relative(root, p).replaceAll('\\', '/');
      let heading = '';
      let body = [];
      const flush = () => { if (body.length) sections.push({ rel, heading, text: body.join('\n').toLowerCase().replaceAll('ё', 'е') }); body = []; };
      for (const line of fs.readFileSync(p, 'utf8').split('\n')) {
        if (/^#{1,3} /.test(line)) { flush(); heading = line; }
        body.push(line);
      }
      flush();
    }
  }
})(root);
const stems = (q) => [...new Set((q.toLowerCase().replaceAll('ё', 'е').match(/[\p{L}\p{N}_]{4,}/gu) ?? []).map((w) => w.slice(0, 5)))];
let flagged = 0;
for (const c of cases.filter((x) => x.category === 'negative')) {
  const phrases = [...c.query.matchAll(/"([^"]+)"/g)].map((m) => m[1].toLowerCase().replaceAll('ё', 'е'));
  if (phrases.length) {
    const hit = sections.filter((s) => phrases.every((f) => s.text.includes(f)));
    if (hit.length) { flagged++; console.log(`ФРАЗА ЕСТЬ ${c.id}: ${hit.slice(0, 3).map((s) => s.rel + ' ' + s.heading).join('; ')}`); }
    continue;
  }
  const w = stems(c.query);
  let best = { cover: 0 };
  for (const s of sections) {
    const cover = w.filter((x) => s.text.includes(x)).length / Math.max(1, w.length);
    if (cover > best.cover) best = { cover, s };
  }
  if (best.cover >= threshold) { flagged++; console.log(`проверить ${c.id}: покрытие ${best.cover.toFixed(2)} (${w.length} слов) — ${best.s.rel} ${best.s.heading}`); }
}
console.log(`запросов без ответа: ${cases.filter((x) => x.category === 'negative').length}, к ручной проверке: ${flagged}, разделов в снимке: ${sections.length}`);
