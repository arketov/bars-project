// Шаг 2.3.7: рабочие каталоги для `bench_quality dump` и наборы запросов в формате бенча.
// Копирует индексы из %USERPROFILE%\.bars\bench\<проект>\index.sqlite (оригиналы не трогаются:
// открытие индекса новым кодом пересоздаёт полнотекстовую таблицу), снимок документов проекта
// (для индекса ссылок) и переводит `queries.json` набора 03-11-2026 в формат `quality::Case`.
// Всё складывается в `.bars/bench-237/` репозитория (в .gitignore), в репозиторий не попадает.
//   node setup.mjs <корень-репозитория>
import fs from 'node:fs';
import path from 'node:path';

const repo = path.resolve(process.argv[2] ?? '.');
const home = process.env.USERPROFILE.replaceAll('\\', '/');
const D = 'C:/Users/Shket/OneDrive/Рабочий стол/';
const PATHS = {
  'bars-project': D + 'SELF Repository/bars-project',
  'overdrive-bots': D + 'SELF Repository/overdrive/overdrive-bots',
  RoleGPT: D + 'RoleGPT',
  agaugu: D + 'agaugu',
  'tmc-opovechenie': D + 'tmc-opovechenie',
};
const out = path.join(repo, '.bars', 'bench-237');
// Деление на подбор и проверку лежит в наборе репозитория (в копиях запросов его нет).
const splitOf = new Map(JSON.parse(fs.readFileSync(path.join(repo, 'docs/knowledge/search-bench-2026-11-03/queries.json'), 'utf8').replace(/^﻿/, '')).map((q) => [q.id, q.split]));
fs.mkdirSync(out, { recursive: true });

function copyDocs(from, to) {
  let n = 0;
  for (const e of fs.readdirSync(from, { withFileTypes: true })) {
    if (e.isSymbolicLink()) continue;
    const src = path.join(from, e.name);
    if (e.isDirectory()) n += copyDocs(src, path.join(to, e.name));
    else if (e.name.toLowerCase().endsWith('.md')) {
      fs.mkdirSync(to, { recursive: true });
      fs.copyFileSync(src, path.join(to, e.name));
      n++;
    }
  }
  return n;
}

for (const [project, root] of Object.entries(PATHS)) {
  const ws = path.join(out, project);
  fs.mkdirSync(ws, { recursive: true });
  const index = path.join(ws, 'index.sqlite');
  if (!fs.existsSync(index)) fs.copyFileSync(`${home}/.bars/bench/${project}/index.sqlite`, index);
  const docs = path.join(ws, 'project', 'docs');
  const files = fs.existsSync(docs) ? 0 : copyDocs(path.join(root, 'docs'), docs);
  const raw = JSON.parse(fs.readFileSync(`${home}/.bars/bench/${project}/queries.json`, 'utf8').replace(/^\uFEFF/, ''));
  const cases = raw.map((q) => {
    if (!q.answerable || !q.gold.length) throw new Error(`запрос без ответа в основном наборе: ${q.id}`);
    return {
      id: q.id,
      split: q.split ?? splitOf.get(q.id),
      category: q.group,
      query: q.query,
      gold: q.gold.map((g) => ({ path: g.path, heading: g.heading, evidence: g.evidence, reason: 'бенч 03-11-2026' })),
    };
  });
  fs.writeFileSync(path.join(out, `cases-${project}.json`), JSON.stringify(cases, null, 1));
  console.log(project, 'запросов', cases.length, 'документов скопировано', files);
}

// Запросы без ответа из набора IDEA-06 overdrive-bots (od-081…od-100).
const od = JSON.parse(fs.readFileSync(path.join(repo, 'crates/bars-core/examples/quality/overdrive.json'), 'utf8').replace(/^\uFEFF/, ''));
const negatives = od.filter((c) => c.category === 'negative');
fs.writeFileSync(path.join(out, 'cases-overdrive-negative.json'), JSON.stringify(negatives, null, 1));
console.log('overdrive negative', negatives.length);
