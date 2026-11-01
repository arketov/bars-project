import type { TextOutput } from "../../lib/text-search.svelte";

// Только состояния представления. Настоящий поиск и синтаксис regex не имитируются.
export function queryCardText(): TextOutput {
  const state = new URLSearchParams(window.location.search).get("text");
  if (state === "error") throw new Error("Ошибка выражения: незакрытая скобка, позиция 1. Исправьте запрос.");
  const output: TextOutput = { documents: [], docs_total: 0, hits_total: 0, truncated: null, skipped: [], elapsed_ms: 4 };
  if (state === "skipped") output.skipped = [{ path: "docs/locked.md", reason: "Нет доступа к файлу" }];
  if (state === "limited") output.truncated = "limit_hits";
  if (state === "hits" || state === "zero") {
    const zero = state === "zero";
    output.docs_total = 1;
    output.hits_total = 1;
    output.documents = [{
      path: "docs/features/search/hybrid-search.md", module: "features", title: "Гибридный поиск",
      revision: "query-card-fixture", hits_total: 1, has_more: false,
      hits: [{ line: 13, column: zero ? 1 : 24, length: zero ? 0 : 5,
        text: "## Оценка результата и порог", ranges: zero ? [[0, 0]] : [[23, 28]] }],
    }];
  }
  return output;
}
