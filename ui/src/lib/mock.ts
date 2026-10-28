// Заглушка воркера для просмотра интерфейса в браузере (`npm run dev`) и для проверок без Tauri.
// Сцена выбирается параметром адреса: `?scene=empty|indexing|consent|reindex-required|worker-down|provider|missing|cosine|rerank-failed`.

import { BarsError } from "./types";
import { clone } from "./settings-logic";
import type { TopicCheck, TopicDetail, TopicHit, TopicStatus } from "./new-topics";
import type {
  ActivityOutput,
  Operation,
  ProjectInfo,
  SearchHit,
  SearchOutput,
  SettingsDoc,
  StatusOutput,
  TreeDocument,
} from "./types";

const scene = (): string => new URLSearchParams(window.location.search).get("scene") ?? "";

const autostart = { on: false };

const DOCS_BY_PROJECT: Record<string, TreeDocument[]> = {};

function docs(root: string): TreeDocument[] {
  const make = (path: string, chunks: number): TreeDocument => ({
    path: `${root}/${path}`,
    module: path.includes("/") ? path.split("/")[0] : ".",
    title: path.split("/").pop()!.replace(/\.md$/, ""),
    chunks,
  });
  return [
    make("Index.md", 2),
    make("architecture/Index.md", 2),
    make("architecture/protocol.md", 14),
    make("architecture/worker.md", 12),
    make("features/search/Index.md", 1),
    make("features/search/hybrid-search.md", 6),
    make("features/indexing/chunking.md", 10),
    make("features/indexing/updates.md", 8),
    make("knowledge/Index.md", 1),
    make("knowledge/stack-research.md", 16),
    make("knowledge/spikes/fts5-stemming.md", 5),
    make("knowledge/spikes/ort-build.md", 4),
  ];
}

const projects = (): ProjectInfo[] => {
  const module = (name: string, documents: number) => ({ name, documents });
  const modules = [
    module(".", 1),
    module("architecture", 3),
    module("features", 5),
    module("knowledge", 4),
  ];
  switch (scene()) {
    case "empty":
      return [];
    case "notifications":
      return [
        { path: "C:\\work\\bars-project", name: "bars-project", docs_root: "docs", state: "indexing", modules },
        { path: "C:\\work\\ai-mutiplayer-bots", name: "ai-mutiplayer-bots", docs_root: "docs", state: "ready", modules },
      ];
    case "reindex-required":
    case "consent":
      return [
        { path: "C:\\work\\bars-project", name: "bars-project", docs_root: "docs", state: "reindex_required", modules },
        { path: "C:\\work\\tmc-sa", name: "tmc-sa", docs_root: "docs", state: "reindex_required", modules },
        { path: "C:\\work\\tmc-region", name: "tmc-region", docs_root: "docs", state: "ready", modules },
      ];
    case "indexing":
      return [
        { path: "C:\\work\\bars-project", name: "bars-project", docs_root: "docs", state: "indexing", modules },
        { path: "C:\\work\\tmc-region", name: "tmc-region", docs_root: "docs", state: "indexing", modules },
        { path: "C:\\work\\old-landing", name: "old-landing", docs_root: "docs", state: "unavailable", modules: [] },
      ];
    case "missing":
      return [{ path: "C:\\work\\old-landing", name: "old-landing", docs_root: "docs", state: "unavailable", modules: [] }];
    default:
      return [
        { path: "C:\\work\\bars-project", name: "bars-project", docs_root: "docs", state: "ready", modules },
        { path: "C:\\work\\tmc-region", name: "tmc-region", docs_root: "docs", state: "ready", modules },
        { path: "C:\\work\\old-landing", name: "old-landing", docs_root: "docs", state: "unavailable", modules: [] },
      ];
  }
};

