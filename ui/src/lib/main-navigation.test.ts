import { afterEach, expect, it } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import SearchPane from "./SearchPane.svelte";
import { app } from "./app.svelte";

let component: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  document.body.replaceChildren();
  app.view = "main";
  app.query = "";
  app.retrievalMode = "hybrid";
  app.includeRelated = true;
  app.selected = new Set();
});

it("меняет режим поиска и настройку связанных источников без отправки запроса", () => {
  app.query = "Условия обновления";
  app.current = null;
  component = mount(SearchPane, { target: document.body });
  flushSync();
  const mode = document.querySelector<HTMLSelectElement>('select[aria-label="Режим поиска"]')!;
  mode.value = "mix";
  mode.dispatchEvent(new Event("change", { bubbles: true }));
  flushSync();
  expect(app.retrievalMode).toBe("mix");
  const related = document.querySelector<HTMLInputElement>('.related-option input')!;
  expect(related.checked).toBe(true);
  related.click();
  flushSync();
  expect(app.includeRelated).toBe(false);
  expect(app.query).toBe("Условия обновления");
});

it("называет режимы поиска по-русски и сообщает об откате, только если режим изменился (ISS-45)", () => {
  component = mount(SearchPane, { target: document.body });
  flushSync();
  const options = [...document.querySelectorAll<HTMLOptionElement>('select[aria-label="Режим поиска"] option')];
  expect(options.map((option) => [option.value, option.textContent])).toEqual([["hybrid", "Обычный"], ["mix", "Со связями"], ["graph", "Отношения и пути"]]);

  app.results = { results: [], applied: { score_kind: "rerank", min_score: 0.3 } } as never;
  app.searchedQuery = "запрос";
  app.graphSearch = { requested_mode: "mix", applied_mode: "mix", fallback_reason: null, related: null, graph_results: null } as never;
  flushSync();
  expect(document.querySelector(".graph-search-info")).toBeNull();

  app.graphSearch = { requested_mode: "mix", applied_mode: "hybrid", fallback_reason: "Связи отключены: include_related=false", related: null, graph_results: null } as never;
  flushSync();
  const info = document.querySelector(".graph-search-info")!;
  expect(info.textContent).toContain("Запрошен режим «Со связями», применён «Обычный»");
  expect(info.textContent).toContain("Связанные источники выключены");
  expect(info.textContent).not.toContain("include_related");
  expect(info.textContent).not.toMatch(/\b(mix|hybrid)\b/);
  app.results = null;
  app.searchedQuery = "";
  app.graphSearch = null;
});
