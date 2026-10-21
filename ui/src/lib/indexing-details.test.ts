import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import IndexingPage from "./IndexingPage.svelte";
import SearchPane from "./SearchPane.svelte";
import App from "../App.svelte";
import { theme } from "./theme.svelte";
import { app } from "./app.svelte";
import type { Operation, OperationEventsOutput } from "./types";

vi.mock("./api", async original => ({ ...await original<typeof import("./api")>(), call: vi.fn(), joinPath: vi.fn(), openPath: vi.fn(), pickFolder: vi.fn() }));
vi.mock("./graph-renderer", () => ({ GraphRenderer: class { set() {} destroy() {} } }));
import { call } from "./api";
const invoke = vi.mocked(call);
const project = { path: "C:/проект", name: "Проект", docs_root: "docs", state: "ready" as const, modules: [{ name: "manual", documents: 10 }] };
const result: Operation = { id: 5, sequence: 10, project: project.path, generation: 1, kind: "check", source: "explicit", model: null, variant: null, reranker: false, state: "success", started_at: "2026-10-20T10:00:00Z", finished_at: "2026-10-20T10:00:01Z", docs_done: 10, docs_total: 10, percent: null, report: { added: 2, changed: 1, removed: 0, unchanged: 7, problems: 0, index_applied: true }, error: null, target_available: true };
const events = (completed = [result]): OperationEventsOutput => ({ epoch: "test", cursor: 10, active: [], completed, gap: false });
let component: ReturnType<typeof mount> | undefined;
const settle = async () => { await new Promise(resolve => setTimeout(resolve, 0)); flushSync(); };
beforeEach(() => {
  app.current = project.path; app.projects = [project]; app.view = "main";
  app.activity = { paused: false, running: null, queue: [], problems: [] };
  app.workerDown = null; app.viewer = null; app.topicsOpen = false; app.consentOpen = false; app.deletingProject = null;
  app.topics.bind(null);
  app.indexingHistory = null; app.indexingHistoryError = null; app.indexingHistoryLoading = false;
  invoke.mockImplementation(async method => method === "operation_events" ? events() : { docs_root: "docs", documents: [], topics: [] });
});
afterEach(async () => {
  if (component) await unmount(component); component = undefined;
  document.body.replaceChildren(); app.stop(); invoke.mockReset(); vi.restoreAllMocks();
});

it("renders running and queued jobs of the same project without duplicate keys", () => {
  app.activity!.running = { ...project, project: project.path, kind: "check", priority: "explicit", docs_done: 2, docs_total: 10, chunks_done: 3, chunks_per_second: 1, elapsed_seconds: 2, current_file: null, embed_device: "cpu", attempt: 0 };
  app.activity!.queue = [{ project: project.path, name: project.name, kind: "files", priority: "watch", files: 1 }];
  component = mount(IndexingPage, { target: document.body }); flushSync();
  expect(document.body.textContent).toContain("Идёт индексация");
  expect(document.querySelectorAll(".q")).toHaveLength(2);
});

it("opens details from the project while history is blocked, then shows the last result", async () => {
  app.activity!.queue = [{ project: project.path, name: project.name, kind: "check", priority: "explicit", files: 3 }];
  let finish!: (value: OperationEventsOutput) => void;
  invoke.mockImplementation(() => new Promise(resolve => { finish = resolve; }));
  component = mount(SearchPane, { target: document.body }); flushSync();
  [...document.querySelectorAll("button")].find(b => b.textContent === "Подробнее")!.click();
  expect(app.view).toBe("indexing");
  await unmount(component); component = mount(IndexingPage, { target: document.body }); flushSync();
  expect(document.body.textContent).toContain("В очереди");
  // Первое чтение получает эпоху, второе — историю.
  finish(events()); await settle(); finish(events()); await settle();
  app.activity!.queue = []; flushSync();
  expect(document.body.textContent).toContain("Индекс обновлён · добавлено 2, изменено 1, удалено 0");
});

it("opens a notification before a busy tree/status response", async () => {
  app.current = "другой";
  invoke.mockImplementation(method => method === "operation_events" ? Promise.resolve(events()) : new Promise(() => {}));
  await app.visitNotification({ id: "test:5", epoch: "test", operation: result, project: project.path, title: "Индекс", text: "Итог", active: false, bad: false, unread: false, percent: null });
  expect(app.view).toBe("indexing"); expect(app.current).toBe(project.path);
});

