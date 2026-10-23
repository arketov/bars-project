import { describe, expect, it } from "vitest";
import { cleanTitle, linkOptions, linkTargets, resolveWikilink, semanticOptions, wikiName, wikiTarget, type LinkTarget } from "./link-picker";

it("поиск е/ё остаётся нечётким, но разные имена допускают отдельную ссылку", () => {
  const all = [{path:"docs/Все.md",docs:"Все.md",title:"Все"}];
  expect(linkOptions(all,"Всё").some(option => option.create)).toBe(true);
  expect(resolveWikilink(all,"docs/x.md","Всё")).toBeNull();
});
import { buildTree } from "./tree";

const target = (docs: string, title: string): LinkTarget => ({ path: `docs/${docs}`, docs, title });
const targets: LinkTarget[] = [
  target("features/editor/design.md", "Проект редактора"),
  target("MAIN_IDEA.md", "MAIN_IDEA — требования"),
  target("features/search/hybrid search.md", "Гибридный поиск"),
  target("features/a/plan.md", "План A"),
  target("features/b/plan.md", "План B"),
];

describe("ссылки как в Obsidian (Т-30)", () => {
  it("существующий документ вставляется как [[имя файла]] без .md", () => {
    const [first] = linkOptions(targets, "редактор");
    expect(first).toMatchObject({ create: false, insert: "[[design]]", detail: "features/editor/design.md" });
    expect(linkOptions(targets, "гибрид")[0].insert).toBe("[[hybrid search]]");
  });

  it("одинаковые имена файлов — путь от корня документации", () => {
    expect(wikiName(targets[3], targets)).toBe("features/a/plan");
    expect(wikiName(targets[4], targets)).toBe("features/b/plan");
    expect(wikiName(targets[0], targets)).toBe("design");
  });

  it("названия, которого нет, предлагают создать ссылку в никуда", () => {
    const options = linkOptions(targets, "Новая тема");
    expect(options).toEqual([{ label: "Создать ссылку на «Новая тема»", detail: "файла пока нет", insert: "[[Новая тема]]", create: true }]);
  });

  it("при точном совпадении названия или имени файла пункта «создать» нет", () => {
    expect(linkOptions(targets, "проект редактора").some((o) => o.create)).toBe(false);
    expect(linkOptions(targets, "Design").some((o) => o.create)).toBe(false);
  });

  it("пустой ввод показывает документы без пункта «создать»", () => {
    const options = linkOptions(targets, "");
    expect(options).toHaveLength(targets.length);
    expect(options.some((o) => o.create)).toBe(false);
  });

  it("из введённого названия убираются знаки, запрещённые в [[…]]", () => {
    expect(cleanTitle("  a [b] | c # d ^e ")).toBe("a b c d e");
    expect(linkOptions(targets, "x]]y").at(-1)?.insert).toBe("[[x y]]");
  });

  it("имя из ссылки: псевдоним, раздел и .md отбрасываются", () => {
    expect(wikiTarget("Название|текст")).toBe("Название");
    expect(wikiTarget("папка/Название#Раздел")).toBe("папка/Название");
    expect(wikiTarget("Файл.md")).toBe("Файл");
    expect(wikiTarget("#Раздел")).toBeNull();
  });

  it("разбор ссылки: регистр не важен, путь сверяется с корнем документации, ближайший к документу выигрывает", () => {
    expect(resolveWikilink(targets, "docs/x.md", "DESIGN")?.docs).toBe("features/editor/design.md");
    expect(resolveWikilink(targets, "docs/x.md", "Features/B/Plan")?.docs).toBe("features/b/plan.md");
    expect(resolveWikilink(targets, "docs/features/b/x.md", "plan")?.docs).toBe("features/b/plan.md");
    expect(resolveWikilink(targets, "docs/x.md", "нет такого")).toBeNull();
  });

  it("поиск по смыслу идёт после названий и не повторяет документы первой группы", () => {
    const local = linkOptions(targets, "редактор");
    const found = [targets[0], targets[2], targets[2], targets[3]];
    const options = semanticOptions(targets, found, local);
    expect(options.map((o) => o.insert)).toEqual(["[[hybrid search]]", "[[features/a/plan]]"]);
    expect(options[0].detail).toBe("features/search/hybrid search.md · по смыслу");
  });

  it("Index.md в выбор не входит, заголовок берётся из индекса", () => {
    const tree = buildTree([
      { path: "docs/m/index.md", module: "m", title: "Модуль", chunks: 1 },
      { path: "docs/m/doc.md", module: "m", title: "Документ из индекса", chunks: 1 },
    ], "docs");
    expect(linkTargets(tree, "docs")).toEqual([{ path: "docs/m/doc.md", docs: "m/doc.md", title: "Документ из индекса" }]);
  });
});
