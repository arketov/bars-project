import { afterEach, expect, it, vi } from "vitest";
import type { ModelsInfoOutput } from "./types";
import type { Notice } from "./notifications";

vi.mock("./api", () => ({ call: vi.fn(), joinPath: vi.fn(), openPath: vi.fn(), pickFolder: vi.fn() }));
import { call } from "./api";
import { app } from "./app.svelte";
const invoke = vi.mocked(call);
afterEach(() => { invoke.mockReset(); app.stop(); });

it("shares a single model request across settings and polling and retains state on failure", async () => {
  const snapshot: ModelsInfoOutput = { runtime: null, models_dir: "test", embed: [{ id: "test-shared", dim: 1, backend: "onnx", warnings: [], downloaded: false, downloading: true, download_percent: 37 }], rerank: [] };
  let finish!: (value: ModelsInfoOutput) => void;
  invoke.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  const first = app.loadModels();
  const second = app.loadModels();
  expect(invoke).toHaveBeenCalledTimes(1);
  expect(first).toBe(second);
  finish(snapshot); await first;
  expect(app.models).toEqual(snapshot);
  expect(app.notifications.some(n => n.id === "model:test-shared")).toBe(false);
  invoke.mockRejectedValueOnce(new Error("Нет ответа"));
  await expect(app.loadModels()).rejects.toThrow("Нет ответа");
  expect(app.notifications.some(n => n.id === "model:test-shared")).toBe(false);
  expect(app.models).toEqual(snapshot);
});

it("does not replace a newer project selection with a late notification response", async () => {
  let finish!: (value: unknown) => void;
  invoke.mockImplementation((method) => method === "operation_events" ? new Promise(resolve => { finish = resolve; }) : Promise.resolve({ docs_root: "docs", documents: [] }));
  const operation = { id: 91, sequence: 8, kind: "check", source: "explicit", project: "original", generation: 1, model: null, variant: null, reranker: false, state: "success", started_at: "2026-10-18T10:00:00Z", finished_at: "2026-10-18T10:00:01Z", docs_done: 1, docs_total: 1, percent: null, report: null, error: null, target_available: true };
  const item: Notice = { id: "test:91", title: "Индекс", text: "Готово", active: false, bad: false, unread: false, percent: null, project: "original", epoch: "test", operation };
  app.current = "before"; app.view = "main";
  const pending = app.visitNotification(item);
  await app.select("newer");
  finish({ epoch: "test", cursor: 8, gap: false, active: [], completed: [operation] });
  await pending;
  expect(app.current).toBe("newer"); expect(app.view).toBe("main");
  expect(app.toast?.text).toContain("переход отменён");
});
