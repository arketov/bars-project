// Типы ответов воркера: зеркало `crates/bars-protocol/src/methods.rs` (docs/architecture/protocol.md).

export type ProjectState = "ready" | "indexing" | "reindex_required" | "unavailable";
export type ScoreKind = "rerank" | "cosine";
export type JobKind = "rebuild" | "check" | "files";
export type JobPriority = "explicit" | "edit" | "watch" | "periodic";

export interface ModuleInfo {
  name: string;
  documents: number;
}

export interface ProjectInfo {
  path: string;
  name: string;
  docs_root: string;
  state: ProjectState;
  modules: ModuleInfo[];
}

export interface ProjectsOutput {
  projects: ProjectInfo[];
}

export interface StatusOutput {
  path: string;
  state: ProjectState;
  updated_at: string | null;
  pending: number;
  model: { index: string | null; settings: string };
  last_error: string | null;
  watching: boolean;
}

export interface SearchHit {
  source_hash?: string;
  /** ID чанка для `get_similar`; окно его не показывает. */
  chunk_id?: string;
  path: string;
  module: string;
  title: string;
  heading_chain: string;
  first_line: number;
  last_line: number;
  text: string;
  score: number;
  score_kind: ScoreKind;
}

export interface AppliedParams {
  results: number;
  candidates: number;
  rerank_candidates: number;
  min_score: number;
  score_kind: ScoreKind;
  clamped: boolean;
}

export interface SearchOutput {
  results: SearchHit[];
  applied: AppliedParams;
  indexing: boolean;
  pending: number;
  rerank_error?: string;
}

export interface ReadOutput {
  revision?: string;
  title: string;
  text: string;
}

export interface SimilarOutput {
  source: SearchHit;
  results: SearchHit[];
  limit: number;
  clamped: boolean;
  indexing: boolean;
  pending: number;
}

export interface TreeDocument {
  path: string;
  module: string;
  title: string;
  chunks: number;
}

export interface TreeOutput {
  docs_root: string;
  documents: TreeDocument[];
}

export interface RunningJob {
  embed_device: string | null;
  attempt: number;
  project: string;
  name: string;
  kind: JobKind;
  priority: JobPriority;
  docs_total: number;
  docs_done: number;
  chunks_done: number;
  chunks_per_second: number;
  elapsed_seconds: number;
  current_file: string | null;
}

export interface QueuedJob {
  project: string;
  name: string;
  kind: JobKind;
  priority: JobPriority;
  files: number;
}

export interface FileProblem {
  stage: string;
  attempts: number;
  previous_version: boolean;
  path: string;
  reason: string;
}

export interface ProjectProblems {
  rebuild_not_applied: boolean;
  project: string;
  name: string;
  files: FileProblem[];
  last_error: string | null;
}

export type DocumentState = "waiting" | "queued" | "indexing" | "error";
export type DocumentSource = "explicit" | "edit" | "watch" | "periodic";

/** Запись очереди документов (document-queue.md). */
export interface QueuedDocument {
  path: string;
  state: DocumentState;
  source: DocumentSource;
  /** Секунд до конца паузы; только у `waiting`. */
  due_in: number | null;
  error: string | null;
}

/** Очередь документов проекта: поле `documents` ответа `activity`. */
export interface ProjectDocuments {
  project: string;
  documents: QueuedDocument[];
  total: number;
  waiting: number;
  queued: number;
  indexing: number;
  error: number;
}

export interface ActivityOutput {
  paused: boolean;
  running: RunningJob | null;
  queue: QueuedJob[];
  problems: ProjectProblems[];
  /** Очередь документов по проектам; у старого воркера поля нет. */
  documents?: ProjectDocuments[];
}

