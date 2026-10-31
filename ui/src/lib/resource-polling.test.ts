import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.mock("./api", () => ({ call: vi.fn(), treeRead: vi.fn().mockResolvedValue(null), inTauri: () => false }));
import { call } from "./api";
let app: typeof import("./app.svelte").app;
const invoke = vi.mocked(call);

beforeEach(async () => {
  vi.resetModules();
  app = (await import("./app.svelte")).app;
  app.current = null;
  vi.spyOn(document, "hidden", "get").mockReturnValue(false);
  invoke.mockImplementation(async (method) => {
    if (method === "activity") return { running: null, queue: [], problems: [] };
    if (method === "operation_events") return { epoch: "test", cursor: 0, gap: false, active: [], completed: [] };
    if (method === "projects") return { projects: [] };
    if (method === "models_info") return { runtime: null, models_dir: "test", embed: [], rerank: [] };
    if (method === "tree") return { docs_root: "docs", documents: [] };
    return {};
  });
});
afterEach(() => { app.stop(); vi.useRealTimers(); vi.restoreAllMocks(); invoke.mockReset(); });

it("does not reload model files on ordinary idle ticks", async () => {
  await app.refresh(false); await app.refresh(false);
  expect(invoke.mock.calls.filter(([method]) => method === "models_info")).toHaveLength(0);
  app.view = "settings";
  await app.refresh(true);
  expect(invoke.mock.calls.filter(([method]) => method === "models_info")).toHaveLength(1);
});

it("document windows retain the main project and have no general poll", async () => {
  localStorage.setItem("bars-project", "main-project");
  await app.bindDocumentWindow("document-project");
  invoke.mockClear(); vi.useFakeTimers();
  app.start(); await app.refresh(true); await app.loadProjects();
  await vi.advanceTimersByTimeAsync(20_000);
  expect(invoke.mock.calls.map(([method]) => method)).toEqual(["project_observe", "project_observe", "project_observe"]);
  expect(app.current).toBe("document-project");
  expect(localStorage.getItem("bars-project")).toBe("main-project");
});

it("hidden polling only reads notifications once per ten seconds", async () => {
  vi.useFakeTimers(); vi.setSystemTime(new Date("2026-10-25T12:00:00Z"));
  vi.spyOn(document, "hidden", "get").mockReturnValue(true);
  for (let i = 0; i < 11; i++) { await app.refresh(false); await vi.advanceTimersByTimeAsync(1000); }
  expect(invoke.mock.calls.map(([method]) => method)).toEqual(["activity", "operation_events", "activity", "operation_events"]);
});

it("the interval timer of a hidden window reads only notifications and showing it runs a full refresh", async () => {
  vi.useFakeTimers(); vi.setSystemTime(new Date("2026-10-28T12:00:00Z"));
  const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(true);
  app.start(); await vi.advanceTimersByTimeAsync(0);
  invoke.mockClear();
  await vi.advanceTimersByTimeAsync(9_000);
  const methods = () => invoke.mock.calls.map(([method]) => method).filter(method => method !== "project_observe");
  expect(methods()).toEqual(["activity", "operation_events"]);
  invoke.mockClear();
  hidden.mockReturnValue(false);
  document.dispatchEvent(new Event("visibilitychange"));
  await vi.advanceTimersByTimeAsync(0);
  expect(methods()).toEqual(expect.arrayContaining(["activity", "operation_events", "projects"]));
});

it("a forced refresh received during a request runs immediately afterward", async () => {
  let finish!: (value: unknown) => void;
  invoke.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  const first = app.refresh(false);
  const forced = app.refresh(true);
  expect(invoke.mock.calls.filter(([method]) => method === "activity")).toHaveLength(1);
  finish({ running: null, queue: [], problems: [] });
  await first;
  await forced;
  expect(invoke.mock.calls.filter(([method]) => method === "activity")).toHaveLength(2);
});

it("manual check shows the checking state until the deferred refresh completes", async () => {
  let finish!: (value: unknown) => void;
  invoke.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  const tick = app.refresh(false);
  const check = app.recheck();
  await vi.waitFor(() => expect(app.checking).toBe(true));
  finish({ running: null, queue: [], problems: [] });
  await tick;
  await check;
  expect(app.checking).toBe(false);
  expect(invoke.mock.calls.filter(([method]) => method === "check_all")).toHaveLength(1);
  expect(invoke.mock.calls.filter(([method]) => method === "activity")).toHaveLength(2);
});

it("a second manual check is ignored while one is running", async () => {
  const first = app.recheck();
  await app.recheck();
  await first;
  expect(invoke.mock.calls.filter(([method]) => method === "check_all")).toHaveLength(1);
});

it("window observation follows project selection, survives hiding and releases on stop", async () => {
  vi.useFakeTimers();
  await app.bindDocumentWindow("one");
  app.start();
  const initial = invoke.mock.calls.find(([method]) => method === "project_observe")![1]!;
  expect(initial.project).toBe("one");
  invoke.mockClear();
  vi.spyOn(document, "hidden", "get").mockReturnValue(true);
  await app.select("two");
  expect(invoke).toHaveBeenCalledWith("project_observe", { window: initial.window, project: "two" });
  invoke.mockClear();
  await vi.advanceTimersByTimeAsync(10_000);
  expect(invoke).toHaveBeenCalledWith("project_observe", { window: initial.window, project: "two" });
  app.stop();
  expect(invoke).toHaveBeenLastCalledWith("project_observe", { window: initial.window, project: null });
});
