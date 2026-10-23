import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { app } from "./app.svelte";
import { buildTree, projectPathOf } from "./tree";
import { destinations, entries, parentNodePath } from "./entries.svelte";

const api = vi.hoisted(() => ({
  call: vi.fn(),
  treeRead: vi.fn(),
  entryCreateDoc: vi.fn(),
  entryCreateDir: vi.fn(),
  entryMove: vi.fn(),
  entryTrash: vi.fn(),
  inTauri: () => false,
}));
vi.mock("./api", () => ({ ...api, joinPath: (a: string, b: string) => `${a}/${b}`, openPath: vi.fn(), pickFolder: vi.fn() }));

const project = { path: "C:/Проект", name: "Проект", docs_root: "docs", state: "ready" as const, modules: [] };
const documents = [
  { path: "docs/a/x.md", module: "a", title: "x", chunks: 2 },
  { path: "docs/a/b/y.md", module: "a", title: "y", chunks: 1 },
  { path: "docs/root.md", module: ".", title: "root", chunks: 1 },
];
const disk = {
  files: ["docs/a/x.md", "docs/a/b/y.md", "docs/root.md", "docs/new.md", "docs/a/fresh.md"],
  dirs: ["docs/a", "docs/a/b", "docs/empty", "docs/a/empty-inner"],
};

describe("дерево по диску (Т-33)", () => {
  it("показывает пустые папки и документы без индекса, прячет исчезнувшие с диска", () => {
    const tree = buildTree([...documents, { path: "docs/gone.md", module: ".", title: "gone", chunks: 3 }], "docs", disk);
    const names = (path: string) => tree.find((m) => m.path === path)?.children.map((c) => c.name);
    expect(tree.map((m) => m.path)).toEqual([".", "a", "empty"]);
    expect(names(".")).toEqual(["new.md", "root.md"]);
    expect(names("a")).toEqual(["b", "empty-inner", "fresh.md", "x.md"]);
    const fresh = tree[1].children.find((c) => c.name === "fresh.md")!;
    expect(fresh.indexed).toBe(false);
    expect(fresh.chunks).toBe(0);
    expect(fresh.documentPath).toBe("docs/a/fresh.md");
    expect(tree[1].children.find((c) => c.name === "x.md")!.indexed).toBeUndefined();
  });

  it("без диска строит дерево по индексу, как раньше", () => {
    const tree = buildTree(documents, "docs");
    expect(tree.map((m) => m.path)).toEqual([".", "a"]);
    expect(tree[1].indexPath).toBeUndefined();
  });

  it("папка без index.md получает путь файла папки, папка с ним — его путь (Т-32)", () => {
    const tree = buildTree([...documents, { path: "docs/a/Index.md", module: "a", title: "a", chunks: 1 }], "docs", {
      files: [...disk.files, "docs/a/Index.md"],
      dirs: disk.dirs,
    });
    const a = tree.find((m) => m.path === "a")!;
    expect(a.indexPath).toBe("docs/a/Index.md");
    expect(a.children.find((c) => c.path === "a/b")!.indexPath).toBe("docs/a/b/index.md");
    expect(tree.find((m) => m.path === "empty")!.indexPath).toBe("docs/empty/index.md");
    expect(tree.find((m) => m.path === ".")!.indexPath).toBeUndefined();
  });

  it("корень документации «.»: пути проекта без префикса", () => {
    const tree = buildTree([{ path: "a/x.md", module: "a", title: "x", chunks: 1 }], ".", { files: ["a/x.md"], dirs: ["a", "b"] });
    expect(tree.map((m) => m.path)).toEqual(["a", "b"]);
    expect(tree[1].indexPath).toBe("b/index.md");
    expect(projectPathOf("a/x.md", ".")).toBe("a/x.md");
    expect(projectPathOf(".", ".")).toBe("");
    expect(projectPathOf("./root.md", "manual/docs")).toBe("manual/docs/root.md");
    expect(projectPathOf(".", "docs")).toBe("docs");
  });
});

