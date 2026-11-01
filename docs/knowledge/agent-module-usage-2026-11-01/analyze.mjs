// Повторный счёт 4.4.4. Только read-only SQLite и обезличенная проекция, без зависимостей.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { performance } from 'node:perf_hooks';
import assert from 'node:assert/strict';

export function scopeOf(requested, incomplete = []) {
  if (incomplete.some(note => note === 'original_mcp_input_unavailable' || note === 'requested' || /^requested(?:\.(?:modules|files)|:)/.test(note))) return 'unknown';
  if (!requested || typeof requested !== 'object' || Array.isArray(requested)) return 'unknown';
  for (const key of ['modules', 'files']) {
    if (requested[key] != null && !Array.isArray(requested[key])) return 'unknown';
  }
  const modules = !!requested.modules?.length;
  const files = !!requested.files?.length;
  return modules ? (files ? 'both' : 'modules') : (files ? 'files' : 'broad');
}

const rate = (numerator, denominator) => ({ numerator, denominator, percent: denominator ? 100 * numerator / denominator : null });
const hasModules = r => r.scope === 'modules' || r.scope === 'both';
const hasFiles = r => r.scope === 'files' || r.scope === 'both';
const date = seconds => new Date(seconds * 1000).toISOString();
const day = seconds => new Date((seconds + 5 * 3600) * 1000).toISOString().slice(0, 10);

export function summarize(records) {
  // projects не имеет проекта; соединяем по сессии до группировки поисков по проекту.
  const projects = new Set(records.filter(r => r.tool === 'bars_projects').map(r => r.session));
  const searches = records.filter(r => r.tool === 'bars_rag_search').sort((a, b) => a.started - b.started || a.id - b.id);
  const first = new Map();
  for (const row of searches) if (!first.has(row.session)) first.set(row.session, row);
  const scopes = { broad: 0, modules: 0, files: 0, both: 0, unknown: 0 };
  const outcomes = {};
  const modulesSessions = new Set();
  const filesSessions = new Set();
  const scopedSessions = new Set();
  const unknownSessions = new Set();
  for (const row of searches) {
    assert(Object.hasOwn(scopes, row.scope), 'Неизвестная категория области');
    scopes[row.scope]++;
    outcomes[row.outcome] = (outcomes[row.outcome] || 0) + 1;
    if (hasModules(row)) modulesSessions.add(row.session);
    if (hasFiles(row)) filesSessions.add(row.session);
    if (hasModules(row) || hasFiles(row)) scopedSessions.add(row.session);
    if (row.scope === 'unknown') unknownSessions.add(row.session);
  }
  const sessionsWithProjects = [...first.keys()].filter(s => projects.has(s));
  assert.equal(Object.values(scopes).reduce((a, b) => a + b, 0), searches.length);
  return {
    searches: searches.length, sessions: first.size, scopes, outcomes,
    withModules: rate(scopes.modules + scopes.both, searches.length),
    withModulesKnown: rate(scopes.modules + scopes.both, searches.length - scopes.unknown),
    withFiles: rate(scopes.files + scopes.both, searches.length),
    sessionsWithProjects: rate(sessionsWithProjects.length, first.size),
    sessionsWithModules: rate(modulesSessions.size, first.size),
    sessionsWithFiles: rate(filesSessions.size, first.size),
    sessionsWithoutObservedScope: first.size - scopedSessions.size,
    sessionsWithUnknownScope: unknownSessions.size,
    projectsSessionsWithoutObservedModules: sessionsWithProjects.filter(s => !modulesSessions.has(s)).length,
    firstObservedWithModules: rate([...first.values()].filter(hasModules).length, first.size),
    firstObservedWithoutFilter: [...first.values()].filter(r => r.scope === 'broad').length,
  };
}

export function report(records) {
  const grouped = (rows, key) => Object.fromEntries([...new Set(rows.filter(r => r.tool === 'bars_rag_search').map(key))].sort().map(k => {
    const searches = rows.filter(r => r.tool === 'bars_rag_search' && key(r) === k);
    const sessions = new Set(searches.map(r => r.session));
    return [k, summarize(rows.filter(r => sessions.has(r.session) && (r.tool === 'bars_projects' || key(r) === k)))];
  }));
  const filtered = records.filter(r => r.proxyAttempt == null || r.proxyAttempt <= 1);
  return {
    attempts: summarize(records),
    withoutMarkedProxyRetries: summarize(filtered),
    removedProxyRetries: records.length - filtered.length,
    unmarkedAttempts: records.filter(r => r.proxyAttempt == null).length,
    byProject: grouped(records, r => r.project),
    // Для дня bars_projects учитывается только в том же дне и интервале.
    byDay: Object.fromEntries([...new Set(records.map(r => day(r.started)))].sort().map(d => [d, summarize(records.filter(r => day(r.started) === d))])),
  };
}

