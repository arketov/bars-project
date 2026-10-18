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
  expect(target.querySelector('[aria-pressed="true"]')?.textContent).toBe("GPU if available");
  expect(target.textContent).toContain("CPU");
  expect(target.textContent).toContain("GPU session unavailable");
});

it("external and CPU-only models do not offer GPU controls", () => {
  for (const config of [{source:"remote" as const}, {source:"builtin" as const, cpuOnly:true}]) {
    const target = document.createElement("div");
    components.push(mount(DevicePicker, {target, props:{role:"Embed", value:"auto", ...config}}));
    flushSync();
    expect(target.querySelectorAll("button").length).toBe(0);
    expect(target.textContent).toContain(config.source === "remote" ? "server device unknown" : "CPU only");
  }
});
