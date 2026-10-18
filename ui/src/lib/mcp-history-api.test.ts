import { afterEach, expect, it, vi } from "vitest";
import { mcpHistory, mcpHistoryControl, mcpHistoryOptions, mcpHistoryLegacy, mcpHistoryDiagnostics } from "./api";
import type { HistoryFilter } from "./mcp-history";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
import { invoke } from "@tauri-apps/api/core";
afterEach(() => { Reflect.deleteProperty(window, "__TAURI_INTERNALS__"); vi.clearAllMocks(); });
it("передаёт точные имена Tauri-команд, camelCase управления и непрозрачный курсор", async () => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
  const filter: HistoryFilter = { project: "", session: "s", tool: "bars_read", outcome: "success", from: 1, to: 2, limit: 50, cursor: { started: 1, id: 8, ceiling: 20, generation: 1, from: 1, to: 2, filter_key: "opaque" } };
  await mcpHistory(filter); expect(invoke).toHaveBeenLastCalledWith("mcp_history", { filter });
  await mcpHistoryControl(false); expect(invoke).toHaveBeenLastCalledWith("mcp_history_control", { enabled: false, clearConfirmed: false });
  await mcpHistoryControl(undefined, true); expect(invoke).toHaveBeenLastCalledWith("mcp_history_control", { enabled: null, clearConfirmed: true });
  await mcpHistoryOptions(filter, "p", "part"); expect(invoke).toHaveBeenLastCalledWith("mcp_history_options", { filter, projectQuery: "p", sessionQuery: "part" });
  await mcpHistoryLegacy(); expect(invoke).toHaveBeenLastCalledWith("mcp_history_legacy", {});
  await mcpHistoryDiagnostics(); expect(invoke).toHaveBeenLastCalledWith("mcp_history_diagnostics", {});
});
it("браузер не выдаёт отсутствие соединения за пустую настоящую историю", async () => {
  await expect(mcpHistory({})).rejects.toThrow("доступна в приложении");
  await expect(mcpHistoryControl(false)).rejects.toThrow("в приложении");
  await expect(mcpHistoryLegacy()).rejects.toThrow("в приложении");
  expect(invoke).not.toHaveBeenCalled();
});