const HITS: SearchHit[] = [
  {
    path: "docs/features/search/hybrid-search.md",
    module: "features",
    title: "Гибридный поиск",
    heading_chain: "Гибридный поиск/Оценка результата и порог",
    first_line: 13,
    last_line: 17,
    text: "Порог применяется к оценке, у которой есть абсолютный смысл. Оценка RRF его не имеет:\n\n- **С реранком** — оценка реранка, порог `min_score_rerank`.\n- **Без реранка** — косинусная близость чанка к запросу, порог `min_score_vector`.",
    score: 0.87,
    score_kind: "rerank",
  },
  {
    path: "docs/architecture/protocol.md",
    module: "architecture",
    title: "Протокол канала bars",
    heading_chain: "Протокол канала bars/Порог агента вне пределов",
    first_line: 20,
    last_line: 22,
    text: "Порог агента вне пределов из настроек — не ошибка: он прижимается к границе, и в ответе указан применённый порог (Т-24).",
    score: 0.61,
    score_kind: "rerank",
  },
  {
    path: "docs/architecture/worker.md",
    module: "architecture",
    title: "Воркер bars",
    heading_chain: "Воркер bars/Очередь индексации воркера bars",
    first_line: 30,
    last_line: 34,
    text: "Приоритет: явная переиндексация и хук начала сессии → события слежения → проверка при старте и часовая.",
    score: 0.38,
    score_kind: "rerank",
  },
];

const DOC_TEXT = `---
title: Гибридный поиск
---

# Гибридный поиск

Поиск сливает полнотекстовый и векторный списки.

## Реранк кандидатов

Если rerank включён, слитые кандидаты переоцениваются rerank-моделью.

## Оценка результата и порог

Порог применяется к оценке, у которой есть абсолютный смысл. Оценка RRF его не имеет:

- **С реранком** — оценка реранка, порог \`min_score_rerank\`.
- **Без реранка** — косинусная близость чанка к запросу, порог \`min_score_vector\`.

## Состав результата

Каждый результат: путь, модуль, \`title\` документа, цепочка заголовков, строки, текст чанка и оценка.
`;

