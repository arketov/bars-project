// Только браузерный макет: записанные ответы настоящего bars-graph на тестовых документах.
import recorded from "./fixtures/graph.json";
import type { GraphReply, GraphStatus } from "./graph";
let building = false;
export function graphFixture(
  method: string,
  params: Record<string, unknown>,
): unknown {
  const records = recorded as unknown as Record<string, GraphReply>;
  const request = (params.request ?? {}) as Record<string, unknown>;
  if (method === "graph_status")
    return {
      state: building ? "building" : "partial",
      snapshot: records["graph:documents"].snapshot,
      active_job: building ? "fixture-job" : null,
      source_count: 6,
      error: null,
      method: "browser-recorded-fixture",
    } satisfies GraphStatus;
  if (method === "graph_build") {
    building = true;
    setTimeout(() => (building = false), 1200);
    return { accepted: true, job_id: "fixture-job" };
  }
  if (method === "graph_cancel") {
    building = false;
    return { cancelled: true };
  }
  const operation = String(request.operation ?? "graph");
  const key =
    operation === "graph"
      ? `graph:${request.view ?? "documents"}`
      : operation === "explore" &&
          request.analysis &&
          request.analysis !== "topics"
        ? `explore:${String(request.analysis).replaceAll("_", "")}`
        : operation;
  const result = structuredClone(records[key] ?? records["graph:documents"]);
  result.warnings.push(
    "Браузерный макет: заранее записанные ответы на тестовых документах. Произвольные запросы и scope проверяются через настоящий воркер.",
  );
  return result;
}
export function fixtureSource(path: string): string | undefined {
  return (recorded.__sources as Record<string, string>)[
    path.replace(/^\.\//, "")
  ];
}
