import { afterEach, expect, it } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import SettingsPage from "./SettingsPage.svelte";
import { app } from "./app.svelte";
import { settings } from "./settings.svelte";
import { clone, parseFingerprint, validate } from "./settings-logic";
import { mockCall } from "./mock";
import type { SettingsOutput } from "./types";

let component: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  settings.saved = settings.draft = null;
  document.body.innerHTML = "";
});

it("selects TEI independently for both models with correct ports and reindex identity", async () => {
  const output = await mockCall("settings_get", {}) as SettingsOutput;
  settings.saved = clone(output.settings);
  settings.draft = clone(output.settings);
  settings.draft.rerank.enabled = true;
  app.settingsTab = "models";
  component = mount(SettingsPage, { target: document.body, props: { embedded: true } });
  flushSync();
  const tei = [...document.querySelectorAll("button")].filter(b => b.textContent.trim() === "TEI");
  expect(tei).toHaveLength(2);
  tei[0].click();
  flushSync();
  expect(settings.draft.embed.source).toBe("tei");
  expect(settings.draft.embed.base_url).toBe("http://127.0.0.1:8080");
  expect(settings.draft.rerank.source).toBe(output.settings.rerank.source);
  tei[1].click();
  flushSync();
  expect(settings.draft.rerank.source).toBe("tei");
  expect(settings.draft.rerank.base_url).toBe("http://127.0.0.1:8081");
  expect(validate(settings.draft)).toEqual({});
  expect(settings.willReindex).toBe(true);
  expect(parseFingerprint("tei:BAAI/bge-m3")).toEqual({ source: "tei", model: "BAAI/bge-m3" });
  expect(document.body.textContent).toContain("Корень сервера TEI");
  expect(document.body.textContent).toContain("bars не переключает её на сервере");
});