export interface OperationReport {
  added: number; changed: number; removed: number; unchanged: number;
  problems: number; index_applied: boolean;
  problem_paths?: string[];
}
export interface Operation {
  id: number; sequence: number; kind: string; source: string;
  project: string | null; generation: number; model: string | null; variant: string | null;
  reranker: boolean; state: string; started_at: string; finished_at: string | null;
  docs_done: number | null; docs_total: number | null; percent: number | null;
  queue_position?: number | null;
  report: OperationReport | null; error: string | null; target_available: boolean;
}
export interface OperationEventsOutput {
  epoch: string; cursor: number; gap: boolean; active: Operation[]; completed: Operation[];
}

/** Ошибка воркера: то же `kind`, что видит агент (docs/features/window/error-states.md). */
export class BarsError extends Error {
  constructor(
    message: string,
    readonly kind: string | null = null,
    readonly code: number = -32603,
    readonly data: { percent?: number; reason?: string } | null = null,
  ) {
    super(message);
  }
}

// --- настройки, модели, клиенты ---

export type ModelSourceName = "builtin" | "remote" | "local-openai" | "ollama" | "lmstudio" | "tei";

export interface EmbedDoc {
  accelerator: "auto" | "cpu";
  source: ModelSourceName;
  model: string;
  base_url: string | null;
  api_key_name: string | null;
  api_key_env: string | null;
  batch_size: number;
  /** Минут простоя до выгрузки встроенной модели, 0 — не выгружать. */
  unload_after_minutes: number;
}

export interface RerankDoc {
  accelerator: "auto" | "cpu";
  enabled: boolean;
  source: ModelSourceName;
  model: string;
  base_url: string | null;
  api_key_name: string | null;
  api_key_env: string | null;
  /** Минут простоя до выгрузки встроенной модели, 0 — не выгружать. */
  unload_after_minutes: number;
}

export interface SearchDoc {
  candidates: number;
  rerank_candidates: number;
  results: number;
  results_reranked: number;
  min_score_vector: number;
  min_score_rerank: number;
  agent_min_score: [number, number];
  max_results: number;
  max_candidates: number;
  max_rerank_candidates: number;
}

export interface SettingsDoc {
  indexing: { edit_pause_seconds: number };
  embed: EmbedDoc;
  rerank: RerankDoc;
  search: SearchDoc;
  chunking: { min_chars: number; max_chars: number };
  /** Таблица `[mcp]`: что MCP показывает агентам. */
  mcp: { graph_tools: boolean };
}

export interface SettingsOutput {
  revision: string | null;
  file: string;
  settings: SettingsDoc;
  warnings: string[];
}

export interface SettingsConflict { key: string; base: unknown; disk: unknown; mine: unknown }
export interface SettingsSaveOutput extends SettingsOutput { status: "saved" | "conflict"; conflicts: SettingsConflict[] }

export interface ModelInfo {
  id: string;
  dim: number;
  backend: "onnx" | "candle" | "gguf";
  warnings: string[];
  downloaded: boolean;
  downloading: boolean;
  download_percent?: number | null;
  download_error?: string | null;
  /** Вид ошибки встроенной embed-модели; у rerank-модели не задаётся (OQ-218). */
  download_error_reason?: string | null;
}

export interface ModelsInfoOutput {
  runtime: { embed: ModelRuntime; rerank: ModelRuntime } | null;
  embed: ModelInfo[];
  rerank: ModelInfo[];
  models_dir: string;
}

export interface ModelRuntime {
  model: string;
  source: string;
  requested: "auto" | "cpu";
  state: "unloaded" | "loading" | "ready" | "failed";
  backend: string;
  actual: string | null;
  fallback_reason: string | null;
  precision?: string;
  revision?: string;
}

export type InstallClientName = "claude-code" | "codex";

export interface ClientStatus {
  client: InstallClientName;
  installed: boolean;
  stale: boolean;
  files: string[];
  needs_hook_approval: boolean;
}

export interface ClientsStatusOutput {
  clients: ClientStatus[];
}

export interface AutostartOutput {
  supported: boolean;
  enabled: boolean;
  stale: boolean;
  command?: string;
}

export interface InstructionOutput {
  text: string;
  mcp_config: string;
}
