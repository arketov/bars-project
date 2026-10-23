import { afterEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import SettingsPage from "./SettingsPage.svelte";
import { app } from "./app.svelte";
import { settings } from "./settings.svelte";
import { clone } from "./settings-logic";
import { mockCall } from "./mock";
import { call } from "./api";
import type { ModelsInfoOutput, SettingsOutput } from "./types";
vi.mock("./api", async original => ({ ...await original<typeof import("./api")>(), call: vi.fn() }));
let component: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  settings.saved = settings.draft = null;
  app.models = null;
  document.body.innerHTML = "";
  vi.mocked(call).mockReset();
});
async function render(actual: string) {
  const output = await mockCall("settings_get", {}) as SettingsOutput;
  output.settings.embed.source = "builtin";
  output.settings.embed.model = "BAAI/bge-m3";
  output.settings.embed.accelerator = "auto";
  settings.saved = clone(output.settings);
  settings.draft = clone(output.settings);
  app.models = await mockCall("models_info", {}) as ModelsInfoOutput;
  app.models.embed.push({id:"BAAI/bge-m3:fp16",dim:1024,backend:"onnx",warnings:[],downloaded:false,downloading:false});
  const runtime = {model:"BAAI/bge-m3",source:"builtin",requested:"auto" as const,state:"ready" as const,backend:"onnx",actual,fallback_reason:null,precision:"fp32"};
  app.models.runtime = {embed:runtime,rerank:{...runtime}};
  app.settingsTab = "models";
  component = mount(SettingsPage,{target:document.body,props:{embedded:true}});
  flushSync();
}
it("выбор FP16 меняет только черновик, требует согласия и отменяется без записи", async () => {
  await render("directml");
  const button = [...document.querySelectorAll("button")].find(b => b.textContent.trim() === "Выбрать FP16")!;
  expect(button).toBeTruthy();
  button.click(); flushSync();
  expect(settings.draft!.embed.model).toBe("BAAI/bge-m3:fp16");
  expect(settings.saved!.embed.model).toBe("BAAI/bge-m3");
  expect(settings.willReindex).toBe(true);
  expect(call).not.toHaveBeenCalled();
  [...document.querySelectorAll("button")].find(b => b.textContent.trim() === "Отменить")!.click(); flushSync();
  expect(settings.draft!.embed.model).toBe("BAAI/bge-m3");
  expect(settings.willReindex).toBe(false);
  expect(call).not.toHaveBeenCalled();
});
it("CPU fallback не получает рекомендацию GPU-профиля", async () => {
  await render("cpu");
  expect([...document.querySelectorAll("button")].some(b => b.textContent.trim() === "Выбрать FP16")).toBe(false);
});
