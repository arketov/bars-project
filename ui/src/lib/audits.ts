// Аудиты знаний: типы ответов воркера и подписи экрана (docs/features/knowledge-base/audits.md).
// Аудит ничего не меняет в файлах; находка — повод посмотреть документ, а не вердикт.

import { plural } from "./format";

export type AuditKind = "broken_links" | "duplicates" | "contradictions";
export type AuditState = "never" | "queued" | "running" | "complete" | "partial" | "failed";
export type ScheduleMode = "off" | "daily" | "weekly" | "on_change";
export type Standing = "new" | "known" | "gone";
export type FindingGroup = "main" | "overlap";

export interface AuditSummary {
  kind: AuditKind;
  state: AuditState;
  running_since?: number;
  last_run_at?: number;
  last_status?: "complete" | "partial" | "failed" | "cancelled";
  error?: string;
  result_run_id?: number;
  result_at?: number;
  total: number;
  new_count: number;
  gone_count: number;
  overlap_total: number;
  schedule: ScheduleMode;
  schedules: ScheduleMode[];
}

export interface AuditList {
  audits: AuditSummary[];
  reindex_required: boolean;
  indexing: boolean;
}

export interface AuditSubject {
  path: string;
  first_line: number;
  last_line: number;
  heading: string;
  role: string;
}

export interface AuditFinding {
  fingerprint: string;
  ignored: boolean;
  standing: Standing;
  tag: string;
  group: FindingGroup;
  reason: string;
  score?: number;
  subjects: AuditSubject[];
  detail: string;
}

export interface AuditFindingsPage {
  run_id?: number;
  finished?: number;
  items: AuditFinding[];
  total: number;
  next?: number;
}

export interface AuditRunAnswer {
  accepted: boolean;
  already: boolean;
  queued: boolean;
}

export const KIND_LABEL: Record<AuditKind, string> = {
  broken_links: "Битые ссылки",
  duplicates: "Дубли",
  contradictions: "Противоречия",
};

export const KIND_HINT: Record<AuditKind, string> = {
  broken_links: "Ссылки на документы, которых нет в проекте",
  duplicates: "Повторяющиеся описания в разных разделах",
  contradictions:
    "Почти одинаковые утверждения с разными значениями в разных документах; без журналов, датированных снимков и разделов с датой",
};

export const STATE_LABEL: Record<AuditState, string> = {
  never: "не запускался",
  queued: "ждёт очереди",
  running: "идёт",
  complete: "готово",
  partial: "неполно",
  failed: "ошибка",
};

export const SCHEDULE_LABEL: Record<ScheduleMode, string> = {
  off: "выкл",
  daily: "раз в день",
  weekly: "раз в неделю",
  on_change: "при изменении",
};

export const ROLE_LABEL: Record<string, string> = {
  index: "оглавление",
  session_log: "журнал сессии",
  manual: "руководство",
  feature: "фича",
  architecture: "архитектура",
  decision: "решение",
  research: "исследование",
  backlog: "бэклог",
  document: "документ",
};

export const TAG_LABEL: Record<string, string> = {
  broken: "битая ссылка",
  exact: "точный",
  near: "близкий",
  contained: "содержащийся",
  partial: "частичный",
  semantic: "смысловой",
  num: "число с единицей",
  slot: "значение величины",
  swap: "замена значения",
};

/** Что показывает вид `duplicates` по умолчанию (3.8.2: сильные частичные — 6 из 7 настоящие). */
export const DUPLICATES_DEFAULT = "точные · близкие · содержащиеся · частичные ≥ 0,4";

export const isBusy = (summary: AuditSummary): boolean => summary.state === "running" || summary.state === "queued";

export const roleLabel = (role: string): string => ROLE_LABEL[role] ?? ROLE_LABEL.document;

export const tagLabel = (tag: string): string => TAG_LABEL[tag] ?? tag;

/** «12–20» или «14» для строк, к которым относится находка. */
export const linesLabel = (subject: Pick<AuditSubject, "first_line" | "last_line">): string =>
  subject.last_line > subject.first_line ? `${subject.first_line}–${subject.last_line}` : String(subject.first_line);

const findingsWord = (n: number): string => plural(n, ["находка", "находки", "находок"]);

/** Число находок и новых; пока полного запуска не было — пусто (нуль нельзя выдавать за «всё хорошо»). */
export function countsLabel(summary: AuditSummary): string {
  if (summary.result_run_id === undefined) return "";
  const parts = [`${summary.total} ${findingsWord(summary.total)}`];
  if (summary.new_count > 0) parts.push(`${summary.new_count} ${plural(summary.new_count, ["новая", "новые", "новых"])}`);
  if (summary.gone_count > 0) parts.push(`${summary.gone_count} ${plural(summary.gone_count, ["исчезла", "исчезли", "исчезли"])}`);
  return parts.join(" · ");
}

/** Время секундами Unix → «22.10.2026, 14:03». */
export const stamp = (seconds: number): string =>
  new Date(seconds * 1000).toLocaleString("ru-RU", { day: "2-digit", month: "2-digit", year: "numeric", hour: "2-digit", minute: "2-digit" });

/** Пустой результат — с датой запуска, а не «всё хорошо» (audits.md, «Экран аудитов»). */
export const emptyResultLabel = (summary: AuditSummary): string =>
  summary.result_at === undefined ? "" : `Ничего не найдено при запуске ${stamp(summary.result_at)}. Это не доказывает, что проблем нет.`;

export function stateNote(summary: AuditSummary, list: Pick<AuditList, "reindex_required" | "indexing">): string {
  if (summary.state === "failed") return summary.error || "Запуск не удался.";
  if (summary.state === "partial") return summary.error || "Запуск неполный: прежний полный результат сохранён.";
  if (summary.kind !== "broken_links" && list.reindex_required) return "Индекс требует переиндексации: аудит не запустить, показан прежний результат.";
  if (summary.kind !== "broken_links" && list.indexing) return "Идёт индексация: результат может быть неполным.";
  return "";
}

/** Сколько новых находок стало у вида: для уведомления только при приросте. */
export const newTotal = (list: AuditList): number => list.audits.reduce((sum, audit) => sum + audit.new_count, 0);
