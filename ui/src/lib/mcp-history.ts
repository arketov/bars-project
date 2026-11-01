import { label as uiLabel } from "./labels";
export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export type Fields = Record<string, Json>;
export interface Attempt {
  version: number; id: number; generation: number; epoch: string; session: string;
  tool: string; project: string; project_name: string | null; started: number;
  finished: number | null; duration_ms: number | null; outcome: string;
  requested: Fields | null; applied: Fields | null; result: Fields | null;
  error: Fields | null; incomplete: string[]; delivery: string;
}
export interface Cursor {
  started: number; id: number; ceiling: number; generation: number;
  from: number; to: number; filter_key: string;
}
export interface HistoryFilter {
  from?: number; to?: number; project?: string; session?: string;
  tool?: string; outcome?: string; limit?: number; cursor?: Cursor; id?: number;
}
export interface HistoryDiagnostics { lost_events: number; last_storage_error: string | null }
export interface HistoryOptions { projects: string[]; sessions: string[]; projects_more: boolean; sessions_more: boolean }
export interface HistoryPage extends HistoryDiagnostics {
  records: Attempt[]; next: Cursor | null; enabled: boolean; coverage: string;
  summary: { calls: number; success: number; errors: number; unknown: number;
    running: number; total_ms: number; timed_calls: number; empty_success: number };
}
export const outcomes: Record<string, string> = { running: "Выполняется", success: "Успешно", error: "Ошибка", unknown: "Исход неизвестен" };
export function status(a: Attempt): string {
  if (a.outcome !== "success") return outcomes[a.outcome] ?? "Исход неизвестен";
  if (a.result?.queued) return "В очереди";
  if (a.result?.warning) return "Успех с предупреждением";
  return a.result?.count === 0 ? "Пустой ответ" : "Успешно";
}
export function quantity(a: Attempt): string {
  if (a.result?.unit === "not_applicable") return "Количество не применяется";
  const units: Record<string, string> = { fragments: "фрагментов", lines: "строк", headings: "заголовков", projects: "проектов", graph_items: "элементов графа" };
  const count = a.result?.count;
  return typeof count === "number" ? `${count} ${units[String(a.result?.unit)] ?? "элементов"}` : "Количество неизвестно";
}
export const duration = (ms: number | null) => ms === null ? "Не записано" : `${ms.toLocaleString("ru-RU")} мс`;
export const localTime = (seconds: number | null) => seconds === null ? "Не записано" : new Date(seconds * 1000).toLocaleString("ru-RU");
export function incompleteness(reason: string): string {
  return ({ original_mcp_input_unavailable: "исходный MCP-вход недоступен", graph_detail_contract_not_recorded: "подробности GraphRAG не сохраняются", worker_restarted: "worker перезапущен", collection_disabled: "сбор отключён" } as Record<string, string>)[reason] ?? reason;
}
export function value(v: Json | undefined): string {
  if (v === undefined) return "Не записано";
  if (v === null) return "null (явно)";
  if (v === "") return "Пустая строка";
  if (v === true) return "Да";
  if (v === false) return "Нет";
  if (v === "not_recorded") return "Не сохранён";
  if (v === "whole_document") return "Документ целиком";
  const meanings: Record<string, string> = { cosine: uiLabel("score", "cosine").text, rerank: uiLabel("score", "rerank").text, model_loading: "Модель загружается", model_unavailable: "Модель недоступна", rerank_execution_failed: "Ошибка оценки моделью", ready: "Готов", indexing: "Индексация", missing: "Проект недоступен", rebuild: "Полная индексация", check: "Проверка изменений" };
  if (typeof v === "string" && meanings[v]) return meanings[v];
  if (Array.isArray(v)) return v.length ? v.map(value).join("; ") : "Пустой список";
  if (typeof v === "object") return Object.entries(v).map(([k, x]) => `${labels[k] ?? k}: ${value(x)}`).join("; ");
  return String(v);
}
const labels: Record<string, string> = {
  query: "Запрос", project: "Проект", format: "Формат", modules: "Модули", files: "Файлы",
  limit: "Фрагментов (limit)", candidates: "Кандидатов", min_score: "Порог", path: "Путь",
  levels: "Уровни", under: "Раздел", under_line: "Строка раздела", full: "Полная проверка",
  range: "Диапазон", cwd: "Рабочая папка", resolved_project: "Разрешённый проект",
  resolved_project_name: "Имя проекта", rerank_performed: "Оценка моделью выполнена",
  rerank_fallback: "Запасной способ оценки", rerank_fallback_reason: "Причина смены способа оценки",
  models: "Модели", embed: "Embed", rerank: "Оценка моделью", rerank_requested: "Оценка моделью запрошена",
  score_kind: "Шкала", indexing: "Индексация", pending: "Ожидают документы",
  watching: "Слежение за файлами", model: "Модель", accepted: "Принято в очередь", rerank_candidates: "Кандидатов для оценки моделью", clamped: "Значения ограничены", details: "Подробности",
  state: "Состояние", updated_at: "Обновлено", index_model: "Модель индекса",
  settings_model: "Модель настроек", has_problem: "Есть проблема", operation_id: "ID задания",
  merged: "Объединено с заданием", chunk_id: "ID чанка", source_chunk: "Исходный чанк", job_kind: "Тип задания", anchor: "Найденный раздел", mode: "Режим",
  proxy_attempt: "Попытка прокси", operation: "Операция графа", topic: "Тема", roots: "Корни",
  analysis: "Анализ", budgets: "Пределы графа", include_related: "Связанные источники",
  related_budget: "Предел связанных (байт)",
};
export function parameters(a: Attempt) {
  const requested = a.requested ?? {};
  const originalUnavailable = a.incomplete.includes("original_mcp_input_unavailable");
  const applied = { ...(a.applied ?? {}) };
  const search = applied.search;
  const sources = applied.sources as Fields | null;
  delete applied.search; delete applied.sources;
  if (search && typeof search === "object" && !Array.isArray(search)) {
    for (const [k, v] of Object.entries(search)) applied[k === "results" ? "limit" : k] = v;
  }
  return [...new Set([...Object.keys(requested), ...Object.keys(applied)])].map(key => ({
    key, label: labels[key] ?? key,
    requested: key in requested ? value(requested[key]) + (originalUnavailable ? " (типизированный снимок)" : "") : originalUnavailable ? "Исходный ввод недоступен" : "Не передано",
    applied: value(applied[key]),
    reason: ({ explicit: "Явно", default: "Умолчание на момент вызова", clamped: "Ограничено" } as Record<string, string>)[String(sources?.[key])] ?? "Источник не записан",
  }));
}

export function periodBounds(period: string, now = new Date()): Pick<HistoryFilter, "from" | "to"> {
  const end = Math.floor(now.getTime() / 1000) + 1;
  const midnight = new Date(now); midnight.setHours(0, 0, 0, 0);
  return { from: period === "today" ? Math.floor(midnight.getTime() / 1000) : end - (period === "week" ? 7 : 1) * 86400, to: end };
}
