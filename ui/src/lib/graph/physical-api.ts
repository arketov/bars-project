// Вызовы воркера для физического графа: срез `graph_physical` (только окно), состояние слоя и построение графа.
import { call } from "../api";
import type { GraphReply, GraphStatus } from "../graph";
import type { PhysicalRequest, ServerSlice } from "./slice";
import type { QueryRequest, ServerQuerySlice } from "./query";

export function loadQuery(project: string, request: QueryRequest): Promise<ServerQuerySlice> {
  return call<ServerQuerySlice>("graph_query_slice", { cwd: project, project, request });
}

export function loadPhysical(project: string, request: PhysicalRequest): Promise<ServerSlice> {
  return call<ServerSlice>("graph_physical", { cwd: project, project, request });
}

/** Операция смыслового слоя (`graph_query`): влияние, путь, контекст, обзор, аудит. Запись и построение она не запускает. */
export function semanticQuery(project: string, request: Record<string, unknown>): Promise<GraphReply> {
  return call<GraphReply>("graph_query", { cwd: project, project, request });
}

export function graphStatus(project: string): Promise<GraphStatus> {
  return call<GraphStatus>("graph_status", { cwd: project, project });
}

/** Ставит построение графа выбранного слоя. */
export async function startGraphBuild(project: string, layer = "all"): Promise<void> {
  await call("graph_build", { cwd: project, project, layer });
}

/** Просит остановить идущее построение; воркер подтверждает остановку в состоянии графа. */
export async function stopGraphBuild(project: string, job: string): Promise<void> {
  await call("graph_cancel", { cwd: project, project, job_id: job });
}
