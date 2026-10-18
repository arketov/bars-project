import { afterEach, expect, it } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import MainNavigation from "./MainNavigation.svelte";
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

it("переключает граф и поиск, сохраняя запрос и область документов", () => {
  app.query = "Условия обновления";
  app.selected = new Set(["architecture"]);
  component = mount(MainNavigation, { target: document.body });
  flushSync();
  const buttons = [...document.querySelectorAll("button")];
  buttons.find(button => button.textContent === "Граф")!.click();
  flushSync();
  expect(app.view).toBe("graph");
  expect(buttons[1].getAttribute("aria-current")).toBe("page");
  buttons.find(button => button.textContent === "Поиск")!.click();
  flushSync();
  expect(app.view).toBe("main");
  expect(app.query).toBe("Условия обновления");
  expect([...app.selected]).toEqual(["architecture"]);
});

it("показывает пункт редактора, сохраняя текущий экран до его реализации", () => {
  component = mount(MainNavigation, { target: document.body });
  flushSync();
  const editor = [...document.querySelectorAll("button")].find(button => button.textContent === "Редактор")!;
  expect(editor).toBeDefined();
  editor.click();
  flushSync();
  expect(app.view).toBe("main");
  expect(app.toast?.text).toContain("Редактор запланирован");
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
