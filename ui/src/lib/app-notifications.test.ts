import { afterEach, expect, it, vi } from "vitest";
import type { ModelsInfoOutput } from "./types";

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
  expect(app.notifications.find(n => n.id === "model:test-shared")?.percent).toBe(37);
  invoke.mockRejectedValueOnce(new Error("Нет ответа"));
  await expect(app.loadModels()).rejects.toThrow("Нет ответа");
  expect(app.notifications.find(n => n.id === "model:test-shared")?.active).toBe(true);
  expect(app.models).toEqual(snapshot);
});