// «Новые темы»: состояние живёт, пока открыта страница; сцена `topics-empty|topics-error|topics-off` меняет исходное.
const NOW = Math.floor(Date.now() / 1000);
const TOPIC_HIT: TopicHit = { path: "docs/features/routes/lifecycle.md", title: "Жизненный цикл маршрута", heading_chain: "Жизненный цикл маршрута/Отмена", first_line: 18, last_line: 39, score: 0.81, score_kind: "rerank" };
let topicsState: { enabled: boolean; generation: number; next: number; topics: TopicDetail[] } | null = null;
function topicStore() {
  if (topicsState) return topicsState;
  const base = { resolved_at: null, variants: 1, reasons: ["empty_result"], checks: [] as TopicCheck[] };
  const empty = scene() === "topics-empty";
  topicsState = {
    enabled: scene() !== "topics-off", generation: 1, next: 4,
    topics: empty ? [] : [
      { ...base, id: 1, revision: 1, title: "Как восстановить изношенные шины бота?", status: "open", first_seen: NOW - 86400, last_seen: NOW - 1080, observations: 4, sources: ["window", "mcp"], variants: 2, variants_list: [{ id: 11, text: "Как восстановить изношенные шины бота?", scope: "project" }, { id: 12, text: "Можно ли вернуть боту новые шины без переподключения?", scope: "m=features;f=" }] },
      { ...base, id: 2, revision: 3, title: "Как отменить маршрут после смены роли?", status: "materials_found", first_seen: NOW - 7200, last_seen: NOW - 3600, observations: 2, sources: ["mcp"], variants_list: [{ id: 21, text: "Как отменить маршрут после смены роли?", scope: "m=features;f=" }], checks: [{ id: 5, topic_revision: 3, at: NOW - 3000, expanded: false, model: "builtin:BAAI/bge-m3", params: '{"min_score":0.3,"results":5}', conclusive: true, results: [{ variant_id: 21, scope: "m=features;f=", outcome: "found", hits: [TOPIC_HIT] }] }] },
      { ...base, id: 3, revision: 2, title: "Где задаётся лимит числа ботов?", status: "resolved", resolved_at: NOW - 43200, first_seen: NOW - 172800, last_seen: NOW - 100000, observations: 1, sources: ["window"], reasons: ["manual_missing"], variants_list: [{ id: 31, text: "Где задаётся лимит числа ботов?", scope: "project" }] },
    ],
  };
  return topicsState;
}
const topicSummary = (t: TopicDetail) => { const { variants_list: _v, checks: _c, ...summary } = t; return summary; };
function topicFor(params: Record<string, unknown>, expected = true): TopicDetail {
  const topic = topicStore().topics.find((item) => item.id === Number(params.id));
  if (!topic || (expected && Number(params.revision) !== topic.revision)) throw new BarsError("тема изменилась или удалена: обновите список", "stale_ref", -32012);
  return topic;
}
// --- Аудиты знаний (docs/features/knowledge-base/audits.md): заглушка воркера для просмотра экрана ---
interface MockAudit { kind: string; runningUntil: number; done: boolean; schedule: string; }
const auditState: Record<string, MockAudit> = {};
function auditFor(kind: string): MockAudit {
  return (auditState[kind] ??= { kind, runningUntil: 0, done: kind === "broken_links", schedule: kind === "broken_links" ? "on_change" : "off" });
}
interface MockFinding { standing: string; tag: string; group: string; reason: string; score?: number; subjects: unknown[]; detail: string; }
const auditIgnored = new Set<string>();
const mockFingerprint = (kind: string, i: number) => `${kind}:${i}`;
const AUDIT_FINDINGS: Record<string, MockFinding[]> = {
  contradictions: [
    { standing: "known", tag: "num", group: "main", reason: "Возможное расхождение — сек: 30 ↔ 60", subjects: [{ path: "docs/manual/search.md", first_line: 12, last_line: 12, heading: "Время ожидания", role: "manual" }, { path: "docs/features/search/hybrid.md", first_line: 15, last_line: 15, heading: "Время ожидания", role: "feature" }], detail: "Время ожидания ответа сервера составляет 30 с.\n↔\nВремя ожидания ответа сервера составляет 60 секунд." },
  ],
  broken_links: [
    { standing: "new", tag: "broken", group: "main", reason: "Ссылка на документ, которого нет", subjects: [{ path: "docs/features/search/hybrid.md", first_line: 14, last_line: 14, heading: "", role: "feature" }], detail: "Новая тема → Новая тема" },
    { standing: "known", tag: "broken", group: "main", reason: "Ссылка на документ, которого нет", subjects: [{ path: "docs/manual/search.md", first_line: 31, last_line: 31, heading: "", role: "manual" }], detail: "план → ../PLAN.md" },
    { standing: "gone", tag: "broken", group: "main", reason: "Ссылка на документ, которого нет", subjects: [{ path: "docs/adr/0001-layout.md", first_line: 9, last_line: 9, heading: "", role: "decision" }], detail: "" },
  ],
  duplicates: [
    { standing: "known", tag: "partial", group: "main", reason: "Заметная часть текста скопирована", score: 0.62, subjects: [{ path: "docs/manual/search.md", first_line: 40, last_line: 52, heading: "Поиск/Как работает", role: "manual" }, { path: "docs/features/search/hybrid.md", first_line: 8, last_line: 21, heading: "Гибридный поиск", role: "feature" }], detail: "Поиск складывает результаты полнотекстового и векторного поиска и сортирует их." },
    { standing: "known", tag: "semantic", group: "overlap", reason: "Близкий смысл, не копия", score: 0.84, subjects: [{ path: "docs/chats/2026-10-18-search/handoff.md", first_line: 5, last_line: 9, heading: "Сделано", role: "session_log" }, { path: "docs/features/search/hybrid.md", first_line: 30, last_line: 36, heading: "Проверка", role: "feature" }], detail: "" },
  ],
};
function auditSummary(kind: string) {
  const audit = auditFor(kind), now = Math.floor(Date.now() / 1000);
  const running = audit.runningUntil > Date.now();
  if (!running && audit.runningUntil > 0) { audit.done = true; audit.runningUntil = 0; }
  const findings = (AUDIT_FINDINGS[kind] ?? []).filter((_, i) => !auditIgnored.has(mockFingerprint(kind, i)));
  const main = findings.filter((f) => f.group === "main" && f.standing !== "gone");
  return {
    kind, state: running ? "running" : audit.done ? "complete" : "never", running_since: running ? now - 1 : undefined,
    last_run_at: audit.done ? now - 3600 : undefined, last_status: audit.done ? "complete" : undefined,
    result_run_id: audit.done ? 1 : undefined, result_at: audit.done ? now - 3600 : undefined,
    total: audit.done ? main.length : 0, new_count: audit.done ? main.filter((f) => f.standing === "new").length : 0,
    gone_count: audit.done ? findings.filter((f) => f.standing === "gone").length : 0,
    overlap_total: audit.done ? findings.filter((f) => f.group === "overlap").length : 0,
    schedule: audit.schedule, schedules: kind === "broken_links" ? ["off", "daily", "weekly", "on_change"] : ["off", "daily", "weekly"],
  };
}
async function auditsCall(method: string, params: Record<string, unknown>): Promise<unknown> {
  const kind = String(params.kind ?? "");
  switch (method) {
    case "audit_list":
      if (scene() === "audits-error") throw new BarsError("не удалось открыть хранилище аудитов: доступ запрещён", null, -32603);
      return { audits: ["broken_links", "duplicates", "contradictions"].map(auditSummary), reindex_required: scene() === "audits-reindex", indexing: scene() === "audits-indexing" };
    case "audit_ignore": {
      const fingerprint = String(params.fingerprint);
      if (params.ignored) auditIgnored.add(fingerprint); else auditIgnored.delete(fingerprint);
      return { ignored: Boolean(params.ignored) };
    }
    case "audit_run": { const audit = auditFor(kind); const already = audit.runningUntil > Date.now(); if (!already) audit.runningUntil = Date.now() + 1800; return { accepted: true, already, queued: false }; }
    case "audit_cancel": { const audit = auditFor(kind), was = audit.runningUntil > Date.now(); audit.runningUntil = 0; return { cancelled: was }; }
    case "audit_schedule": auditFor(kind).schedule = String(params.mode); return { mode: String(params.mode) };
    case "audit_findings": {
      const group = params.group ? String(params.group) : null;
      const items = (AUDIT_FINDINGS[kind] ?? []).map((f,i) => ({ ...f, fingerprint: mockFingerprint(kind,i), ignored: auditIgnored.has(mockFingerprint(kind,i)) })).filter((f) => (!group || f.group === group) && (params.include_gone || f.standing !== "gone") && (params.include_ignored || !f.ignored));
      return { run_id: auditFor(kind).done ? 1 : undefined, finished: Math.floor(Date.now() / 1000) - 3600, items, total: items.length };
    }
  }
  throw new BarsError(`нет метода ${method}`, "method_not_found", -32601);
}

