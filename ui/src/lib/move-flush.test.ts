import { expect, it, vi } from "vitest";
import { editors } from "./editors.svelte";
import { startFlushListener } from "./unsaved.svelte";

const listeners = vi.hoisted(() => new Map<string, (event: {payload:string}) => Promise<void>>());
const invoke = vi.hoisted(() => vi.fn());
vi.mock("./api", () => ({ inTauri: () => true }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({listen: vi.fn(async (name, handler) => { listeners.set(name,handler); return vi.fn(); })}));

it("перед переносом записывает только проект события; чужой конфликт не запрещает перенос", async () => {
  const flush = vi.spyOn(editors,"flush").mockResolvedValue([]);
  const stop = await startFlushListener();
  await listeners.get("bars://move-flush-request")!({payload:"C:/a"});
  expect(flush).toHaveBeenLastCalledWith("C:/a");
  expect(invoke).toHaveBeenLastCalledWith("flush_reply",{reply:"clean"});
  flush.mockResolvedValue([{}] as never);
  await listeners.get("bars://move-flush-request")!({payload:"C:/b"});
  expect(flush).toHaveBeenLastCalledWith("C:/b");
  expect(invoke).toHaveBeenLastCalledWith("flush_reply",{reply:"cancel"});
  stop(); flush.mockRestore();
});
