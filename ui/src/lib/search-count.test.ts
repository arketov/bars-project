import { expect, it } from "vitest";
import { app } from "./app.svelte";
import type { SettingsDoc } from "./types";

const doc = (rerank: boolean, max = 50) => ({
  rerank: { enabled: rerank },
  search: { results: 3, results_reranked: 7, max_results: max },
}) as unknown as SettingsDoc;

it("умолчание «Результатов» и предел берутся из настроек (ISS-37)", () => {
  app.count = null; app.results = null;
  app.applySearchSettings(doc(true, 50));
  expect(app.shownCount).toBe(7);
  expect(app.maxCount).toBe(50);
  app.applySearchSettings(doc(false, 10));
  expect(app.shownCount).toBe(3);
  expect(app.maxCount).toBe(10);
  app.count = 40;
  expect(app.shownCount).toBe(10);
  app.count = null;
});