async function topicsCall(method: string, params: Record<string, unknown>): Promise<unknown> {
  if (scene() === "topics-error" && method === "new_topics_list") throw new BarsError("не удалось открыть хранилище тем: доступ запрещён", null, -32603);
  const store = topicStore();
  switch (method) {
    case "new_topics_list":
      return { enabled: store.enabled, generation: store.generation, topics: store.topics.map(topicSummary), store_path: `${String(params.project)}\.bars\new-topics.sqlite3`, losses: scene() === "topics-loss" ? 3 : 0, loss_reason: scene() === "topics-loss" ? "очередь записи переполнена" : null };
    case "new_topics_get":
      return clone(topicFor(params, false));
    case "new_topics_mark_missing": {
      if (!store.enabled) throw new BarsError("сбор выключен", "invalid_params", -32007);
      const text = String(params.query);
      const same = store.topics.find((t) => t.variants_list.some((v) => v.text === text));
      if (same) return { topic_id: same.id, created: false };
      const id = store.next++;
      store.topics.push({ id, revision: 1, title: text, status: "open", first_seen: NOW, last_seen: NOW, resolved_at: null, observations: 1, sources: ["window"], reasons: ["manual_missing"], variants: 1, variants_list: [{ id: id * 10, text, scope: "project" }], checks: [] });
      return { topic_id: id, created: true };
    }
    case "new_topics_check": {
      await delay(700);
      const topic = topicFor(params);
      const found = topic.id !== 1 || Boolean(params.expand);
      const check: TopicCheck = { id: topic.checks.length + 100, topic_revision: topic.revision, at: Math.floor(Date.now() / 1000), expanded: Boolean(params.expand), model: "builtin:BAAI/bge-m3", params: '{"min_score":0.3,"results":5}', conclusive: true, results: topic.variants_list.map((v) => ({ variant_id: v.id, scope: params.expand ? "project" : v.scope, outcome: found ? "found" : "empty", hits: found ? [TOPIC_HIT] : [] })) };
      topic.checks = [check, ...topic.checks].slice(0, 2);
      if (found && topic.status === "open") { topic.status = "materials_found"; topic.revision += 1; }
      return clone(topic);
    }
    case "new_topics_set_status": {
      const topic = topicFor(params);
      topic.status = String(params.status) as TopicStatus;
      topic.resolved_at = topic.status === "resolved" ? Math.floor(Date.now() / 1000) : topic.resolved_at;
      return { revision: ++topic.revision };
    }
    case "new_topics_rename": {
      const topic = topicFor(params);
      topic.title = String(params.title);
      return { revision: ++topic.revision };
    }
    case "new_topics_delete": {
      const topic = topicFor(params);
      store.topics = store.topics.filter((t) => t !== topic);
      return { accepted: true };
    }
    case "new_topics_merge": {
      const topic = topicFor(params);
      const source = topicFor({ ...params, id: params.source_id, revision: params.source_revision });
      if (topic === source || topic.variants_list.length + source.variants_list.length > 50) throw new BarsError("объединение невозможно", "invalid_params", -32007);
      topic.variants_list.push(...source.variants_list);
      topic.observations += source.observations;
      topic.first_seen = Math.min(topic.first_seen, source.first_seen);
      topic.last_seen = Math.max(topic.last_seen, source.last_seen);
      topic.sources = [...new Set([...topic.sources, ...source.sources])];
      topic.reasons = [...new Set([...topic.reasons, ...source.reasons])];
      topic.checks = [...topic.checks, ...source.checks].sort((a, b) => b.at - a.at).slice(0, 2).map(check => ({ ...check, topic_revision: 0 }));
      topic.status = "open";
      store.topics = store.topics.filter((t) => t !== source);
      return { revision: ++topic.revision };
    }
    case "new_topics_clear":
      if (params.confirmed !== true) throw new BarsError("нужно подтверждение", "invalid_params", -32007);
      store.topics = [];
      return { generation: ++store.generation };
    case "new_topics_settings":
      store.enabled = Boolean(params.enabled);
      return { enabled: store.enabled, generation: ++store.generation };
    default:
      throw new BarsError(`заглушка не знает метод ${method}`, null, -32601);
  }
}

