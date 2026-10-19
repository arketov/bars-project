// Логика вкладок настроек без интерфейса: проверка значений, признак переиндексации, оценка времени.

import type { EmbedDoc, ModelSourceName, SettingsDoc } from "./types";

/** Глубокая копия настроек. Через JSON, а не `structuredClone`: тот не копирует прокси `$state` из Svelte. */
export const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;

/** Изменилось ли что-то относительно сохранённого. */
export function isDirty(saved: SettingsDoc, draft: SettingsDoc): boolean {
  return JSON.stringify(saved) !== JSON.stringify(draft);
}

/** Отпечаток embed-модели: источник и модель, как их сравнивает воркер (Т-25). */
export function embedFingerprint(embed: Pick<EmbedDoc, "source" | "model">): string {
  return `${embed.source}:${embed.model.trim()}`;
}

/** Потребует ли сохранение переиндексации проектов, построенных прежней моделью. */
export function needsReindex(saved: SettingsDoc, draft: SettingsDoc): boolean {
  return embedFingerprint(saved.embed) !== embedFingerprint(draft.embed);
}

/** Источник и модель из отпечатка воркера `builtin:BAAI/bge-m3`. */
export function parseFingerprint(
  fingerprint: string | null | undefined,
): { source: ModelSourceName; model: string } | null {
  if (!fingerprint) return null;
  const index = fingerprint.indexOf(":");
  if (index < 0) return null;
  const source = fingerprint.slice(0, index);
  if (source !== "builtin" && source !== "remote" && source !== "local-openai") return null;
  return { source, model: fingerprint.slice(index + 1) };
}

/** Наибольший срок выгрузки модели, минут (неделя), как у воркера (settings.md). */
export const MAX_UNLOAD_MINUTES = 10_080;

const isHttpUrl = (text: string) => /^https?:\/\/[^\s/@]+(:\d+)?(\/\S*)?$/i.test(text.trim());

/** Ошибки значений по тем же правилам, что у воркера при чтении (settings.md); пусто — можно сохранять. */
export function validate(doc: SettingsDoc): Record<string, string> {
  const errors: Record<string, string> = {};
  const positive = (key: string, value: number, label: string) => {
    if (!Number.isFinite(value) || value < 1 || !Number.isInteger(value)) {
      errors[key] = `${label}: целое число больше нуля`;
    }
  };
  const embed = doc.embed;
  if (embed.source !== "builtin") {
    if (!embed.model.trim()) errors["embed.model"] = "Укажите модель провайдера";
    if (!embed.base_url || !isHttpUrl(embed.base_url)) {
      errors["embed.base_url"] = "Адрес должен начинаться с http:// или https://";
    }
    if (embed.source === "remote" && !embed.api_key_name?.trim() && !embed.api_key_env?.trim()) {
      errors["embed.key"] = "Удалённому провайдеру нужен ключ или переменная окружения";
    }
  }
  positive("embed.batch_size", embed.batch_size, "Пачка");
  for (const [key, value] of [
    ["embed.unload_after_minutes", embed.unload_after_minutes],
    ["rerank.unload_after_minutes", doc.rerank.unload_after_minutes],
  ] as const) {
    if (!Number.isInteger(value) || value < 0 || value > MAX_UNLOAD_MINUTES) {
      errors[key] = `Целое число от 0 до ${MAX_UNLOAD_MINUTES}; 0 — не выгружать`;
    }
  }
  const rerank = doc.rerank;
  if (rerank.enabled && rerank.source !== "builtin") {
    if (!rerank.model.trim()) errors["rerank.model"] = "Укажите модель провайдера";
    if (!rerank.base_url || !isHttpUrl(rerank.base_url)) {
      errors["rerank.base_url"] = "Адрес должен начинаться с http:// или https://";
    }
    if (rerank.source === "remote" && !rerank.api_key_name?.trim() && !rerank.api_key_env?.trim()) {
      errors["rerank.key"] = "Удалённому провайдеру нужен ключ или переменная окружения";
    }
  }
  const search = doc.search;
  positive("search.candidates", search.candidates, "Кандидаты");
  positive("search.rerank_candidates", search.rerank_candidates, "Кандидаты на реранк");
  positive("search.results", search.results, "Результатов без реранка");
  positive("search.results_reranked", search.results_reranked, "Результатов с реранком");
  positive("search.max_results", search.max_results, "Предел результатов");
  positive("search.max_candidates", search.max_candidates, "Предел кандидатов");
  positive("search.max_rerank_candidates", search.max_rerank_candidates, "Предел кандидатов на реранк");
  for (const [key, value] of [
    ["search.min_score_rerank", search.min_score_rerank],
    ["search.min_score_vector", search.min_score_vector],
  ] as const) {
    if (!(value >= 0 && value <= 1)) errors[key] = "Оценка лежит от 0 до 1";
  }
  const [low, high] = search.agent_min_score;
  if (!(low >= 0 && high <= 1) || low > high) {
    errors["search.agent_min_score"] = "Пределы: от 0 до 1, «от» не выше «до»";
  }
  positive("chunking.min_chars", doc.chunking.min_chars, "Минимум чанка");
  positive("chunking.max_chars", doc.chunking.max_chars, "Максимум чанка");
  return errors;
}

/** «0,30» или «0.3» → число; `NaN`, если не число. */
export function parseScore(text: string): number {
  return Number(text.trim().replace(",", "."));
}

/** Оценка времени переиндексации (OQ-58): известна только для bge-m3, около 0,5 с на чанк. */
export function estimate(chunks: number, fingerprint: string | null): { seconds: number | null; note: string } {
  const parsed = parseFingerprint(fingerprint);
  if (!parsed) return { seconds: null, note: "—" };
  if (parsed.source !== "builtin") return { seconds: null, note: "зависит от провайдера" };
  if (parsed.model === "BAAI/bge-m3") return { seconds: chunks * 0.5, note: "" };
  return { seconds: null, note: "будет известно после первых пачек" };
}
