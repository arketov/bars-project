import { expect, it } from "vitest";
import { mockCall } from "./mock";
import type { SettingsOutput } from "./types";

it("Save/Get accepts a proxy-backed settings draft and keeps devices independent", async () => {
  const before = await mockCall("settings_get", {}) as SettingsOutput;
  const draft = new Proxy(before.settings, {});
  draft.embed.accelerator = "cpu";
  draft.rerank.accelerator = "auto";
  await mockCall("settings_save", {settings:draft});
  const saved = await mockCall("settings_get", {}) as SettingsOutput;
  expect(saved.settings.embed.accelerator).toBe("cpu");
  expect(saved.settings.rerank.accelerator).toBe("auto");
});
