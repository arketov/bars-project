// «Новые темы»: типы ответов воркера и чистые функции экрана (docs/features/new-topics/design.md).
// Формат ответов — docs/architecture/protocol.md, «Методы новых тем (только окно)».

import { plural } from "./format";

export type TopicStatus = "open" | "materials_found" | "resolved";
export type TopicFilter = "active" | "materials_found" | "resolved";
export type TopicSort = "recent" | "count";
export type CheckOutcome = "found" | "empty" | "incomplete";

export interface TopicSummary {
  id: number;
  revision: number;
  title: string;
  status: TopicStatus;
  first_seen: number;
  last_seen: number;
  resolved_at?: number | null;
  observations: number;
  sources: string[];
  reasons: string[];
  variants: number;
}

export interface TopicsList {
  enabled: boolean;
  generation: number;
  topics: TopicSummary[];
  store_path: string;
  losses: number;
  loss_reason?: string | null;
}

export interface TopicVariant { id: number; text: string; scope: string; original_params?: string }

export interface TopicHit {
  path: string;
  title: string;
  heading_chain: string;
  first_line: number;
  last_line: number;
  score: number;
  score_kind: "rerank" | "cosine";
}

export interface CheckResult {
  variant_id: number;
  scope: string;
  outcome: CheckOutcome;
  reason?: string | null;
  index_stamp?: string | null;
  hits: TopicHit[];
}

export interface TopicCheck {
  id: number;
  topic_revision: number;
  at: number;
  expanded: boolean;
  model: string;
  params: string;
  conclusive: boolean;
  results: CheckResult[];
}

export interface TopicDetail extends TopicSummary {
  variants_list: TopicVariant[];
  checks: TopicCheck[];
}

export const STATUS_LABEL: Record<TopicStatus, string> = {
  open: "Новая тема",
  materials_found: "Найдены совпадения",
  resolved: "Закрыта",
};

export const FILTER_LABEL: Record<TopicFilter, string> = {
  active: "Активные",
  materials_found: "Найдены совпадения",
  resolved: "Закрытые",
};

export const SOURCE_LABEL: Record<string, string> = { window: "Окно", mcp: "MCP" };

export const REASON_LABEL: Record<string, string> = {
  empty_result: "Поиск ничего не вернул",
  manual_missing: "Пользователь отметил недостаток знаний",
};

const INCOMPLETE_LABEL: Record<string, string> = {
  indexing: "идёт индексация",
  rerank_unavailable: "rerank недоступен; проверка знаний не завершена",
  model_loading: "модель ещё загружается",
  reindex_required: "нужна переиндексация после смены модели",
  scope_missing: "область больше недоступна",
  error: "ошибка поиска",
};

/** Тема ждёт внимания пользователя: всё, кроме закрытых. */
export const isActive = (topic: Pick<TopicSummary, "status">): boolean => topic.status !== "resolved";

export function activeCount(topics: readonly TopicSummary[]): number {
  return topics.filter(isActive).length;
}

/** Фильтр «Активные» включает все незакрытые; остальные — точный статус. */
export function matchesFilter(topic: TopicSummary, filter: TopicFilter): boolean {
  return filter === "active" ? isActive(topic) : topic.status === filter;
}

/** Поиск по тексту внутри списка: без учёта регистра, по заголовку. */
export function matchesQuery(topic: TopicSummary, query: string): boolean {
  const needle = query.trim().toLocaleLowerCase("ru");
  return !needle || topic.title.toLocaleLowerCase("ru").includes(needle);
}

/** Сортировка: по последнему обращению либо по числу обращений; равные — по более свежему, затем по номеру. */
export function sortTopics(topics: readonly TopicSummary[], sort: TopicSort): TopicSummary[] {
  const by = (a: TopicSummary, b: TopicSummary) =>
    (sort === "count" ? b.observations - a.observations : 0) || b.last_seen - a.last_seen || b.id - a.id;
  return [...topics].sort(by);
}

export function visibleTopics(topics: readonly TopicSummary[], filter: TopicFilter, query: string, sort: TopicSort): TopicSummary[] {
  return sortTopics(topics.filter((topic) => matchesFilter(topic, filter) && matchesQuery(topic, query)), sort);
}

export const sourcesLabel = (sources: readonly string[]): string =>
  sources.map((source) => SOURCE_LABEL[source] ?? source).join(" · ") || "—";

export const reasonsLabel = (reasons: readonly string[]): string =>
  reasons.map((reason) => REASON_LABEL[reason] ?? reason).join("; ") || "—";

/** «4 обращения»: метка говорит «обращений», не «пользователей» и не «задач» (design.md). */
export const observationsLabel = (count: number): string =>
  `${count} ${plural(count, ["обращение", "обращения", "обращений"])}`;

/** Область варианта: `project` или `m=<модули>;f=<файлы>` (`topic_scope` воркера). */
export function scopeLabel(scope: string): string {
  if (scope === "project") return "Весь проект";
  const match = /^m=([^;]*);f=(.*)$/.exec(scope);
  if (!match) return scope;
  const parts: string[] = [];
  if (match[1]) parts.push(`модули: ${match[1].split(",").join(", ")}`);
  if (match[2]) parts.push(`файлы: ${match[2].split(",").join(", ")}`);
  return parts.join("; ") || "Весь проект";
}

