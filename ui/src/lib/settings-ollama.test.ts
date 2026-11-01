import { afterEach, expect, it } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import SettingsPage from "./SettingsPage.svelte";
import { app } from "./app.svelte";
import { settings } from "./settings.svelte";
import { clone } from "./settings-logic";
import { mockCall } from "./mock";
import type { SettingsOutput } from "./types";

let component: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  settings.saved = settings.draft = null;
  document.body.innerHTML = "";
});

it.each(["ollama", "lmstudio"] as const)("selects %s for rerank without reindexing embed", async (source) => {
  const output = await mockCall("settings_get", {}) as SettingsOutput;
  settings.saved = clone(output.settings);
  settings.draft = clone(output.settings);
  settings.draft.rerank.enabled = true;
  app.settingsTab = "models";
  component = mount(SettingsPage, { target: document.body, props: { embedded: true } });
  flushSync();
  const label = source === "ollama" ? "Ollama" : "LM Studio";
  [...document.querySelectorAll("button")].filter(b => b.textContent.trim() === label)[1].click();
  flushSync();
  expect(settings.draft.rerank.source).toBe(source);
  expect(settings.draft.embed).toEqual(output.settings.embed);
  expect(settings.willReindex).toBe(false);
  expect(settings.errors["rerank.model"]).toBeTruthy();
  settings.draft.rerank.model = "Qwen3-Reranker-0.6B";
  flushSync();
  expect(settings.errors).toEqual({});
  expect(document.querySelector('input[aria-label="Параллельных запросов rerank"]')).not.toBeNull();
});

it.each([
  ["ollama", "Ollama", "http://127.0.0.1:11434", "qwen3-embedding:0.6b"],
  ["lmstudio", "LM Studio", "http://127.0.0.1:1234", ""],
] as const)("selects %s independently for embed, clears inherited keys and restores saved source", async (source, label, url, model) => {
  const output = await mockCall("settings_get", {}) as SettingsOutput;
  const saved = clone(output.settings);
  saved.embed.source = "remote";
  saved.embed.model = "remote-embedding";
  saved.embed.base_url = "https://example.com";
  saved.embed.api_key_name = "test-key-name";
  saved.embed.api_key_env = "TEST_KEY_ENV";
  settings.saved = clone(saved);
  settings.draft = clone(saved);
  app.settingsTab = "models";
  component = mount(SettingsPage, { target: document.body, props: { embedded: true } });
  flushSync();
  const buttons = () => [...document.querySelectorAll("button")];
  const ollama = buttons().filter(b => b.textContent.trim() === label);
  expect(ollama).toHaveLength(2);
  ollama[0].click();
  flushSync();
  expect(settings.draft?.embed.source).toBe(source);
  expect(settings.draft?.embed.base_url).toBe(url);
  expect(settings.draft?.embed.model).toBe(model);
  expect(settings.draft?.embed.api_key_name).toBeNull();
  expect(settings.draft?.embed.api_key_env).toBeNull();
  expect(settings.draft?.rerank).toEqual(saved.rerank);
  if (source === "lmstudio") {
    expect(settings.errors["embed.model"]).toBe("Укажите модель провайдера");
  } else expect(settings.errors).toEqual({});
  expect(settings.willReindex).toBe(true);
  expect(document.body.textContent).toContain(source === "ollama" ? "Корень сервера Ollama" : "Запустите сервер LM Studio");
  buttons().find(b => b.textContent.trim() === "Удалённый провайдер")!.click();
  flushSync();
  expect(settings.draft?.embed).toEqual(saved.embed);
  expect(settings.saved).toEqual(saved);
});