describe("операции с файлами и папками", () => {
  beforeEach(() => {
    app.current = project.path;
    app.projects = [project];
    app.docsRoot = "docs";
    app.tree = buildTree(documents, "docs", disk);
    vi.spyOn(app, "loadTree").mockResolvedValue();
    vi.spyOn(app, "openDocument").mockResolvedValue();
    app.selected = new Set(["a/x.md", "gone"]);
  });
  afterEach(() => {
    entries.cancel();
    app.current = null;
    app.tree = [];
    vi.restoreAllMocks();
    vi.clearAllMocks();
  });

  it("список папок назначения не содержит переносимую папку и её потомков", () => {
    const moving = app.tree.find((m) => m.path === "a")!;
    expect(destinations(app.tree, moving).map((d) => d.path)).toEqual([".", "empty"]);
    const file = app.tree[1].children.find((c) => c.name === "x.md")!;
    expect(destinations(app.tree, file).map((d) => d.path)).toEqual([".", "a", "a/b", "a/empty-inner", "empty"]);
    expect(parentNodePath(file)).toBe("a");
    expect(parentNodePath(app.tree[0].children[0])).toBe(".");
  });

  it("новый документ создаётся в папке по пути проекта и открывается", async () => {
    api.entryCreateDoc.mockResolvedValue({ status: "ok", path: "docs/a/n.md" });
    entries.open("newDoc", "a");
    expect(entries.request?.docs).toBe(3);
    await entries.submit("  n  ");
    expect(api.entryCreateDoc).toHaveBeenCalledWith(project.path, "docs/a", "n");
    expect(app.loadTree).toHaveBeenCalled();
    expect(app.openDocument).toHaveBeenCalledWith(expect.objectContaining({ path: "docs/a/n.md" }));
    expect(entries.request).toBeNull();
  });

  it("новая папка открывает её index.md; частичный успех сообщает причину", async () => {
    api.entryCreateDir.mockResolvedValue({ status: "ok", path: "docs/m", index_path: "docs/m/index.md" });
    entries.open("newDir", ".");
    await entries.submit("m");
    expect(api.entryCreateDir).toHaveBeenCalledWith(project.path, "docs", "m");
    expect(app.openDocument).toHaveBeenCalledWith(expect.objectContaining({ path: "docs/m/index.md" }));
    api.entryCreateDir.mockResolvedValue({ status: "partial", path: "docs/n", index_path: "docs/n/index.md", kind: "io", message: "диск полон" });
    entries.open("newDir", ".");
    await entries.submit("n");
    expect(app.toast?.text).toContain("диск полон");
    expect(app.toast?.bad).toBe(true);
  });

  it("отказ команды остаётся в диалоге с текстом причины, дерево не перечитывается", async () => {
    api.entryCreateDoc.mockResolvedValue({ status: "error", kind: "exists", message: "«x.md» уже есть в этой папке" });
    entries.open("newDoc", "a");
    await entries.submit("x");
    expect(entries.error).toBe("«x.md» уже есть в этой папке");
    expect(entries.request).not.toBeNull();
    expect(app.loadTree).not.toHaveBeenCalled();
  });

  it("переименование — перенос в ту же папку; перенос сохраняет имя; отметки пропавших узлов снимаются", async () => {
    api.entryMove.mockResolvedValue({ status: "ok", path: "docs/a/z.md" });
    entries.open("rename", "a/x.md");
    await entries.submit("z.md");
    expect(api.entryMove).toHaveBeenLastCalledWith(project.path, "docs/a/x.md", "docs/a", "z.md");
    entries.open("move", "a/x.md");
    await entries.submit("a/b");
    expect(api.entryMove).toHaveBeenLastCalledWith(project.path, "docs/a/x.md", "docs/a/b", "x.md");
    expect([...app.selected]).toEqual(["a/x.md"]);
  });

  it("файл корня: родитель — корень документации", async () => {
    api.entryMove.mockResolvedValue({ status: "ok", path: "docs/r.md" });
    entries.open("rename", "./root.md");
    await entries.submit("r.md");
    expect(api.entryMove).toHaveBeenCalledWith(project.path, "docs/root.md", "docs", "r.md");
  });

  it("удаление идёт в корзину; корень документации открыть для переименования нельзя", async () => {
    api.entryTrash.mockResolvedValue({ status: "ok", path: "docs/a" });
    entries.open("trash", "a");
    expect(entries.request?.docs).toBe(3);
    await entries.submit("");
    expect(api.entryTrash).toHaveBeenCalledWith(project.path, "docs/a");
    expect(app.toast?.text).toBe("Перемещено в корзину");
  });
});