let paused = false;
const secrets = new Set<string>();
let savedSettings: SettingsDoc = {
  indexing: { edit_pause_seconds: 30 },
  mcp: { graph_tools: false },
  embed: { accelerator: "auto", source: "builtin", model: "BAAI/bge-m3", base_url: null, api_key_name: null, api_key_env: null, batch_size: 32, unload_after_minutes: 15 },
  rerank: { accelerator: "cpu", enabled: false, source: "builtin", model: "BAAI/bge-reranker-v2-m3", base_url: null, api_key_name: null, api_key_env: null, unload_after_minutes: 5 },
  search: {
    candidates: 30, rerank_candidates: 15, results: 5, results_reranked: 3, min_score_vector: 0, min_score_rerank: 0.1,
    agent_min_score: [0, 1], max_results: 20, max_candidates: 100, max_rerank_candidates: 50,
  },
  chunking: { min_chars: 250, max_chars: 6000 },
};

function activity(): ActivityOutput {
  if (scene() !== "indexing" && scene() !== "consent" && scene() !== "notifications") {
    return { paused: false, running: null, queue: [], problems: [] };
  }
  const t = Math.floor(Date.now() / 1000) % 100;
  return {
    paused,
    running: {
      embed_device: "directml", attempt: 1,
      project: "C:\\work\\bars-project",
      name: "bars-project",
      kind: scene() === "consent" ? "rebuild" : "check",
      priority: "explicit",
      docs_total: 214,
      docs_done: 40 + t,
      chunks_done: 180 + t * 4,
      chunks_per_second: paused ? 0 : 2.1,
      elapsed_seconds: 120 + t,
      current_file: "docs/features/indexing/chunking.md",
    },
    queue: [
      { project: "C:\\work\\tmc-region", name: "tmc-region", kind: "files", priority: "watch", files: 3 },
    ],
    problems: [
      {
        project: "C:\\work\\bars-project",
        name: "bars-project",
        rebuild_not_applied: false,
        files: [{ path: "docs/knowledge/spikes/ort-build.md", reason: "файл не UTF-8", stage: "", attempts: 0, previous_version: false }],
        last_error: null,
      },
    ],
    documents: [
      {
        project: "C:\\work\\bars-project",
        documents: [
          { path: "docs/features/indexing/document-queue.md", state: "waiting", source: "edit", due_in: 18, error: null },
          { path: "docs/manual/search.md", state: "queued", source: "watch", due_in: null, error: null },
          { path: "docs/knowledge/build.md", state: "error", source: "watch", due_in: null, error: "Провайдер вернул 503" },
        ],
        total: 3, waiting: 1, queued: 1, indexing: 0, error: 1,
      },
    ],
  };
}

