import type { SearchOutput } from "./types";
export type ObjectKind =
  "document" | "section" | "chunk" | "entity" | "community";
export interface ObjectRef {
  kind: ObjectKind;
  id: string;
  revision: string;
}
export interface GraphNode {
  reference: ObjectRef;
  label: string;
  namespace: string;
  aliases: string[];
  source_ids: string[];
  evidence: string[];
  roles: string[];
  parent?: string;
  anchor?: string;
}
export interface GraphEdge {
  id: string;
  from: string;
  to: string;
  predicate: string;
  polarity: "positive" | "negative";
  qualifiers: Record<string, string>;
  origin: string;
  evidence: string[];
  score?: number;
}
export interface Evidence {
  id: string;
  document: ObjectRef;
  span: { start: number; end: number; first_line: number; last_line: number };
  quote: string;
  source_hash: string;
  method: string;
}
export interface GraphSource {
  reference: ObjectRef;
  path: string;
  docs_path: string;
  module: string;
  title: string;
  hash: string;
}
export interface GraphPath {
  nodes: ObjectRef[];
  steps: { edge: string; reversed: boolean }[];
  evidence: string[];
  complete: boolean;
}
export type GraphItem =
  | { type: "node"; node: GraphNode }
  | { type: "edge"; edge: GraphEdge }
  | { type: "relation"; assertion: GraphEdge }
  | { type: "path"; path: GraphPath }
  | { type: "impact"; node: ObjectRef; direct: boolean; path: GraphPath }
  | { type: "role_group"; role: string; evidence: string[] }
  | {
      type: "topic";
      id: string;
      label: string;
      members: ObjectRef[];
      representatives: ObjectRef[];
    }
  | {
      type: "audit";
      kind: string;
      node?: ObjectRef;
      evidence: string[];
      detail: string;
    }
  | {
      type: "conflict";
      assertions: string[];
      evidence: string[];
      candidate: boolean;
    };
export type GraphOperation =
  "graph" | "relations" | "paths" | "impact" | "context" | "explore";
export type GraphView =
  "documents" | "sections" | "chunks" | "entities" | "mixed";
export interface GraphReply {
  schema_version: number;
  operation: GraphOperation;
  snapshot: string;
  scope: { modules: string[]; files: string[] };
  layer_state: string;
  layer_states: Record<string, string>;
  layers?: Record<string, GraphLayerStatus>;
  coverage: {
    indexed: number;
    processed: number;
    failed: number;
    denominator: number | null;
    semantic_completeness: number | null;
    method: string;
  };
  nodes: GraphNode[];
  sources: GraphSource[];
  assertions: GraphEdge[];
  evidence: Evidence[];
  items: GraphItem[];
  truncated: boolean;
  limit_reasons: string[];
  next_cursor: string | null;
  warnings: string[];
  resolution: string;
  candidates: GraphNode[];
  missing_roles: string[];
  budget_mode: string;
  used_bytes: number;
  elapsed_ms: number;
  method: string;
}
export interface GraphStatus {
  freshness?: "not_built" | "current" | "stale";
  completeness?: "complete" | "partial" | "unknown";
  availability?: "enabled" | "disabled";
  job_state?: "idle" | "queued" | "running" | "stopped" | "failed";
  state: string;
  snapshot: string | null;
  active_job: string | null;
  source_count: number;
  error: string | null;
  method: string;
  progress?: { phase: string; processed: number; total: number | null; elapsed_ms: number } | null;
  file_errors?: { path: string; code: string; message: string }[];
  file_errors_total?: number;
  paused?: boolean;
  layers?: Record<string, GraphLayerStatus>;
}
export interface GraphLayerStatus {
  stale_reason?: string | null;
  state: string;
  freshness: "not_built" | "current" | "stale";
  completeness: "complete" | "partial" | "unknown";
  availability: "enabled" | "disabled";
  generation: string | null;
  fingerprint: string;
  source_revisions: Record<string, string>;
  coverage: { unit: string; processed: number; total: number; missing: number; comparisons: number; reason: string | null };
  processed: number; total: number; failed: number;
}
export function graphDataLabel(status: GraphStatus): string {
  if (status.availability === "disabled") return "Граф выключен";
  if (status.freshness === "stale") return "Граф устарел";
  if (status.freshness === "current") return "Граф актуален";
  return "Граф ещё не построен";
}
export type RetrievalMode = "hybrid" | "mix" | "graph";
export interface GraphSearchOutput extends SearchOutput {
  requested_mode: RetrievalMode;
  applied_mode: RetrievalMode;
  fallback_reason: string | null;
  related: GraphReply | null;
  graph_results: GraphReply | null;
}
export function sameRef(a: ObjectRef, b: ObjectRef): boolean {
  return a.id === b.id && a.revision === b.revision && a.kind === b.kind;
}
