// Типы ответов воркера: зеркало `crates/bars-protocol/src/methods.rs` (docs/architecture/protocol.md).

export type ProjectState = "ready" | "indexing" | "reindex_required" | "unavailable";
export type ScoreKind = "rerank" | "cosine";
export type JobKind = "rebuild" | "check" | "files";
export type JobPriority = "explicit" | "watch" | "periodic";

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
  title: string;
  text: string;
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
  path: string;
  reason: string;
}

export interface ProjectProblems {
  project: string;
  name: string;
  files: FileProblem[];
  last_error: string | null;
}

export interface ActivityOutput {
  paused: boolean;
  running: RunningJob | null;
  queue: QueuedJob[];
  problems: ProjectProblems[];
}

/** Ошибка воркера: то же `kind`, что видит агент (docs/features/window/error-states.md). */
export class BarsError extends Error {
  constructor(
    message: string,
    readonly kind: string | null = null,
    readonly code: number = -32603,
    readonly data: { percent?: number } | null = null,
  ) {
    super(message);
  }
}

// --- настройки, модели, клиенты ---

export type ModelSourceName = "builtin" | "remote" | "local-openai";

export interface EmbedDoc {
  source: ModelSourceName;
  model: string;
  base_url: string | null;
  api_key_name: string | null;
  api_key_env: string | null;
  batch_size: number;
}

export interface RerankDoc {
  enabled: boolean;
  source: ModelSourceName;
  model: string;
  base_url: string | null;
  api_key_name: string | null;
  api_key_env: string | null;
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
  embed: EmbedDoc;
  rerank: RerankDoc;
  search: SearchDoc;
  device: { accelerator: "auto" | "cpu" };
  chunking: { min_chars: number; max_chars: number };
}

export interface SettingsOutput {
  file: string;
  settings: SettingsDoc;
  warnings: string[];
}

export interface ModelInfo {
  id: string;
  dim: number;
  backend: "onnx" | "candle";
  warnings: string[];
  downloaded: boolean;
  downloading: boolean;
}

export interface ModelsInfoOutput {
  embed: ModelInfo[];
  rerank: ModelInfo[];
  models_dir: string;
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
