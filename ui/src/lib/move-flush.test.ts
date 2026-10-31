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

it("выход при идущей записи: окно сразу отвечает «writing», а «clean» — только после записи (ISS-93)", async () => {
  invoke.mockClear();
  let finish!: () => void;
  const writing = new Promise<never[]>((resolve) => { finish = () => resolve([]); });
  const unsaved = vi.spyOn(editors, "unsaved", "get").mockReturnValue([{}] as never);
  const flush = vi.spyOn(editors, "flush").mockReturnValue(writing);
  const stop = await startFlushListener();
  const handled = listeners.get("bars://flush-request")!({ payload: "" });
  await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("flush_reply", { reply: "writing" }));
  expect(invoke).not.toHaveBeenCalledWith("flush_reply", { reply: "clean" });
  finish();
  await handled;
  expect(invoke.mock.calls.map((call) => call[1].reply)).toEqual(["writing", "clean"]);
  unsaved.mockRestore(); flush.mockRestore(); stop();
});

it("выход без изменённых документов отвечает «clean» без «writing»", async () => {
  invoke.mockClear();
  const unsaved = vi.spyOn(editors, "unsaved", "get").mockReturnValue([]);
  const flush = vi.spyOn(editors, "flush").mockResolvedValue([]);
  const stop = await startFlushListener();
  await listeners.get("bars://flush-request")!({ payload: "" });
  expect(invoke.mock.calls.map((call) => call[1].reply)).toEqual(["clean"]);
  unsaved.mockRestore(); flush.mockRestore(); stop();
});
