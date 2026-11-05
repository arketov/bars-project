import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, tick, unmount } from "svelte";
import { app } from "./app.svelte";
import { graphScope } from "./graph-scope.svelte";
import { buildTree } from "./tree";
import SearchPane from "./SearchPane.svelte";
import type { SearchOutput } from "./types";

vi.mock("./api", () => ({ call: vi.fn(async () => ({})), inTauri: () => true, joinPath: async (a: string, b: string) => `${a}/${b}`, openPath: vi.fn(), pickFolder: vi.fn(), copyText: vi.fn() }));

const documents = ["adr/a.md", "adr/b.md", "features/c.md", "features/d.md"].map((p) => ({ path: `docs/${p}`, module: p.split("/")[0], title: p, chunks: 2 }));
const output = { results: [], applied: { score_kind: "cosine", min_score: 0.3 } } as unknown as SearchOutput;
const displayed = (modules: string[]) => ({ requestId: "r1", project: "C:/a", query: "очередь", modules, files: [], limit: null });

let component: ReturnType<typeof mount> | null = null;
let target: HTMLDivElement;

beforeEach(() => {
  app.projects = [{ path: "C:/a", name: "A", docs_root: "docs", state: "ready", modules: [] }];
  app.current = "C:/a";
  app.docsRoot = "docs";
  app.tree = buildTree(documents, "docs");
  app.selected = new Set(["adr"]);
  app.query = "очередь";
  app.results = output;
  app.displayed = displayed(["adr"]);
  app.searchedQuery = "очередь";
  app.view = "graph";
  graphScope.state = null;
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  target.remove();
  graphScope.state = null;
  app.current = null;
  app.projects = [];
  app.results = null;
  app.displayed = null;
});

it("«Искать в подграфе» заменяет область документами подграфа и не запускает поиск; возврат восстанавливает всё", () => {
  const result = graphScope.apply(["docs/adr/b.md", "docs/features/c.md", "docs/features/c.md"]);
  expect(result).toEqual({ ok: true, documents: 2, skipped: 0 });
  expect([...app.selected].sort()).toEqual(["adr/b.md", "features/c.md"]);
  expect(graphScope.active?.documents).toBe(2);
  expect(app.view).toBe("main");
  // Запрос и параметры не менялись, прежняя выдача на месте, но она относится к прежней области.
  expect(app.query).toBe("очередь");
  expect(app.results).toEqual(output);
  graphScope.restore();
  expect([...app.selected]).toEqual(["adr"]);
  expect(app.results).toEqual(output);
  expect(app.displayed?.modules).toEqual(["adr"]);
  expect(graphScope.active).toBeNull();
});

it("вторая замена не перезаписывает исходную область; возврат ведёт к первой", () => {
  graphScope.apply(["docs/adr/a.md"]);
  graphScope.apply(["docs/features/d.md", "docs/features/c.md"]);
  expect(graphScope.active?.documents).toBe(2);
  graphScope.restore();
  expect([...app.selected]).toEqual(["adr"]);
});

it("удалённые и внешние документы исключаются с причиной; если пригодных нет, область не меняется", () => {
  const partly = graphScope.apply(["docs/adr/a.md", "docs/gone.md", "outside/zzz.md"]);
  expect(partly).toEqual({ ok: true, documents: 1, skipped: 2 });
  graphScope.restore();
  const none = graphScope.apply(["docs/gone.md"]);
  expect(none.ok).toBe(false);
  expect(none.ok === false && none.reason).toContain("нет документов");
  expect([...app.selected]).toEqual(["adr"]);
  expect(graphScope.active).toBeNull();
});

it("ручная правка галочек и смена проекта явно завершают временную область", () => {
  graphScope.apply(["docs/adr/a.md"]);
  app.toggleNode("features", true);
  expect(graphScope.active).toBeNull();
  graphScope.restore();
  expect(app.selected.has("features")).toBe(true);
  graphScope.apply(["docs/adr/a.md"]);
  app.current = "C:/other";
  expect(graphScope.active).toBeNull();
});

it("над поиском видны область из графа и «Вернуть прежнюю область»; выдача помечена как прежняя; фокус уходит в поле запроса", async () => {
  component = mount(SearchPane, { target });
  flushSync();
  expect(target.querySelector(".graph-scope")).toBeNull();
  graphScope.apply(["docs/adr/b.md", "docs/features/c.md"]);
  await tick();
  flushSync();
  const banner = target.querySelector(".graph-scope")!;
  expect(banner.textContent).toContain("2 документа");
  expect(target.textContent).toContain("Выдача ниже относится к прежней области");
  await new Promise((r) => setTimeout(r, 0));
  expect(document.activeElement).toBe(target.querySelector("textarea"));
  banner.querySelector<HTMLButtonElement>("button")!.click();
  await tick();
  flushSync();
  expect(target.querySelector(".graph-scope")).toBeNull();
  expect(target.textContent).not.toContain("относится к прежней области");
});
