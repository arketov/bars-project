import { describe, expect, it } from "vitest";
import {
  buildTree,
  checkState,
  filterTree,
  relativeToDocs,
  searchScope,
  toggle,
  touchedModules,
} from "./tree";
import type { TreeDocument } from "./types";

const doc = (path: string, chunks = 1): TreeDocument => ({ path, module: "", title: path, chunks });

const DOCS: TreeDocument[] = [
  doc("docs/Index.md", 2),
  doc("docs/search/Index.md"),
  doc("docs/search/hybrid-search.md", 6),
  doc("docs/storage/files.md", 5),
  doc("docs/knowledge/spikes/fts5-stemming.md", 5),
  doc("docs/knowledge/spikes/ort-build.md", 4),
  doc("docs/knowledge/stack-research.md", 16),
];

const tree = () => buildTree(DOCS, "docs");

describe("buildTree", () => {
  it("делит документы на модули, папки и файлы, считает чанки", () => {
    const modules = tree();
    expect(modules.map((m) => m.path)).toEqual([".", "knowledge", "search", "storage"]);
    expect(modules[0].label).toBe("(корень)");
    expect(modules[0].children.map((c) => c.path)).toEqual(["./Index.md"]);
    const knowledge = modules[1];
    expect(knowledge.chunks).toBe(25);
    // Папка раньше файла.
    expect(knowledge.children.map((c) => [c.path, c.kind])).toEqual([
      ["knowledge/spikes", "dir"],
      ["knowledge/stack-research.md", "file"],
    ]);
    expect(knowledge.children[0].chunks).toBe(9);
  });

  it("Index.md идёт первым среди файлов, имена — для людей", () => {
    const search = tree()[2];
    expect(search.children.map((c) => c.label)).toEqual(["Index", "Hybrid search"]);
  });

  it("путь документа считается от корня документации", () => {
    expect(relativeToDocs("docs/a/b.md", "docs")).toBe("a/b.md");
    expect(relativeToDocs("a/b.md", ".")).toBe("a/b.md");
    expect(relativeToDocs("documentation/x.md", "documentation/")).toBe("x.md");
  });
});

describe("галочки", () => {
  it("модуль отмечает всё внутри, частичный выбор — «some»", () => {
    const modules = tree();
    let sel = toggle(modules, new Set(), "search", true);
    expect(checkState(modules[2], sel)).toBe("all");
    expect(checkState(modules[3], sel)).toBe("none");
    sel = toggle(modules, new Set(), "knowledge/spikes/ort-build.md", true);
    expect(checkState(modules[1], sel)).toBe("some");
  });

  it("снятие файла внутри отмеченного модуля отмечает соседей", () => {
    const modules = tree();
    const sel = toggle(modules, new Set(["knowledge"]), "knowledge/spikes/ort-build.md", false);
    expect([...sel].sort()).toEqual(["knowledge/spikes/fts5-stemming.md", "knowledge/stack-research.md"]);
    expect(checkState(modules[1], sel)).toBe("some");
  });

  it("отметка папки поглощает отмеченное внутри неё", () => {
    const modules = tree();
    const sel = toggle(modules, new Set(["knowledge/spikes/ort-build.md"]), "knowledge/spikes", true);
    expect([...sel]).toEqual(["knowledge/spikes"]);
  });

  it("корневой модуль покрывает только файлы корня", () => {
    const modules = tree();
    const sel = toggle(modules, new Set(), ".", true);
    expect(checkState(modules[0], sel)).toBe("all");
    expect(checkState(modules[2], sel)).toBe("none");
  });

  it("снятие единственного файла корня очищает выбор", () => {
    const modules = tree();
    const sel = toggle(modules, new Set(["."]), "./Index.md", false);
    expect(sel.size).toBe(0);
  });
});

describe("область поиска", () => {
  it("целиком отмеченные модули идут в modules, остальное — файлами", () => {
    const modules = tree();
    const sel = new Set(["search", "knowledge/spikes/ort-build.md"]);
    expect(searchScope(modules, sel)).toEqual({
      modules: ["search"],
      files: ["knowledge/spikes/ort-build.md"],
    });
    expect(touchedModules(modules, sel).map((m) => m.path)).toEqual(["knowledge", "search"]);
  });

  it("ничего не отмечено — пустая область: весь проект", () => {
    expect(searchScope(tree(), new Set())).toEqual({ modules: [], files: [] });
  });

  it("корневой модуль уходит как `.`", () => {
    expect(searchScope(tree(), new Set(["."])).modules).toEqual(["."]);
  });
});

describe("фильтр", () => {
  it("ищет по показанному имени и по пути, совпавшая папка видна целиком", () => {
    const byLabel = filterTree(tree(), "hybrid");
    expect(byLabel.map((m) => m.path)).toEqual(["search"]);
    expect(byLabel[0].children.map((c) => c.path)).toEqual(["search/hybrid-search.md"]);
    const byPath = filterTree(tree(), "knowledge/spikes");
    expect(byPath[0].children[0].children).toHaveLength(2);
    expect(filterTree(tree(), "нет такого")).toEqual([]);
    expect(filterTree(tree(), "  ")).toHaveLength(4);
  });
});
