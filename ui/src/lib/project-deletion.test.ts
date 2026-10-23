import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import DeleteProjectDialog from "./DeleteProjectDialog.svelte";
import { app } from "./app.svelte";
import { call } from "./api";
import type { ProjectsOutput, SearchOutput } from "./types";

vi.mock("./api", () => ({ call: vi.fn(), joinPath: vi.fn(), openPath: vi.fn(), pickFolder: vi.fn(), inTauri: () => false }));
const invoke = vi.mocked(call);
const first = { name: "Одинаковое имя", path: "C:/первый проект", docs_root: "docs", state: "ready" as const, modules: [] };
const other = { ...first, path: "D:/другой проект" };
const mounted: ReturnType<typeof mount>[] = [];
const settle = async () => { await new Promise(resolve => setTimeout(resolve, 0)); flushSync(); };
function show() {
  const host = document.createElement("div"); document.body.append(host);
  mounted.push(mount(DeleteProjectDialog, { target: host })); flushSync(); return host;
}
beforeEach(() => {
  HTMLDialogElement.prototype.showModal = function() { this.setAttribute("open", ""); };
  app.projects = [first, other]; app.current = first.path; app.deletingProject = null; app.deleteBusy = false;
  app.deleteError = null; app.results = null; app.searching = false; app.activity = null;
  invoke.mockImplementation(async method => {
    if (method === "tree") return { docs_root: "docs", documents: [] };
    if (method === "status") return null;
    if (method === "project_delete") return { removed: true };
    throw new Error(`unexpected ${method}`);
  });
});
afterEach(async () => {
  for (const component of mounted.splice(0)) await unmount(component);
  app.cancelDeletion(); app.stop(); app.projects = []; app.current = null;
  document.body.replaceChildren(); localStorage.clear(); vi.clearAllMocks();
});

it("dialog names exact project; initial focus, Escape and cancel never delete", () => {
  app.requestDeleteProject(other.path); const host = show();
  expect(host.textContent).toContain(other.path); expect(host.textContent).toContain("BARS_INSTRUCTION.md останутся");
  expect(document.activeElement?.textContent).toBe("Отмена");
  host.querySelector("dialog")!.dispatchEvent(new Event("cancel", { cancelable: true }));
  expect(app.deletingProject).toBeNull(); expect(invoke).not.toHaveBeenCalled();
  app.requestDeleteProject(other.path); app.cancelDeletion(); expect(invoke).not.toHaveBeenCalled();
});

it("deletes frozen target even if selection changes; double submit and cancellation blocked", async () => {
  app.requestDeleteProject(first.path); app.current = other.path;
  let done!: (value: unknown) => void;
  invoke.mockImplementationOnce(() => new Promise(resolve => { done = resolve; }));
  const deleting = app.confirmDeletion(); await app.confirmDeletion(); app.cancelDeletion();
  expect(app.deleteBusy).toBe(true); expect(app.deletingProject?.path).toBe(first.path);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("project_delete", { project: first.path, confirmed: true });
  done({ removed: true }); await deleting;
  expect(app.projects).toEqual([other]); expect(app.current).toBe(other.path);
});

it("keeps failure in dialog and permits retry; success clears last project and persisted selection", async () => {
  app.projects = [first]; localStorage.setItem("bars-project", first.path);
  app.requestDeleteProject(first.path);
  invoke.mockRejectedValueOnce(new Error("Нет доступа"));
  const host = show(); await app.confirmDeletion(); flushSync();
  expect(host.querySelector('[role="alert"]')?.textContent).toBe("Нет доступа");
  expect(app.projects).toHaveLength(1); expect(app.deleteBusy).toBe(false);
  await app.confirmDeletion();
  expect(app.deletingProject).toBeNull(); expect(app.current).toBeNull(); expect(app.tree).toEqual([]);
  expect(app.results).toBeNull(); expect(app.viewer).toBeNull(); expect(localStorage.getItem("bars-project")).toBeNull();
});

it("late project list and search cannot restore deleted project data", async () => {
  app.projects = [first]; app.query = "вопрос";
  let list!: (value: ProjectsOutput) => void, search!: (value: SearchOutput) => void;
  invoke.mockImplementation(async method => {
    if (method === "projects") return new Promise(resolve => { list = resolve; });
    if (method === "search") return new Promise(resolve => { search = resolve; });
    if (method === "project_delete") return { removed: true };
    throw new Error(method);
  });
  const listing = app.loadProjects(), searching = app.search();
  app.requestDeleteProject(first.path); await app.confirmDeletion();
  list({ projects: [first] }); search({ results: [], applied: { results: 5, candidates: 30, rerank_candidates: 15, min_score: 0, score_kind: "cosine", clamped: false }, indexing: false, pending: 0 });
  await Promise.all([listing, searching]);
  expect(app.projects).toEqual([]); expect(app.current).toBeNull(); expect(app.results).toBeNull(); expect(app.searching).toBe(false);
});
