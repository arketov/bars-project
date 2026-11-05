import { afterEach, expect, it } from "vitest";
import { mockCall } from "./mock";
import { clone } from "./settings-logic";
import { settingsChanges } from "./settings.svelte";
import type { ActivityOutput, SearchOutput, SettingsOutput, StatusOutput, TreeDocument } from "./types";
import { type ServerSlice } from "./graph/slice";
import { acceptQuerySlice, queryRequest, type QueryInput, type ServerQuerySlice } from "./graph/query";

afterEach(() => window.history.replaceState(null, "", "/"));

const project = "C:\\work\\bars-project";

it("дерево, поиск и физический граф используют одинаковые пути документов (ISS-53)", async () => {
  const tree = await mockCall("tree", { project }) as { documents: TreeDocument[] };
  const graph = await mockCall("graph_physical", { project }) as ServerSlice;
  expect(graph.nodes.map(n => n.path).sort()).toEqual(tree.documents.map(d => d.path).sort());
  for (const node of graph.nodes) {
    const doc = tree.documents.find(d => d.path === node.path)!;
    expect(node).toMatchObject({ label: doc.title, chunks: doc.chunks, module: doc.module });
  }
  const search = await mockCall("search", { project, query: "порог" }) as SearchOutput;
  const input: QueryInput = {
    id: "test", text: "порог", hits: search.results, scoreKind: "rerank",
    requested: "hybrid", applied: "hybrid", minScore: 0.3, scope: { modules: [], files: [] },
  };
  const server = await mockCall("graph_query_slice", { project, request: queryRequest(input, 1, 0.5) }) as ServerQuerySlice;
  const query = acceptQuerySlice(server, input);
  expect(query.info.results).toHaveLength(3);
  expect(query.info.results.every(r => r.node !== null && r.status === "ready")).toBe(true);
  expect(query.slice.edges.filter(e => e.kind === "query_result")).toHaveLength(3);
});

it("область и раскрытые чанки заглушки сохраняют пути и владельцев документов", async () => {
  const graph = await mockCall("graph_physical", { project, request: { scope: { modules: ["features"], files: [] } } }) as ServerSlice;
  expect(graph.nodes.length).toBeGreaterThan(0);
  expect(graph.nodes.every(n => n.module === "features")).toBe(true);
  const doc = graph.nodes[0];
  const expanded = await mockCall("graph_physical", { project, request: {
    scope: { modules: [], files: [doc.docs_path] }, expanded: [{ document: doc.id, shown: 50 }],
  } }) as ServerSlice;
  expect(expanded.totals.documents).toBe(1);
  expect(expanded.nodes.filter(n => n.kind === "chunk")).toHaveLength(doc.chunks);
  expect(expanded.nodes.every(n => n.path === doc.path && n.owner === doc.id)).toBe(true);
});

it("сцена переиндексации без задания возвращает ошибку только ожидающим проектам", async () => {
  window.history.replaceState(null, "", "/?scene=reindex-required");
  const status = await mockCall("status", { project }) as StatusOutput;
  expect(status.state).toBe("reindex_required");
  expect(status.model.index).not.toBe(status.model.settings);
  expect((await mockCall("activity", {}) as ActivityOutput).running).toBeNull();
  for (const method of ["search"]) {
    await expect(mockCall(method, { project, query: "порог", mode: "mix" })).rejects.toMatchObject({ kind: "reindex_required" });
  }
  const ready = await mockCall("search", { project: "C:\\work\\tmc-region", query: "порог" }) as SearchOutput;
  expect(ready.results).toHaveLength(3);
});

it("Save/Get accepts a proxy-backed settings draft and keeps devices independent", async () => {
  const before = await mockCall("settings_get", {}) as SettingsOutput;
  const draft = new Proxy(clone(before.settings), {});
  draft.embed.accelerator = "cpu";
  draft.rerank.accelerator = "auto";
  await mockCall("settings_save", { base: before.settings, changes: settingsChanges(before.settings, draft), expected_revision: before.revision });
  const saved = await mockCall("settings_get", {}) as SettingsOutput;
  expect(saved.settings.embed.accelerator).toBe("cpu");
  expect(saved.settings.rerank.accelerator).toBe("auto");
});
