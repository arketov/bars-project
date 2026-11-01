import { expect, it } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import SearchPane from "./SearchPane.svelte";
import { app } from "./app.svelte";
import type { GraphSearchOutput } from "./graph";

it("выдача показывает слова пользователя, сохраняет коды в подсказках (ISS-59)", async () => {
  app.projects = [{ path: "C:/проект", name: "Проект", docs_root: "docs", state: "ready", modules: [] }];
  app.current = "C:/проект";
  app.tree = []; app.selected = new Set(); app.activity = null;
  app.searchError = null; app.searching = false; app.searchUnsaved = [];
  app.query = "Как устроен поиск?"; app.displayed = null;
  app.textSearch.reset();
  const graph: GraphSearchOutput = {
    requested_mode: "mix", applied_mode: "hybrid", fallback_reason: "Связи отключены: include_related=false",
    related: null, graph_results: null,
    results: [{ path: "docs/a.md", module: ".", title: "Поиск", heading_chain: "Поиск", first_line: 1, last_line: 3, text: "Ответ в документации", score: 0.7, score_kind: "cosine" }],
    applied: { results: 5, candidates: 30, rerank_candidates: 15, min_score: 0, score_kind: "cosine", clamped: false },
    indexing: false, pending: 0,
  };
  app.results = graph; app.graphSearch = graph;
  const target = document.createElement("div"); document.body.append(target);
  const component = mount(SearchPane, { target }); flushSync();
  try {
    expect(target.textContent).toContain("Связанные источники выключены");
    expect(target.textContent).toContain("Оценка по близости");
    expect(target.textContent).not.toMatch(/include_related|\bmix\b|\bhybrid\b|\bcosine\b|\brerank\b/);
    expect(target.querySelector('[title="Связи отключены: include_related=false"]')).not.toBeNull();
    app.graphSearch = { ...graph, fallback_reason: "future_fallback=1" }; flushSync();
    expect(target.textContent).toContain("Причина изменения режима не определена");
    expect(target.textContent).not.toContain("future_fallback");
    expect(target.querySelector('[title="future_fallback=1"]')).not.toBeNull();
  } finally {
    await unmount(component); target.remove();
    app.results = null; app.graphSearch = null; app.current = null; app.projects = [];
  }
});
