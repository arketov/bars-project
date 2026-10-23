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

/** Ожидаемый отказ команд документа: `kind` — `invalid_path`, `outside_root`, `read_only`, `locked` и др. */
export interface DocumentFailure { status: "error"; kind: string; message: string }
export type DocumentReadOnly = "not_utf8" | "too_large" | "file";
export interface DocumentRead {
  status: "ok";
  text: string;
  /** `null` — файла ещё нет (например, `index.md` папки): запись создаст его. */
  revision: string | null;
  eol: "lf" | "crlf";
  bom: boolean;
  size: number;
  read_only: DocumentReadOnly | null;
}
export type DocumentWrite =
  | { status: "saved"; revision: string }
  | { status: "conflict"; revision: string | null }
  | DocumentFailure;

/** Чтение документа для редактора; вне Tauri — `null`: окно в браузере правит текст из мока. */
export async function documentRead(project: string, path: string): Promise<DocumentRead | DocumentFailure | null> {
  return shell<DocumentRead | DocumentFailure>("document_read", { project, path });
}

/** Запись документа, если на диске ещё `baseRevision` (`null` — файла быть не должно). */
export async function documentWrite(project: string, path: string, text: string, baseRevision: string | null, eol: "lf" | "crlf", bom: boolean): Promise<DocumentWrite> {
  if (!inTauri()) return { status: "saved", revision: `browser-${text.length}-${Date.now()}` };
  return (await shell<DocumentWrite>("document_write", { project, path, text, baseRevision, eol, bom }))!;
}

/** Ревизия документа на диске без текста; `revision: null` — файла нет. */
export async function documentRevision(project: string, path: string): Promise<{ status: "ok"; revision: string | null } | DocumentFailure | null> {
  return shell<{ status: "ok"; revision: string | null } | DocumentFailure>("document_revision", { project, path });
}

/** Цвета и иконки проекта (Т-38): файл `.bars/appearance.toml`. */
export interface AppearanceEntry { color: string | null; icon: string | null }
export type AppearanceReply =
  | { status: "ok"; folders: Record<string, AppearanceEntry>; documents: Record<string, AppearanceEntry> }
  | DocumentFailure;

export async function appearanceRead(project: string): Promise<AppearanceReply | null> {
  return shell<AppearanceReply>("appearance_read", { project });
}

/** `null` у цвета или иконки сбрасывает поле. */
export async function appearanceSet(project: string, kind: "folder" | "document", path: string, color: string | null, icon: string | null): Promise<AppearanceReply | null> {
  return shell<AppearanceReply>("appearance_set", { project, kind, path, color, icon });
}

/** Открывает документ в отдельном окне bars (Т-40); уже открытое окно выводится вперёд. */
export async function documentWindowOpen(project: string, path: string): Promise<void> {
  if (!inTauri()) throw new Error("Отдельные окна доступны в приложении bars");
  await shell("document_window_open", { project, path });
}

/** Документ этого окна, если оно отдельное окно документа; в главном окне и в браузере — `null`. */
export async function documentWindowTarget(): Promise<{ project: string; path: string } | null> {
  return shell<{ project: string; path: string } | null>("document_window_target");
}

/** Закрывает окно документа после записи правок. */
export async function documentWindowClose(): Promise<void> {
  await shell("document_window_close");
}

/** Просит основное окно открыть документ (ссылка внутри отдельного окна). */
export async function openInMainWindow(project: string, path: string, firstLine: number, lastLine: number): Promise<void> {
  await shell("open_in_main_window", { project, path, firstLine, lastLine });
}

/** Дерево документации по диску (Т-33): файлы и папки, пути от корня проекта. */
export interface DiskTreeOutput {
  status: "ok";
  docs_root: string;
  revision: string;
  dirs: string[];
  files: { path: string; size: number; modified: number }[];
  problems: { path: string; reason: string }[];
}

/** Ответ операции с файлом или папкой: `partial` — папка создана, а её `index.md` нет. */
export type EntryResult =
  | { status: "ok"; path: string; index_path?: string }
  | { status: "partial"; path: string; index_path: string; kind: string; message: string }
  | DocumentFailure;

/** Дерево с диска; вне приложения и при ошибке — `null`: окно строит дерево по индексу. */
export async function treeRead(project: string): Promise<DiskTreeOutput | null> {
  if (!inTauri()) return (await import("./mock-disk")).mockTreeRead(project);
  const out = await shell<DiskTreeOutput | DocumentFailure>("tree_read", { project });
  return out && out.status === "ok" ? out : null;
}

export async function entryCreateDoc(project: string, parent: string, name: string): Promise<EntryResult> {
  if (!inTauri()) return (await import("./mock-disk")).mockEntry("create_doc", project, { parent, name });
  return (await shell<EntryResult>("entry_create_doc", { project, parent, name }))!;
}

export async function entryCreateDir(project: string, parent: string, name: string): Promise<EntryResult> {
  if (!inTauri()) return (await import("./mock-disk")).mockEntry("create_dir", project, { parent, name });
  return (await shell<EntryResult>("entry_create_dir", { project, parent, name }))!;
}

/** Переименование и перемещение: `toParent` — папка назначения, `name` — имя на новом месте. */
export async function entryMove(project: string, from: string, toParent: string, name: string): Promise<EntryResult> {
  if (!inTauri()) return (await import("./mock-disk")).mockEntry("move", project, { from, toParent, name });
  return (await shell<EntryResult>("entry_move", { project, from, toParent, name }))!;
}

/** Отправляет файл или папку в корзину ОС; без корзины — отказ. */
export async function entryTrash(project: string, path: string): Promise<EntryResult> {
  if (!inTauri()) return (await import("./mock-disk")).mockEntry("trash", project, { path });
  return (await shell<EntryResult>("entry_trash", { project, path }))!;
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
