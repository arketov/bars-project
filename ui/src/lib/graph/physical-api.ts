// Вызовы воркера для физического графа: срез `graph_physical` (только окно), состояние слоя и построение графа.
import { call } from "../api";
import type { GraphStatus } from "../graph";
import type { PhysicalRequest, ServerSlice } from "./slice";

export function loadPhysical(project: string, request: PhysicalRequest): Promise<ServerSlice> {
  return call<ServerSlice>("graph_physical", { cwd: project, project, request });
}

export function graphStatus(project: string): Promise<GraphStatus> {
  return call<GraphStatus>("graph_status", { cwd: project, project });
}

/** Ставит построение графа или просит остановить идущее; возвращает идентификатор задания, если оно есть. */
export async function toggleGraphBuild(project: string, activeJob: string | null, layer = "all"): Promise<void> {
  if (activeJob) await call("graph_cancel", { cwd: project, project, job_id: activeJob });
  else await call("graph_build", { cwd: project, project, layer });
}
