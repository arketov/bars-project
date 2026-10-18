// Связь окна с воркером: одна команда Tauri `bars_call` поверх тех же обработчиков, что у канала
// (docs/architecture/protocol.md, «Кто пользуется протоколом bars»). Вне Tauri (просмотр интерфейса
// в браузере) вызовы отвечает заглушка из `mock.ts`.

import { BarsError } from "./types";
import type { McpStatistic } from "./mcp-statistics";
import type { HistoryFilter, HistoryPage, HistoryDiagnostics, HistoryOptions } from "./mcp-history";

export async function mcpHistoryOptions(filter: HistoryFilter, projectQuery: string, sessionQuery: string): Promise<HistoryOptions> {
  if (!inTauri()) throw new Error("Подсказки доступны в приложении bars");
  return (await shell<HistoryOptions>("mcp_history_options", { filter, projectQuery, sessionQuery }))!;
}

export async function mcpHistory(filter: HistoryFilter): Promise<HistoryPage> {
  if (!inTauri()) throw new Error("История MCP доступна в приложении bars; браузер не читает установленный профиль");
  return (await shell<HistoryPage>("mcp_history", { filter }))!;
}
export async function mcpHistoryControl(enabled?: boolean, clearConfirmed = false): Promise<void> {
  if (!inTauri()) throw new Error("Управление историей доступно в приложении bars");
  await shell("mcp_history_control", { enabled: enabled ?? null, clearConfirmed });
}
export async function mcpHistoryLegacy(): Promise<McpStatistic[]> {
  if (!inTauri()) throw new Error("Прежние агрегаты доступны в приложении bars");
  return (await shell<McpStatistic[]>("mcp_history_legacy"))!;
}
export async function mcpHistoryDiagnostics(): Promise<HistoryDiagnostics> {
  if (!inTauri()) throw new Error("Диагностика доступна в приложении bars");
  return (await shell<HistoryDiagnostics>("mcp_history_diagnostics"))!;
}

export const inTauri = (): boolean => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

interface RpcErrorShape {
  code?: number;
  message?: string;
  data?: { kind?: string; percent?: number } | null;
}

export function toBarsError(raw: unknown): BarsError {
  if (raw instanceof BarsError) return raw;
  if (typeof raw === "string") return new BarsError(raw);
  if (raw && typeof raw === "object") {
    const rpc = raw as RpcErrorShape;
    return new BarsError(rpc.message ?? "неизвестная ошибка", rpc.data?.kind ?? null, rpc.code ?? -32603, rpc.data ?? null);
  }
  return new BarsError(String(raw));
}

/** Вызов метода воркера. Ошибка — `BarsError` с тем же `kind`, что видит агент. */
export async function call<T>(method: string, params: Record<string, unknown> = {}): Promise<T> {
  try {
    if (!inTauri()) {
      const { mockCall } = await import("./mock");
      return (await mockCall(method, params)) as T;
    }
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<T>("bars_call", { method, params });
  } catch (error) {
    throw toBarsError(error);
  }
}

async function shell<T>(command: string, args: Record<string, unknown> = {}): Promise<T | null> {
  if (!inTauri()) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}

/** Диалог выбора папки; `null` — отказ. */
export async function pickFolder(): Promise<string | null> {
  if (!inTauri()) return window.prompt("Путь к папке проекта") || null;
  return shell<string | null>("pick_folder");
}

/** Открывает файл или папку программой ОС. */
export async function openPath(path: string): Promise<void> {
  await shell("open_path", { path });
}

/** Перезапускает весь процесс bars (OQ-57). */
export async function restartApp(): Promise<void> {
  await shell("restart_app");
}

/** Домашняя папка bars (`~/.bars`): настройки, модели, логи. */
export async function barsDir(): Promise<string | null> {
  return shell<string>("bars_dir");
}

export type LogLevel = "debug" | "info" | "warn" | "error";
export interface LogEntry { time: string; pid: number; level: LogLevel; target: string; message: string }
export interface LogSnapshot { directory: string; entries: LogEntry[]; skipped: number; total: number; write_error: boolean }

export async function workerLogs(level: LogLevel): Promise<LogSnapshot> {
  if (!inTauri()) {
    // Явная фикстура: просмотр в браузере не читает журнал установленного bars.
    const entries: LogEntry[] = [
      { time: "2026-10-18T12:20:03Z", pid: 1234, level: "error", target: "bars_worker", message: "Индексация завершилась ошибкой; подробности — в отчёте задания." },
      { time: "2026-10-18T12:20:02Z", pid: 1234, level: "warn", target: "bars_core::models", message: "rerank: подозрительные логиты; max ниже −8." },
      { time: "2026-10-18T12:20:01Z", pid: 1234, level: "info", target: "bars_worker", message: "Воркер готов принимать вызовы." },
    ];
    const order: LogLevel[] = ["debug", "info", "warn", "error"];
    return { directory: "~/.bars/logs (макет)", entries: entries.filter(entry => order.indexOf(entry.level) >= order.indexOf(level)), skipped: 0, total: entries.length, write_error: false };
  }
  return (await shell<LogSnapshot>("worker_logs", { level }))!;
}

export async function copyText(text: string): Promise<void> {
  await navigator.clipboard.writeText(text);
}

export interface DocumentEditor { id: string; label: string }

export interface FolderTarget { id: string; absolute: string; missing: boolean }
export async function folderTarget(project: string, docsRoot: string, path: string): Promise<FolderTarget> {
  if (!inTauri()) return { id: "mock", absolute: joinPath(project, docsRoot === "." ? path : docsRoot + (path === "." ? "" : "/" + path)), missing: false };
  return (await shell<FolderTarget>("folder_target", { project, docsRoot, path }))!;
}
export async function folderAction(id: string, action: "open" | "path" | "check"): Promise<{ absolute: string; missing: boolean; operation_id: string | null }> {
  if (!inTauri()) throw new Error("Действие папки доступно в приложении bars; браузер показывает макет");
  return (await shell<{ absolute: string; missing: boolean; operation_id: string | null }>("folder_action", { id, action }))!;
}

export async function documentEditors(): Promise<DocumentEditor[]> {
  if (!inTauri()) return [{ id: "code", label: "VS Code (макет)" }, { id: "notepad", label: "Блокнот (макет)" }];
  return (await shell<DocumentEditor[]>("document_editors")) ?? [];
}

export async function documentAction(project: string, path: string, action: "path" | "open" | "reveal" | "check", editor?: string): Promise<string> {
  if (!inTauri()) {
    if (action !== "path") throw new Error("Действие ОС/очереди доступно в приложении bars; браузер показывает макет");
    return joinPath(project, path);
  }
  return (await shell<string>("document_action", { project, path, action, editor: editor ?? null }))!;
}

/** Абсолютный путь файла проекта для «В редакторе». */
export function joinPath(root: string, relative: string): string {
  const sep = root.includes("\\") ? "\\" : "/";
  return root.replace(/[\\/]+$/, "") + sep + relative.replace(/\//g, sep);
}
