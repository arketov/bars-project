// sim-heading-id.mjs [вес...] — приближённый прогон сигнала «номер в собственном заголовке» (шаг 5.4.1, ISS-115).
// Не бенч продукта: берёт готовые десять результатов bars-cli от 03-11-2026 (cli-full.json) и пересортировывает их по S' = опора + w·b,
// где b = 1, если токен-идентификатор запроса (с цифрой, дефисом, подчёркиванием или точкой внутри) стоит в последнем заголовке цепочки куска.
// Пул — только эти десять результатов: определение, которого в десятке не было, сигнал не поднимет.
import fs from 'node:fs';

const BENCH = process.env.USERPROFILE + '/.bars/bench/';
const PROJECTS = ['bars-project', 'overdrive-bots', 'RoleGPT', 'agaugu', 'tmc-opovechenie'];
const WEIGHTS = process.argv.slice(2).length ? process.argv.slice(2).map(Number) : [0, 0.05, 0.1, 0.2, 0.3, 0.5, 1];
const N = (x) => x.split(/\s+/).join(' ');
const ID_RE = /[\p{L}\p{N}]+(?:[-_.][\p{L}\p{N}]+)+|\p{L}+\d+/gu;

const idTokens = (query) => (query.match(ID_RE) || []).filter((t) => /[-_.\d]/.test(t)).map((t) => t.toLowerCase());
const lastHeading = (chain) => chain.split('/').pop().toLowerCase();
// граница слова: токен не должен быть куском более длинного идентификатора (ISS-1 в ISS-115)
const hasToken = (text, tok) => {
  let i = -1;
  while ((i = text.indexOf(tok, i + 1)) !== -1) {
    const before = text[i - 1], after = text[i + tok.length];
    if ((!before || !/[\p{L}\p{N}_-]/u.test(before)) && (!after || !/[\p{L}\p{N}_]/u.test(after))) return true;
  }
  return false;
};
const bonus = (q, row) => {
  const h = lastHeading(row.heading_chain);
  return idTokens(q.query).some((t) => hasToken(h, t)) ? 1 : 0;
};

const relevant = (gold, row) => gold.some((g) => g.path === row.path && N(row.text).includes(N(g.evidence)));
function rankOf(q, rows) {
  const i = rows.slice(0, 10).findIndex((r) => relevant(q.gold, r));
  return i < 0 ? 0 : i + 1;
}

const per = [];
for (const proj of PROJECTS) {
  const qs = JSON.parse(fs.readFileSync(`${BENCH}${proj}/queries.json`, 'utf8')).filter((q) => q.gold.length);
  const cli = Object.fromEntries(JSON.parse(fs.readFileSync(`${BENCH}${proj}/cli-full.json`, 'utf8')).map((o) => [o.id, o]));
  for (const q of qs) {
    const o = cli[q.id];
    if (!o || o.error || o.rerank_error) continue;
    const rows = o.results.map((r, i) => ({ ...r, i, b: bonus(q, r) }));
    const ranks = {};
    for (const w of WEIGHTS) {
      const sorted = [...rows].sort((a, b) => (b.score + w * b.b) - (a.score + w * a.b) || a.i - b.i);
      ranks[w] = rankOf(q, sorted);
    }
    per.push({ id: q.id, project: proj, group: q.group, query: q.query, bonusRows: rows.filter((r) => r.b).length, ranks });
  }
}

const m = (items, w) => ({
  n: items.length,
  first: items.filter((x) => x.ranks[w] === 1).length,
  hit3: items.filter((x) => x.ranks[w] && x.ranks[w] <= 3).length,
  mrr: items.reduce((s, x) => s + (x.ranks[w] ? 1 / x.ranks[w] : 0), 0),
});
const by = (key) => Object.entries(per.reduce((a, x) => ((a[x[key]] ||= []).push(x), a), {}));

console.log('запросов', per.length, '· с бонусом хотя бы у одной строки десятки:', per.filter((x) => x.bonusRows).length);
for (const w of WEIGHTS) {
  const t = m(per, w);
  console.log(`w=${w}`.padEnd(8), `всего первым ${t.first}/${t.n} Hit@3 ${t.hit3} MRR ${(t.mrr / t.n).toFixed(3)}`);
}
for (const key of ['group', 'project']) {
  for (const [k, items] of by(key)) {
    const base = m(items, WEIGHTS[0]);
    const line = WEIGHTS.slice(1).map((w) => { const x = m(items, w); return `w${w}: ${x.first - base.first >= 0 ? '+' : ''}${x.first - base.first}/${x.hit3 - base.hit3 >= 0 ? '+' : ''}${x.hit3 - base.hit3}`; }).join('  ');
    console.log(key.padEnd(7), k.padEnd(28), `n=${items.length} база первым ${base.first} Hit@3 ${base.hit3} | Δпервым/ΔHit@3 ${line}`);
  }
}
console.log('--- запросы, у которых ранг изменился хоть при одном весе ---');
for (const x of per) {
  const r = WEIGHTS.map((w) => x.ranks[w]);
  if (new Set(r).size > 1) console.log(x.id.padEnd(6), x.group.padEnd(16), x.query.slice(0, 44).padEnd(44), 'ранги по весам', r.join(' '));
}
