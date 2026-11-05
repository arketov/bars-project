// Чистые Codex-сессии IDEA-08; временные проекты и JSONL остаются в target/.
import fs from 'node:fs';
import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';

const repo = path.resolve(import.meta.dirname, '..');
const evidence = path.resolve(process.argv[2] ?? path.join(repo, 'target/stage16/agents'));
const codex = process.env.STAGE16_CODEX ?? 'codex';
const authHome = process.env.CODEX_HOME ?? path.join(process.env.USERPROFILE, '.codex');
const instruction = fs.readFileSync(path.join(repo, 'crates/bars-core/assets/BARS_INSTRUCTION.md'), 'utf8');
const sha = text => createHash('sha256').update(text).digest('hex');
const write = (base, name, text) => {
  const file = path.join(base, name);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, text);
};
const doc = (title, text) => `---\ntitle: ${title}\n---\n\n# ${title}\n\n${text}\n`;
const standard = `Дата теста 2026-10-18. Работай только внутри этого временного проекта. Это тест ведения документации, программный код не нужен. Не меняй состояние git, не запускай сеть и внешние инструменты.
Выполни все записи, сохранив смысл и авторство:
1. Обнаружен баг: запрос cache после регистрации проекта возвращает 0 результатов, хотя слово есть в проиндексированном manual.md. Ожидался хотя бы один результат. Это воспроизводится; причина неизвестна.
2. Автор попросил кнопку избранных запросов. Идея ещё не одобрена к реализации.
3. Замерены два алгоритма на 10 запросах: A — медиана 120 мс, B — 80 мс. Качество и другие корпуса не измерялись, продукт не менялся. Сохрани исследование.
4. Автор сегодня принял архитектурное решение: хранить метаданные в SQLite, поскольку нужна транзакционность; JSON отвергнут из-за отсутствия транзакций. Оформи решение и обнови существующее описание устройства.
5. Автор принял требования фичи export: экспортировать найденные фрагменты в Markdown, сохранять пути источников; пустой результат даёт пустой документ. Дай проверяемую приёмку со ссылкой на Т-1 в SPEC.md.
6. Первый опыт с B отменён из-за нестабильного качества; более узкий повтор дал указанные времена. Запиши только существенное событие сессии, закончи handoff с тем, что качество ещё надо проверить.
Обнови нужные оглавления. Не создавай второго источника проектных требований, не превращай идею в принятое решение, не заявляй внедрение измеренного алгоритма. В финале перечисли созданные и изменённые пути.`;
const override = `Дата теста 2026-10-18. Работай только в этом временном проекте, без кода, сети, git-операций и внешних инструментов. Прочитай правила проекта и существующие документы. Добавь воспроизводимый баг: при поиске cache выдача пуста, хотя ожидается результат; причина неизвестна. Обнови существующее исследование: на 10 запросах A=120 мс, B=80 мс, качество не проверяли. Сохрани принятые проектом места документов, обнови оглавление. Заверши сессию короткой передачей с оставшейся проверкой качества. В финале перечисли пути.`;

