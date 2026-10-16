// Заглушка воркера для просмотра интерфейса в браузере (`npm run dev`) и для проверок без Tauri.
// Сцена выбирается параметром адреса: `?scene=empty|indexing|consent|worker-down|provider|missing`.

import { BarsError } from "./types";
import type {
  ActivityOutput,
  ProjectInfo,
  SearchHit,
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

let paused = false;
const secrets = new Set<string>();
let savedSettings: SettingsDoc = {
  embed: { source: "builtin", model: "BAAI/bge-m3", base_url: null, api_key_name: null, api_key_env: null, batch_size: 32 },
  rerank: { enabled: false, source: "builtin", model: "BAAI/bge-reranker-v2-m3", base_url: null, api_key_name: null, api_key_env: null },
  search: {
    candidates: 30, rerank_candidates: 5, results: 5, results_reranked: 3, min_score_vector: 0, min_score_rerank: 0.1,
    agent_min_score: [0, 1], max_results: 20, max_candidates: 100, max_rerank_candidates: 50,
  },
  device: { accelerator: "auto" },
  chunking: { min_chars: 250, max_chars: 6000 },
};

function activity(): ActivityOutput {
  if (scene() !== "indexing" && scene() !== "consent") {
    return { paused: false, running: null, queue: [], problems: [] };
  }
  const t = Math.floor(Date.now() / 1000) % 100;
  return {
    paused,
    running: {
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
        files: [{ path: "docs/knowledge/spikes/ort-build.md", reason: "файл не UTF-8" }],
        last_error: null,
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
    model: { index: "builtin:BAAI/bge-m3", settings: scene() === "consent" ? "remote:text-embedding-3-small" : "builtin:BAAI/bge-m3" },
    last_error: scene() === "provider" ? "провайдер https://api.example.com не ответил: 401" : null,
    watching: project.state !== "unavailable",
  };
}

const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export async function mockCall(method: string, params: Record<string, unknown>): Promise<unknown> {
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
    case "activity":
      return activity();
    case "index_pause":
      paused = true;
      return { paused };
    case "index_resume":
      paused = false;
      return { paused };
    case "index_cancel":
      return { cancelled: true };
    case "reindex":
    case "unregister":
      return { accepted: true, removed: true };
    case "register":
      return { project: projects()[0] };
    case "search": {
      await delay(180);
      if (scene() === "provider") throw new BarsError("провайдер https://api.example.com не ответил: 401", "provider_error", -32008);
      const query = String(params.query ?? "").toLowerCase();
      const limit = Number(params.limit ?? 3);
      const found = query.includes("нет") ? [] : HITS.slice(0, limit);
      return {
        results: found,
        applied: { results: limit, candidates: 30, rerank_candidates: 5, min_score: 0.3, score_kind: "rerank", clamped: false },
        indexing: scene() === "indexing",
        pending: scene() === "indexing" ? 118 : 0,
      };
    }
    case "read":
      return { title: "Гибридный поиск", text: DOC_TEXT };
    case "settings_get":
      return { file: "C:\\Users\\me\\.bars\\settings.toml", settings: structuredClone(savedSettings), warnings: [] };
    case "settings_save":
      savedSettings = structuredClone(params.settings as SettingsDoc);
      return { saved: true };
    case "models_info":
      return {
        embed: [
          { id: "BAAI/bge-m3", dim: 1024, backend: "onnx", warnings: [], downloaded: true, downloading: false },
          { id: "Qwen/Qwen3-Embedding-0.6B", dim: 1024, backend: "candle", warnings: ["считается на процессоре через candle и заметно медленнее bge-m3"], downloaded: false, downloading: false },
          { id: "Qwen/Qwen3-Embedding-4B", dim: 2560, backend: "candle", warnings: ["модель весит около 8 ГБ на диске и держит в памяти около 16 ГБ"], downloaded: false, downloading: false },
          { id: "nomic-ai/nomic-embed-text-v2-moe", dim: 768, backend: "candle", warnings: [], downloaded: false, downloading: false },
        ],
        rerank: [{ id: "BAAI/bge-reranker-v2-m3", dim: 0, backend: "onnx", warnings: [], downloaded: true, downloading: false }],
        models_dir: "C:\\Users\\me\\.bars\\models",
      };
    case "model_download":
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
