import { afterEach, expect, it } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import DevicePicker from "./DevicePicker.svelte";
import type { ModelRuntime } from "./types";

const components: ReturnType<typeof mount>[] = [];
afterEach(() => components.splice(0).forEach(component => void unmount(component)));
const runtime: ModelRuntime = { model: "bge", source: "builtin", requested: "auto", state: "ready", backend: "onnx", actual: "cpu", fallback_reason: "GPU session unavailable" };

it("shows fallback independently of the requested preference", () => {
  const target = document.createElement("div");
  components.push(mount(DevicePicker, {target, props:{role:"Embed", value:"auto", source:"builtin", runtime}}));
  flushSync();
  expect(target.querySelector('[aria-pressed="true"]')?.textContent).toBe("GPU, если доступен");
  expect(target.textContent).toContain("Текущая конфигурация: bge · готова");
  expect(target.textContent).toContain("GPU недоступен; используется CPU.");
  expect(target.textContent).toContain("CPU");
  expect(target.textContent).toContain("GPU session unavailable");
});

it("shows precision reported by the actual session", () => {
  const target = document.createElement("div");
  components.push(mount(DevicePicker, {target, props:{role:"Rerank", value:"auto", source:"builtin",
    runtime:{...runtime, actual:"directml", precision:"fp16", revision:"pinned", fallback_reason:null}}}));
  flushSync();
  expect(target.textContent).toContain("DirectML");
  expect(target.textContent).toContain("FP16");
  expect(target.querySelector(".fallback")).toBeNull();
});

it("external and CPU-only models do not offer GPU controls", () => {
  for (const config of [{source:"remote" as const}, {source:"builtin" as const, cpuOnly:true}]) {
    const target = document.createElement("div");
    components.push(mount(DevicePicker, {target, props:{role:"Embed", value:"auto", ...config}}));
    flushSync();
    expect(target.querySelectorAll("button").length).toBe(0);
    expect(target.textContent).toContain(config.source === "remote" ? "устройство сервера неизвестно" : "Только CPU");
  }
});

it.each([
  ["unloaded", "не загружена"], ["loading", "загружается"],
  ["ready", "готова"], ["failed", "ошибка загрузки"],
] as const)("shows %s as a Russian label", (state, label) => {
  const target = document.createElement("div");
  components.push(mount(DevicePicker, {target, props:{role:"Embed", value:"auto", source:"builtin", runtime:{...runtime, state}}}));
  flushSync();
  expect(target.querySelector("section")?.getAttribute("aria-label")).toBe("Устройство Embed");
  expect(target.querySelector(".runtime")?.textContent).toContain(`Текущая конфигурация: bge · ${label}`);
});