async function run(mode, scenario) {
  const base = path.join(evidence, `${mode}-${scenario}`);
  fs.mkdirSync(base, { recursive: true });
  const home = path.join(base, 'home');
  const project = path.join(base, 'project');
  fs.mkdirSync(home, { recursive: true });
  fs.mkdirSync(project, { recursive: true });
  const init = spawnSync('git', ['init', '--quiet', project], { encoding: 'utf8' });
  if (init.status !== 0) throw new Error(init.stderr);
  const root = scenario === 'standard' ? 'docs' : 'manual';
  write(project, 'AGENTS.md', scenario === 'standard'
    ? 'Документация по-русски. Корень документации docs/. Общие требования всего проекта — только SPEC.md. Работать только в этом проекте.\n'
    : 'Документация по-русски, корень manual/. Баги только manual/tickets.md; исследования только manual/perf.md; передача сессии только manual/next.md. Это принятые места. Новые backlog/, knowledge/, chats/ не создавать. Обновляй существующие документы.\n');
  write(project, 'SPEC.md', doc('Требования тестового проекта', 'Т-1. Поиск возвращает исходные фрагменты с путями источников.'));
  if (scenario === 'standard') {
    write(project, `${root}/Index.md`, doc('Документация', '- [architecture](architecture/Index.md) — устройство.'));
    write(project, `${root}/architecture/Index.md`, doc('Устройство', '- [storage.md](storage.md) — хранение метаданных.'));
    write(project, `${root}/architecture/storage.md`, doc('Хранение метаданных', 'Статус: выбор хранилища ещё не записан. Метаданные используются при поиске.'));
  } else {
    write(project, `${root}/Index.md`, doc('Руководство', '- [tickets.md](tickets.md) — баги.\n- [perf.md](perf.md) — замеры.'));
    write(project, `${root}/tickets.md`, doc('Дефекты', 'Реестр дефектов проекта. Подтверждённых записей пока нет.'));
    write(project, `${root}/perf.md`, doc('Производительность', 'Статус: эксперимент запланирован, результата пока нет.'));
  }
  if (mode === 'instruction') write(project, `${root}/BARS_INSTRUCTION.md`, instruction);
  else {
    for (const skill of ['bars-docs', 'bars-session-log']) {
      const content = fs.readFileSync(path.join(repo, `crates/bars-worker/assets/skills/${skill}/SKILL.md`), 'utf8');
      write(project, `.agents/skills/${skill}/SKILL.md`, content);
    }
  }
  const prefix = mode === 'instruction'
    ? `Сначала прочитай ${root}/BARS_INSTRUCTION.md и применяй его советы с учётом правил проекта.\n`
    : 'Примени установленные проектные skills $bars-docs и $bars-session-log.\n';
  const prompt = prefix + (scenario === 'standard' ? standard : override);
  write(base, 'prompt.txt', prompt);
  const args = ['exec', '--ignore-user-config', '--ignore-rules', '--ephemeral', '--skip-git-repo-check',
    '-c', 'windows.sandbox="unelevated"', '--sandbox', 'workspace-write', '--json', '-C', project,
    '-o', path.join(base, 'answer.txt'), '-'];
  const out = fs.openSync(path.join(base, 'events.jsonl'), 'w');
  const err = fs.openSync(path.join(base, 'stderr.txt'), 'w');
  const start = Date.now();
  const result = await new Promise((resolve, reject) => {
    const child = spawn(codex, args, { cwd: project, windowsHide: true,
      env: { ...process.env, USERPROFILE: home, HOME: home, CODEX_HOME: authHome },
      stdio: ['pipe', out, err] });
    const timeout = setTimeout(() => { child.kill(); reject(new Error('Session timeout (10 minutes)')); }, 600000);
    child.on('error', error => { clearTimeout(timeout); reject(error); });
    child.on('close', code => { clearTimeout(timeout); resolve(code); });
    child.stdin.end(prompt);
  }).finally(() => { fs.closeSync(out); fs.closeSync(err); });
  if (result !== 0) throw new Error(`Codex exited ${result}: ${base}`);
  const exists = name => fs.existsSync(path.join(project, name));
  const read = name => fs.readFileSync(path.join(project, name), 'utf8');
  const all = [];
  function walk(dir, rel = '') {
    for (const item of fs.readdirSync(dir, { withFileTypes: true })) {
      if (item.name === '.git' || item.name === '.agents') continue;
      const name = path.posix.join(rel, item.name);
      if (item.isDirectory()) walk(path.join(dir, item.name), name); else all.push(name);
    }
  }
  walk(project);
  const checks = scenario === 'standard' ? {
    bug: exists('docs/backlog/issues.md') && /cache/i.test(read('docs/backlog/issues.md')),
    idea: exists('docs/backlog/ideas.md'), research: all.some(x => x.startsWith('docs/knowledge/') && x !== 'docs/knowledge/Index.md'),
    decision: all.some(x => /^docs\/adr\/\d{4}-.+\.md$/.test(x)),
    requirements: exists('docs/features/export/requirements.md') && /Т-1/.test(read('docs/features/export/requirements.md')),
    current: /SQLite|\]\(\.\.\/adr\//.test(read('docs/architecture/storage.md')) && !/выбор хранилища ещё не записан/.test(read('docs/architecture/storage.md')),
    handoff: all.some(x => /^docs\/chats\/.+\/handoff.md$/.test(x)),
    one_source: !all.includes('docs/MAIN_IDEA.md') && !all.includes('docs/requirements.md') && !/export|экспорт/i.test(read('SPEC.md')),
  } : {
    bug: /cache/i.test(read('manual/tickets.md')), research: /120/.test(read('manual/perf.md')) && /80/.test(read('manual/perf.md')),
    handoff: exists('manual/next.md'), priority: !exists('manual/backlog') && !exists('manual/knowledge') && !exists('manual/chats') && !exists('docs'),
  };
  const summary = { mode, scenario, code: result, seconds: (Date.now() - start) / 1000,
    instruction_sha256: sha(instruction), checks, files: all, passed: Object.values(checks).every(Boolean) };
  write(base, 'summary.json', JSON.stringify(summary, null, 2));
  console.log(JSON.stringify(summary));
  return summary;
}

fs.mkdirSync(evidence, { recursive: true });
const summaries = [];
for (const mode of ['instruction', 'skills']) {
  for (const scenario of process.argv[3] ? [process.argv[3]] : ['standard', 'override']) summaries.push(await run(mode, scenario));
}
write(evidence, 'summary.json', JSON.stringify(summaries, null, 2));
if (summaries.some(x => !x.passed)) process.exitCode = 1;