export async function capture(database, until, output) {
  const started = performance.now();
  const to = Date.parse(until) / 1000;
  assert(Number.isSafeInteger(to) && to <= Date.now() / 1000, 'Нужна прошедшая граница ISO с часовым поясом');
  assert(/(?:Z|[+-]\d\d:\d\d)$/.test(until), 'Граница должна задавать часовой пояс');
  const from = to - 7 * 86400;
  assert(!fs.existsSync(output), 'Папка результата уже существует; снимок не перезаписывается');
  const { DatabaseSync } = await import('node:sqlite');
  const db = new DatabaseSync(database, { readOnly: true });
  let source, rows, settings, version;
  try {
    db.exec('BEGIN');
    version = db.prepare('PRAGMA user_version').get().user_version;
    assert.equal(version, 2, 'Поддерживается только история v2');
    source = { ...db.prepare('SELECT COUNT(*) rows,MIN(started) first,MAX(started) last,MIN(id) firstId,MAX(id) lastId FROM attempts').get() };
    settings = { ...db.prepare('SELECT enabled,generation FROM history_settings WHERE id=1').get() };
    // Фиксированный диапазон, одна транзакция; добавление и очистка живого журнала не меняют снимок.
    rows = db.prepare("SELECT id,epoch,session,started,project,tool,outcome,data FROM attempts WHERE started>=? AND started<? AND tool IN ('bars_rag_search','bars_projects') ORDER BY started,id").all(from, to);
    db.exec('ROLLBACK');
  } finally { db.close(); }
  const sessions = new Map();
  const projectOf = value => {
    const normalized = value.replaceAll('\\', '/').replace(/^\/\?\//, '').replace(/\/$/, '').toLowerCase();
    if (!normalized) return 'none';
    if (normalized === 'c:/users/arketov/desktop/bars-project') return 'bars-project';
    if (normalized === 'c:/users/arketov/desktop/cyberproject/ai-mutiplayer-bots') return 'overdrive-bots';
    return 'other';
  };
  const records = rows.map(row => {
    const data = JSON.parse(row.data);
    const key = JSON.stringify([row.epoch, row.session]);
    if (!sessions.has(key)) sessions.set(key, `S${sessions.size + 1}`);
    const proxyAttempt = data.applied?.proxy_attempt ?? null;
    assert(proxyAttempt === null || (Number.isSafeInteger(proxyAttempt) && proxyAttempt >= 1));
    return {
      id: row.id, started: row.started, session: sessions.get(key), project: projectOf(row.project),
      tool: row.tool, outcome: row.outcome,
      scope: row.tool === 'bars_rag_search' ? scopeOf(data.requested, data.incomplete) : null,
      proxyAttempt,
    };
  });
  const jsonl = records.map(r => JSON.stringify(r)).join('\n') + (records.length ? '\n' : '');
  const metadata = {
    version: 1, capturedAt: new Date().toISOString(), interval: { from: date(from), to: date(to), timezone: 'Asia/Yekaterinburg', bounds: '[from,to)' },
    source: { schema: version, ...source, settings, firstTime: source.first == null ? null : date(source.first), lastTime: source.last == null ? null : date(source.last) },
    rows: records.length, sessions: sessions.size,
    sha256: createHash('sha256').update(jsonl).digest('hex'),
    elapsedMs: performance.now() - started,
    caveats: ['Срез до исследовательской сессии; не доказательство недели после доставки.', 'Потери событий и удалённые записи не восстанавливаются.', 'Непомеченные повторы и эксперименты неразличимы.', 'Псевдонимы сессий действуют только внутри снимка.'],
  };
  fs.mkdirSync(output, { recursive: true });
  fs.writeFileSync(path.join(output, 'records.jsonl'), jsonl);
  fs.writeFileSync(path.join(output, 'snapshot.json'), JSON.stringify(metadata, null, 2) + '\n');
  return metadata;
}

function replay(directory) {
  const jsonl = fs.readFileSync(path.join(directory, 'records.jsonl'), 'utf8');
  const metadata = JSON.parse(fs.readFileSync(path.join(directory, 'snapshot.json'), 'utf8'));
  assert.equal(createHash('sha256').update(jsonl).digest('hex'), metadata.sha256, 'Снимок изменён');
  const records = jsonl.trim().split('\n').filter(Boolean).map(line => JSON.parse(line));
  assert.equal(records.length, metadata.rows);
  const started = performance.now();
  const result = report(records);
  const elapsedMs = performance.now() - started;
  fs.writeFileSync(path.join(directory, 'summary.json'), JSON.stringify(result, null, 2) + '\n');
  console.log(JSON.stringify({ attempts: result.attempts, removedProxyRetries: result.removedProxyRetries, elapsedMs }, null, 2));
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, ...args] = process.argv.slice(2);
  if (command === 'capture' && args.length === 3) console.log(JSON.stringify(await capture(...args), null, 2));
  else if (command === 'report' && args.length === 1) replay(args[0]);
  else throw new Error('capture <sqlite3> <ISO-to> <новая-папка> | report <папка-снимка>');
}