it.each(["success", "partial", "error", "cancelled"])("shows confirmed %s outcome after the job disappears", async state => {
  const op = { ...result, state, error: state === "error" ? "Ошибка модели" : null, report: state === "success" ? result.report : null };
  invoke.mockResolvedValue(events([op]));
  app.openIndexing(); await settle();
  component = mount(IndexingPage, { target: document.body }); flushSync();
  expect(document.body.textContent).toContain("Последний проход");
  expect(document.body.textContent).not.toContain("Индексация не идёт: индекс актуален");
  if (state === "error") expect(document.body.textContent).toContain("Ошибка модели");
});

it("does not claim an up-to-date index without a result or after a connection failure", async () => {
  invoke.mockResolvedValue(events([])); app.openIndexing(); await settle();
  component = mount(IndexingPage, { target: document.body }); flushSync();
  expect(document.body.textContent).toContain("Подтверждённый итог недоступен");
  invoke.mockRejectedValue(new Error("Воркер занят")); app.openIndexing(); await settle();
  expect(document.body.textContent).toContain("Воркер занят");
  expect(document.body.textContent).not.toContain("индекс актуален");
});

it("opens the indexing screen in the full app and keeps it visible on connection loss", async () => {
  vi.spyOn(app, "start").mockImplementation(() => {});
  vi.spyOn(theme, "start").mockImplementation(() => {});
  app.activity!.queue = [{ project: project.path, name: project.name, kind: "check", priority: "explicit", files: 3 }];
  component = mount(App, { target: document.body }); flushSync();
  [...document.querySelectorAll("button")].find(b => b.textContent === "Подробнее")!.click(); await settle();
  expect(document.body.textContent).toContain("Очередь индексации");
  app.workerDown = "Нет ответа"; flushSync();
  expect(document.body.textContent).toContain("Нет связи с воркером");
  expect(document.body.textContent).toContain("Очередь индексации");
});

it("reads worker history even after notification cards were cleared and ignores unavailable targets", async () => {
  const newer = { ...result, id: 6, sequence: 11, report: { ...result.report!, added: 8 } };
  invoke.mockResolvedValue(events([result, { ...newer, target_available: false }]));
  app.clearNotifications(); app.openIndexing(); await settle();
  expect(app.latestIndexingResult(project.path)?.id).toBe(result.id);
  invoke.mockResolvedValue(events([result, newer])); app.openIndexing(); await settle();
  expect(app.latestIndexingResult(project.path)?.id).toBe(newer.id);
});

it("reads the new epoch after worker restart rather than showing a previous worker result", async () => {
  app.indexingHistory = events();
  invoke.mockResolvedValueOnce({ ...events([]), epoch: "new" }).mockResolvedValueOnce({ ...events([]), epoch: "new" });
  app.openIndexing(); await settle();
  expect(invoke).toHaveBeenLastCalledWith("operation_events", { epoch: "new", after: 0 });
  expect(app.latestIndexingResult(project.path)).toBeNull();
});

it("updates the visible result when the polled job finishes", async () => {
  app.indexingHistory = events([]); app.view = "indexing";
  invoke.mockImplementation(async method => {
    if (method === "operation_events") return events();
    if (method === "activity") return { paused: false, running: null, queue: [], problems: [] };
    if (method === "projects") return { projects: [project] };
    if (method === "models_info") return { runtime: null, models_dir: "test", embed: [], rerank: [] };
    return { docs_root: "docs", documents: [] };
  });
  component = mount(IndexingPage, { target: document.body }); flushSync();
  await app.refresh(true); await settle();
  expect(document.body.textContent).toContain("Индекс обновлён · добавлено 2");
});

it("navigates from the notification center to the correct project while its tree is busy", async () => {
  vi.spyOn(app, "start").mockImplementation(() => {});
  vi.spyOn(theme, "start").mockImplementation(() => {});
  app.projects = [project, { ...project, path: "другой", name: "Другой" }]; app.current = "другой";
  app.notifications = [{ id: "test:5", epoch: "test", operation: result, project: project.path, title: "Индекс", text: "Итог", active: false, bad: false, unread: false, percent: null }];
  invoke.mockImplementation(method => method === "operation_events" ? Promise.resolve(events()) : new Promise(() => {}));
  component = mount(App, { target: document.body }); flushSync();
  document.querySelector<HTMLButtonElement>(".bell")!.click(); await settle();
  [...document.querySelectorAll("button")].find(b => b.textContent === "Подробнее")!.click(); flushSync();
  [...document.querySelectorAll("button")].find(b => b.textContent?.includes("Ход индексации проекта"))!.click(); await settle();
  expect(app.current).toBe(project.path);
  expect(document.querySelector('[data-panel-kind="indexing"] h1')?.textContent).toBe(project.name);
  expect(document.body.textContent).toContain("Индекс обновлён · добавлено 2");
  expect(document.querySelector("#notifications-panel")).toBeNull();
});