/** Время воркера — секунды Unix. */
export const secondsToDate = (seconds: number): Date => new Date(seconds * 1000);

/** «только что», «18 мин назад», «3 ч назад», «вчера», иначе дата. */
export function ago(seconds: number, now: number = Date.now()): string {
  const delta = Math.max(0, Math.round(now / 1000 - seconds));
  if (delta < 60) return "только что";
  const minutes = Math.round(delta / 60);
  if (minutes < 60) return `${minutes} мин назад`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} ч назад`;
  const days = Math.round(hours / 24);
  if (days === 1) return "вчера";
  if (days < 14) return `${days} ${plural(days, ["день", "дня", "дней"])} назад`;
  return secondsToDate(seconds).toLocaleDateString("ru-RU");
}

export const dateTime = (seconds: number): string =>
  secondsToDate(seconds).toLocaleString("ru-RU", { day: "2-digit", month: "2-digit", year: "numeric", hour: "2-digit", minute: "2-digit" });

/** Итог одной проверки по-русски: что именно нашлось по каждому варианту. */
export function outcomeLabel(result: CheckResult): string {
  if (result.outcome === "found") return `Найдено совпадений: ${result.hits.length}`;
  if (result.outcome === "empty") return "Материалы не найдены при этой проверке";
  const reason = result.reason ? (INCOMPLETE_LABEL[result.reason] ?? result.reason) : "причина не указана";
  return `Проверка неполная: ${reason}`;
}

export interface CheckSummary {
  /** Всех вариантов найдено/пусто/неполных. */
  found: number;
  empty: number;
  incomplete: number;
  total: number;
  /** Вывод «материалы не найдены» допустим только при `conclusive` (design.md). */
  headline: "found" | "empty" | "incomplete";
}

export function summarizeCheck(check: TopicCheck): CheckSummary {
  const count = (outcome: CheckOutcome) => check.results.filter((result) => result.outcome === outcome).length;
  const found = count("found");
  const incomplete = count("incomplete");
  const headline = found > 0 ? "found" : incomplete > 0 || !check.conclusive ? "incomplete" : "empty";
  return { found, empty: count("empty"), incomplete, total: check.results.length, headline };
}

/** Проверка устарела, если тема менялась после её начала (design.md, «Внутренний API окна»). */
export const checkIsStale = (check: TopicCheck, topic: Pick<TopicSummary, "revision">): boolean =>
  check.topic_revision < topic.revision;

/** Применённые параметры поиска из JSON проверки; невалидный JSON не ломает экран. */
export function appliedParams(check: TopicCheck): string {
  try {
    const value = JSON.parse(check.params) as Record<string, unknown>;
    const parts: string[] = [];
    if (value.score_kind === "rerank" || value.score_kind === "cosine") parts.push(`оценка ${value.score_kind}`);
    if (typeof value.min_score === "number") parts.push(`порог ${value.min_score.toFixed(2).replace(".", ",")}`);
    if (typeof value.results === "number") parts.push(`результатов ${value.results}`);
    if (typeof value.candidates === "number") parts.push(`кандидатов ${value.candidates}`);
    return parts.join(" · ");
  } catch {
    return "";
  }
}

/** Каждый вариант мог использовать свою оценку, в том числе при сбое rerank в середине проверки. */
export function variantCheckParams(check: TopicCheck, variantId: number): { params: string; rerankError: string | null } {
  try {
    const value = JSON.parse(check.params);
    const perVariant = Array.isArray(value.variants);
    const variant = perVariant ? value.variants.find((item: { variant_id: number }) => item.variant_id === variantId) : null;
    return { params: perVariant ? JSON.stringify(variant?.applied ?? {}) : check.params, rerankError: typeof variant?.rerank_error === "string" ? variant.rerank_error : null };
  } catch {
    return { params: check.params, rerankError: null };
  }
}

export function scoreKindChanged(original: string | undefined, current: string): boolean {
  try {
    return JSON.parse(original ?? "{}").score_kind === "rerank" && JSON.parse(current).score_kind === "cosine";
  } catch {
    return false;
  }
}

/** Строки результата: «42–61» либо «42», если строка одна. */
export const linesLabel = (hit: Pick<TopicHit, "first_line" | "last_line">): string =>
  hit.first_line === hit.last_line ? String(hit.first_line) : `${hit.first_line}–${hit.last_line}`;

/** Счётчик справа от строки дерева: активные темы; `null` — ещё нет данных или ошибка чтения. */
export const badgeText = (count: number | null): string => (count === null ? "—" : String(count));

/** ID отображённого обращения окна; воркер и ручная отметка опираются на него (идемпотентность). */
export function newRequestId(): string {
  const random = typeof crypto !== "undefined" && "randomUUID" in crypto ? crypto.randomUUID() : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  return `window:${random}`;
}
