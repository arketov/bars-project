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