function status(path: string): StatusOutput {
  const project = projects().find((p) => p.path === path) ?? projects()[0];
  return {
    path: project.path,
    state: project.state,
    updated_at: "2026-10-15T14:02:00Z",
    pending: 0,
    model: { index: "builtin:BAAI/bge-m3", settings: scene() === "consent" || scene() === "reindex-required" ? "remote:text-embedding-3-small" : "builtin:BAAI/bge-m3" },
    last_error: scene() === "provider" ? "провайдер https://api.example.com не ответил: 401" : null,
    watching: project.state !== "unavailable",
  };
}

const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export async function mockCall(method: string, params: Record<string, unknown>): Promise<unknown> {
  if (method === "project_observe") return { saved: true };
  // Браузерный стенд не имеет доступа к диску; не выдаём смысловые фикстуры за текстовый поиск.
  if (method === 'text_search') return {documents:[],docs_total:0,hits_total:0,truncated:null,skipped:[],elapsed_ms:0};
  if(method==='graph_query_slice'){
    const project = String(params.project ?? params.cwd);
    return (await import('./mock-query')).queryFixture(params, (DOCS_BY_PROJECT[project] ??= docs("docs")));
  }
  if(method==='graph_physical'){
    const project = String(params.project ?? params.cwd);
    return (await import('./mock-physical')).physicalFixture(params, (DOCS_BY_PROJECT[project] ??= docs("docs")));
  }
  if(['graph_query','graph_status','graph_build','graph_cancel'].includes(method))return (await import('./mock-graph')).graphFixture(method,params);
  if(method==='graph_search'){
    const mode=String(params.mode??'hybrid'),graph=(await import('./mock-graph')).graphFixture('graph_query',{request:{operation:mode==='graph'?'relations':'paths',view:'mixed'}});
    const search=await mockCall('search',params) as SearchOutput;
    const disabled=mode==='mix'&&params.include_related===false;
    return {...search,results:mode==='graph'?[]:search.results,requested_mode:mode,applied_mode:disabled?'hybrid':mode,fallback_reason:disabled?'Связи отключены: include_related=false':null,related:mode==='mix'&&!disabled?graph:null,graph_results:mode==='graph'?graph:null};
  }
  if(method==='read'){
    const source=(await import('./mock-graph')).fixtureSource(String(params.path??''));
    if(source)return {title:'Источник GraphRAG (фикстура)',text:source};
  }
  if (method.startsWith("new_topics_")) return topicsCall(method, params);
  if (scene() === "worker-down") throw new BarsError("нет связи с воркером", null, -32603);
  switch (method) {
    case "projects":
      return { projects: projects() };
    case "status":
      return status(String(params.project ?? params.cwd));
    case "tree": {
      const project = String(params.project ?? params.cwd);
      return { docs_root: "docs", documents: (DOCS_BY_PROJECT[project] ??= docs("docs")) };
    }
    case "audit_list": case "audit_run": case "audit_cancel": case "audit_schedule": case "audit_findings": case "audit_ignore":
      return auditsCall(method, params);
    case "links_of": {
      const project = String(params.project ?? params.cwd), path = String(params.path);
      const others = (DOCS_BY_PROJECT[project] ??= docs("docs")).filter((d) => d.path !== path);
      return {
        known: true,
        outgoing: [
          ...(others[0] ? [{ line: 3, text: others[0].title, raw: others[0].path, kind: "inline", target: others[0].path, target_title: others[0].title, broken: false }] : []),
          { line: 5, text: "Новая тема", raw: "Новая тема", kind: "wiki", broken: true },
        ],
        incoming: others[1] ? [{ path: others[1].path, title: others[1].title, lines: [2, 9] }] : [],
      };
    }
    case "activity":
      return activity();
    case "operation_events": {
      const base: Operation = { id: 1, sequence: 0, kind: "check", source: "explicit", project: "C:\\work\\ai-mutiplayer-bots", generation: 1, model: null, variant: null, reranker: false, state: "running", started_at: new Date(Date.now() - 120000).toISOString(), finished_at: null, docs_done: 38, docs_total: 92, percent: null, report: null, error: null, target_available: true };
      if (scene() !== "notifications") return { epoch: "browser-fixture", cursor: 0, gap: false, active: [], completed: [] };
      const completed: Operation[] = params.epoch ? [
        { ...base, id: 3, sequence: 1, state: "success", finished_at: new Date().toISOString(), report: { added: 2, changed: 4, removed: 0, unchanged: 86, problems: 0, index_applied: true } },
        { ...base, id: 4, sequence: 2, kind: "rebuild", state: "partial", finished_at: new Date().toISOString(), report: { added: 0, changed: 0, removed: 0, unchanged: 0, problems: 2, index_applied: false }, error: "Не обработаны 2 документа. Проверьте ошибки и повторите." },
      ].filter(op => op.sequence > Number(params.after ?? 0)) : [];
      return { epoch: "browser-fixture", cursor: params.epoch ? 2 : 0, gap: false, active: [base, { ...base, id: 2, project: "C:\\work\\bars-project", state: paused ? "queue_paused" : "queued", docs_total: null }, { ...base, id: 5, kind: "prepare", project: null, model: "BAAI/bge-m3", variant: "builtin · auto", docs_total: null, percent: 100 }], completed };
    }
    case "operation_retry":
      return { accepted: true };
    case "index_pause":
      paused = true;
      return { paused };
    case "index_resume":
      paused = false;
      return { paused };
    case "index_cancel":
      return { cancelled: true };
    case "index_retry":
      return { retried: true };
    case "reindex":
    case "unregister":
      return { accepted: true, removed: true };
    case "register":
      return { project: projects()[0] };
    case "search": {
      await delay(180);
      if (status(String(params.project ?? params.cwd)).state === "reindex_required") throw new BarsError("Нужна переиндексация: модель индекса отличается от выбранной. Разрешите переиндексацию проекта.", "reindex_required", -32004);
      if (scene() === "provider") throw new BarsError("провайдер https://api.example.com не ответил: 401", "provider_error", -32008);
      const query = String(params.query ?? "").toLowerCase();
      const limit = Number(params.limit ?? 3);
      const cosine = scene() === "cosine" || scene() === "rerank-failed";
      const hits = cosine ? HITS.map((hit, i) => ({ ...hit, score: [0.63, 0.60, 0.62][i], score_kind: "cosine" as const })) : HITS;
      const found = query.includes("нет") ? [] : hits.slice(0, limit);
      return {
        results: found.map(hit => ({ ...hit, chunk_id: `fixture-${HITS.findIndex(item => item.path === hit.path)}` })),
        applied: { results: limit, candidates: 30, rerank_candidates: 15, min_score: cosine ? 0 : 0.3, score_kind: cosine ? "cosine" : "rerank", clamped: false },
        rerank_error: scene() === "rerank-failed" ? "Test fixture: rerank unavailable" : null,
        indexing: scene() === "indexing",
        pending: scene() === "indexing" ? 118 : 0,
      };
    }
    case "similar": {
      // Фикстура браузера: тот же вход по ID, область до лимита, источник отдельно.
      const source = HITS[Number(String(params.chunk_id).replace("fixture-", ""))];
      if (!source) throw new BarsError("ID раздела устарел", "chunk_not_found");
      const modules = params.modules as string[] ?? [];
      const requested = Number(params.limit ?? 5), limit = Math.max(1, Math.min(20, requested));
      const hit = (item: SearchHit) => ({ ...item, chunk_id: `fixture-${HITS.indexOf(item)}`, score_kind: "cosine" as const });
      return { source: { ...hit(source), score: 1 }, results: HITS.filter(item => item !== source && (!modules.length || modules.includes(item.module))).slice(0, limit).map(hit), limit, clamped: limit !== requested, indexing: scene() === "indexing", pending: scene() === "indexing" ? 118 : 0 };
    }
    case "read":
      return { title: "Гибридный поиск", text: DOC_TEXT };
    case "settings_get":
      return { file: "C:\\Users\\me\\.bars\\settings.toml", revision: JSON.stringify(savedSettings), settings: clone(savedSettings), warnings: [] };
    case "settings_save": {
      const next = clone(savedSettings) as unknown as Record<string, Record<string, unknown>>;
      const base = params.base as Record<string, Record<string, unknown>>;
      const conflicts = [];
      for (const [path, mine] of Object.entries(params.changes as Record<string, unknown>)) {
        const [section, key] = path.split('.');
        const disk = next[section][key];
        if (JSON.stringify(savedSettings) !== params.expected_revision && JSON.stringify(disk) !== JSON.stringify(base[section][key]) && JSON.stringify(disk) !== JSON.stringify(mine)) conflicts.push({ key: path, base: base[section][key], disk, mine });
        next[section][key] = mine;
      }
      if (!conflicts.length) savedSettings = next as unknown as SettingsDoc;
      return { status: conflicts.length ? "conflict" : "saved", conflicts, ...(await mockCall("settings_get", {}) as object) };
    }
    case "models_info":
      return {
        runtime: null,
        embed: [
          { id: "BAAI/bge-m3", dim: 1024, backend: "onnx", warnings: [], downloaded: scene() !== "notifications", downloading: scene() === "notifications", download_percent: scene() === "notifications" ? 42 : 100 },
          { id: "Qwen/Qwen3-Embedding-0.6B", dim: 1024, backend: "gguf", warnings: ["GGUF Q4_K_M; GPU при доступном Vulkan/Metal, иначе CPU; сторонняя конверсия enacimie"], downloaded: false, downloading: false },
          { id: "Qwen/Qwen3-Embedding-4B", dim: 2560, backend: "gguf", warnings: ["GGUF Q4_K_M, около 2,5 ГБ на диске; память контекста добавляется к весам"], downloaded: false, downloading: false },
          { id: "nomic-ai/nomic-embed-text-v2-moe", dim: 768, backend: "candle", warnings: [], downloaded: false, downloading: false },
        ],
        rerank: [{ id: "BAAI/bge-reranker-v2-m3", dim: 0, backend: "onnx", warnings: [], downloaded: true, downloading: false }],
        models_dir: "C:\\Users\\me\\.bars\\models",
      };
    case "model_download":
    case "model_retry":
      return { accepted: true };
    case "secret_status":
      return { present: secrets.has(String(params.name)) };
    case "secret_set":
      secrets.add(String(params.name));
      return { present: true };
    case "autostart_get":
      return { supported: true, enabled: autostart.on, stale: false };
    case "autostart_set":
      autostart.on = Boolean(params.enabled);
      return { supported: true, enabled: autostart.on, stale: false };
    case "clients_status":
      return {
        clients: [
          { client: "claude-code", installed: true, stale: false, files: ["~/.claude.json", "~/.claude/settings.json"], needs_hook_approval: false },
          { client: "codex", installed: true, stale: false, files: ["~/.codex/config.toml", "~/.codex/hooks.json"], needs_hook_approval: true },
        ],
      };
    case "instruction":
      return {
        text: "Connect bars to your client:\n  bars-cli install --client claude-code\nNever pass --accept-model-change: it is the user's consent.\n",
        mcp_config: "{\n  \"mcpServers\": { \"bars\": { \"command\": \"bars-mcp\", \"args\": [] } }\n}",
      };
    case "install":
      return { changed_files: [], needs_hook_approval: params.client === "codex" };
    case "uninstall":
      return { changed_files: [] };
    case "set_docs_root":
    case "check_all":
      return { accepted: true };
    default:
      throw new BarsError(`заглушка не знает метод ${method}`, null, -32601);
  }
}
