import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import Sidebar from "./Sidebar.svelte";
import { app } from "./app.svelte";
import { buildTree } from "./tree";
import { splitView, viewKey } from "./view-key";
import type { ProjectInfo, WorktreeStatus } from "./types";

const api = vi.hoisted(() => ({ call: vi.fn(), treeRead: vi.fn(), worktreeObserve: vi.fn() }));
vi.mock("./api", () => ({ ...api, joinPath: vi.fn(), openPath: vi.fn(), pickFolder: vi.fn(), copyText: vi.fn(), documentAction: vi.fn(), documentEditors: vi.fn(), inTauri: () => false }));

const project: ProjectInfo = { name: "bars", path: "C:\\w\\bars", docs_root: "docs", state: "ready", modules: [] };
const docs = [
  { path: "docs/features/a.md", module: "features", title: "A", chunks: 2 },
  { path: "docs/arch/b.md", module: "arch", title: "B", chunks: 1 },
];
const quorum: WorktreeStatus = { name: "quorum", path: "C:\\w\\bars-wt-quorum", branch: "feat/quorum", locked: null, delta: { state: "ready", documents: 2, pending: 0 } };
const avatar: WorktreeStatus = { name: "avatar", path: "C:\\w\\bars-wt-avatar", branch: null, locked: null, delta: { state: "building", documents: 0, done: 12, total: 40, pending: 0 } };

const components: ReturnType<typeof mount>[] = [];
const settle = async () => { await new Promise((resolve) => setTimeout(resolve, 0)); flushSync(); };
const folder = () => document.querySelector<HTMLElement>('[data-testid="worktree-folder"]');
const items = () => Array.from(document.querySelectorAll<HTMLElement>('[data-testid="worktree-item"]'));

beforeEach(() => {
  api.call.mockImplementation(async (method: string) => {
    if (method === "tree") return { docs_root: "docs", documents: docs };
    if (method === "search") return { results: [], applied: { results: 5, candidates: 5, rerank_candidates: 0, min_score: 0, score_kind: "none", clamped: false }, indexing: false, pending: 0 };
    return { accepted: true };
  });
  api.treeRead.mockResolvedValue({ status: "ok", docs_root: "docs", revision: "r", dirs: [], files: docs.map((d) => ({ path: d.path, size: 1, modified: 1 })), problems: [] });
  api.worktreeObserve.mockResolvedValue(undefined);
  app.projects = [project]; app.current = project.path; app.activity = null; app.filter = "";
  app.tree = buildTree(docs, "docs");
  app.worktrees = [quorum, avatar];
  components.push(mount(Sidebar, { target: document.body })); flushSync();
});
afterEach(async () => {
  for (const component of components.splice(0)) await unmount(component);
  document.body.innerHTML = ""; app.clearSelection(); app.current = null; app.projects = []; app.worktrees = []; app.wtTrees = {};
  vi.clearAllMocks();
});

it("worktree_folder_in_tree_and_isolated_selection: папка «worktree», вид без смешивания выбора", async () => {
  // Папка «worktree» после модулей: число worktree справа, свёрнута.
  expect(folder()).not.toBeNull();
  expect(folder()!.querySelector(".head .ct")!.textContent).toBe("2");
  expect(items()).toHaveLength(0);
  folder()!.querySelector<HTMLButtonElement>(".head .caret")!.click(); flushSync();

  // По папке на worktree: имя, ветка (или detached) и состояние дельты.
  expect(items().map((item) => item.querySelector(".nm")!.textContent)).toEqual(["quorum", "avatar"]);
  expect(items().map((item) => item.querySelector(".tag")!.textContent)).toEqual(["feat/quorum", "detached"]);
  expect(items().map((item) => item.querySelector(".st")!.textContent)).toEqual(["готов, 2 отличаются", "строится 12 из 40"]);
  expect(items().every((item) => item.querySelector("input[type=checkbox]") === null)).toBe(true);

  // Раскрытие worktree — первое обращение: дерево по диску W и индексу вида, граф и аудиты неактивны.
  items()[0].querySelector<HTMLButtonElement>(".caret")!.click(); await settle();
  expect(api.call).toHaveBeenCalledWith("tree", expect.objectContaining({ project: project.path, wt: "quorum" }));
  expect(api.treeRead).toHaveBeenCalledWith(viewKey(project.path, "quorum"));
  expect(app.focusWt).toBe("quorum");
  const disabled = Array.from(items()[0].querySelectorAll<HTMLButtonElement>('[data-testid="worktree-disabled"]'));
  expect(disabled.map((button) => button.disabled)).toEqual([true, true, true]);
  expect(disabled.every((button) => button.title === "только у основного checkout")).toBe(true);
  const inside = () => Array.from(items()[0].querySelectorAll<HTMLInputElement>('.node.module input[type=checkbox]'));
  expect(inside()).toHaveLength(2);

  // Галочка в worktree: выбор принадлежит виду, основной checkout остаётся пустым.
  inside()[0].click(); await settle();
  expect(app.selectionView).toBe("quorum");
  expect(app.selected.size).toBe(1);
  expect(app.scope.modules).toHaveLength(1);
  const mainBoxes = () => Array.from(document.querySelectorAll<HTMLInputElement>('.tree > .node input[type=checkbox]'));
  expect(mainBoxes().every((box) => !box.checked)).toBe(true);
  expect(document.querySelector('[data-testid="selection-hint"]')!.textContent).toBe("Поиск идёт по одному виду");

  // Поиск идёт по виду, в котором стоят галочки.
  app.query = "кворум";
  await app.search();
  expect(api.call).toHaveBeenCalledWith("search", expect.objectContaining({ wt: "quorum", project: project.path }));

  // Галочка в основном checkout снимает выбор worktree и говорит об этом.
  app.toggleNode(app.tree[0].path, true);
  flushSync();
  expect(app.selectionView).toBeNull();
  expect(app.selected.size).toBe(1);
  expect(app.selectionHint).toContain("Поиск идёт по одному виду");
  expect(app.selectionHint).toContain("quorum");
  expect(inside().every((box) => !box.checked)).toBe(true);
  await app.search();
  const last = api.call.mock.calls.filter(([method]) => method === "search").at(-1)![1];
  expect(last).not.toHaveProperty("wt");
});

it("ключ вида разбирается на проект и worktree; основной checkout ключом не меняется", () => {
  expect(viewKey("C:\\w\\bars", null)).toBe("C:\\w\\bars");
  expect(splitView("C:\\w\\bars")).toEqual({ project: "C:\\w\\bars", wt: null });
  expect(splitView(viewKey("C:\\w\\bars", "quorum"))).toEqual({ project: "C:\\w\\bars", wt: "quorum" });
});

it("worktree, которого больше нет, снимает выбор в нём", () => {
  app.toggleNode("features", true, "quorum");
  expect(app.selectionView).toBe("quorum");
  (app as unknown as { applyWorktrees(list: WorktreeStatus[]): void }).applyWorktrees([avatar]);
  expect(app.selectionView).toBeNull();
  expect(app.selected.size).toBe(0);
});
