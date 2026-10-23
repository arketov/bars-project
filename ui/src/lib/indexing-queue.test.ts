import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import IndexingPage from "./IndexingPage.svelte";
import { app } from "./app.svelte";
import type { ActivityOutput, ProjectDocuments } from "./types";

vi.mock("./api", async original => ({ ...await original<typeof import("./api")>(), call: vi.fn(), joinPath: vi.fn(), openPath: vi.fn(), pickFolder: vi.fn() }));
import { call } from "./api";
const invoke = vi.mocked(call);
const project = { path: "C:/проект", name: "Проект", docs_root: "docs", state: "ready" as const, modules: [{ name: "manual", documents: 10 }] };
const queue = (over: Partial<ProjectDocuments> = {}): ProjectDocuments => ({
  project: project.path,
  documents: [
    { path: "docs/a.md", state: "waiting", source: "edit", due_in: 18, error: null },
    { path: "docs/b.md", state: "queued", source: "watch", due_in: null, error: null },
    { path: "docs/c.md", state: "error", source: "watch", due_in: null, error: "Провайдер вернул 503" },
  ],
  total: 3, waiting: 1, queued: 1, indexing: 0, error: 1, ...over,
});
const activity = (documents: ProjectDocuments[]): ActivityOutput => ({ paused: false, running: null, queue: [], problems: [], documents });
let component: ReturnType<typeof mount> | undefined;
const settle = async () => { await new Promise(resolve => setTimeout(resolve, 0)); flushSync(); };

beforeEach(() => {
  app.current = project.path; app.projects = [project]; app.view = "indexing";
  app.activity = activity([queue()]);
  app.workerDown = null; app.indexingHistory = null; app.indexingHistoryError = null; app.indexingHistoryLoading = false;
  invoke.mockImplementation(async method => {
    if (method === "activity") return activity([queue({ documents: queue().documents.filter(d => d.state !== "error"), total: 2, error: 0 })]);
    if (method === "index_retry") return { retried: true };
    return { epoch: "t", cursor: 0, active: [], completed: [], gap: false };
  });
});
afterEach(async () => {
  if (component) await unmount(component); component = undefined;
  document.body.replaceChildren(); app.stop(); invoke.mockReset(); vi.restoreAllMocks();
});

it("shows document states, the remaining pause and the failure reason", () => {
  component = mount(IndexingPage, { target: document.body }); flushSync();
  const text = document.body.textContent ?? "";
  expect(text).toContain("Документы");
  expect(text).toContain("Ждут паузы: 1 · в очереди: 1 · идёт embed: 0 · с ошибкой: 1");
  expect(text).toContain("docs/a.md");
  expect(text).toContain("embed через 18 с");
  expect(text).toContain("правка в bars");
  expect(text).toContain("Провайдер вернул 503");
  expect(text).toContain("Правки документов ждут паузы перед индексацией: 1");
});

it("retries a failed document and shows the refreshed list", async () => {
  component = mount(IndexingPage, { target: document.body }); flushSync();
  [...document.querySelectorAll("button")].find(b => b.textContent === "Повторить")!.click();
  await settle(); await settle();
  expect(invoke).toHaveBeenCalledWith("index_retry", { project: project.path, path: "docs/c.md" });
  expect(document.body.textContent).not.toContain("Провайдер вернул 503");
});

it("shows nothing without records or for an old worker without the field", () => {
  app.activity = activity([]);
  component = mount(IndexingPage, { target: document.body }); flushSync();
  expect(document.querySelector(".docs")).toBeNull();
  app.activity = { paused: false, running: null, queue: [], problems: [] }; flushSync();
  expect(document.querySelector(".docs")).toBeNull();
});

it("says how many records are hidden by the 200-record limit", () => {
  app.activity = activity([queue({ total: 250 })]);
  component = mount(IndexingPage, { target: document.body }); flushSync();
  expect(document.body.textContent).toContain("Показано 3 из 250");
});
