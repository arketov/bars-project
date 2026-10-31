// Как окно называет ошибки воркера по-человечески (docs/features/window/error-states.md).

import type { BarsError } from "./types";

/** Вид ошибки встроенной модели: значения `data.reason` у `provider_error` (error-states.md). */
export type BuiltinReason = "network" | "files_missing" | "out_of_memory" | "runtime";

export type ErrorScreen =
  | { kind: "loading"; title: string; hint: string }
  | { kind: "provider"; title: string; hint: string }
  | { kind: "builtin"; reason: BuiltinReason; title: string; hint: string }
  | { kind: "other"; title: string; hint: string };

/** Источник embed-модели по отпечатку из `status`: `builtin:BAAI/bge-m3` → `builtin`. */
export function sourceOf(fingerprint: string | undefined | null): "builtin" | "remote" | "local-openai" | null {
  const source = fingerprint?.split(":")[0];
  return source === "builtin" || source === "remote" || source === "local-openai" ? source : null;
}

/** Причина и действие для ошибки провайдера: таблица из error-states.md. */
export function explainProvider(message: string): string {
  const text = message.toLowerCase();
  if (/\b40[13]\b/.test(text) || text.includes("не принят")) {
    return "Ключ не принят: проверьте ключ в настройках, на вкладке «Модели», или замените его.";
  }
  if (text.includes("хранилищ") && (text.includes("нет ключа") || text.includes("не найден") || text.includes("ключ"))) {
    return "Ключа нет в хранилище ОС: задайте ключ заново на вкладке «Модели».";
  }
  if (text.includes("слишком длин") || text.includes("too long") || text.includes("context length")) {
    return "Вход слишком длинный: уменьшите максимальный размер чанка в настройках, на вкладке «Нарезка».";
  }
  if (/\b5\d\d\b/.test(text) || /таймаут|timed out|timeout|не отвеча|connection|соединени|сет[ьи]/.test(text)) {
    return "Нет сети, таймаут или ошибка сервера: повторите позже. Индексация при этом сама делает до трёх повторов.";
  }
  return "Проверьте адрес и ключ провайдера в настройках, на вкладке «Модели».";
}

/** Вид ошибки по `data.reason` воркера; пустое и неизвестное значение читается как `runtime`, а не как «файлов нет». */
export function builtinReason(reason: string | null | undefined): BuiltinReason {
  return reason === "network" || reason === "files_missing" || reason === "out_of_memory" ? reason : "runtime";
}

const BUILTIN_SCREENS: Record<BuiltinReason, { title: string; hint: string }> = {
  network: {
    title: "Не удалось скачать модель поиска",
    hint: "Нет связи с huggingface.co. Проверьте сеть и скачайте снова; уже скачанная часть сохранена.",
  },
  files_missing: {
    title: "Файлы модели не найдены или повреждены",
    hint: "В папке моделей нет нужных файлов или не сходится размер. Скачайте модель заново.",
  },
  out_of_memory: {
    title: "Модели не хватило памяти",
    hint: "Закройте тяжёлые программы или посчитайте модель на процессоре: выберите другое устройство в настройках.",
  },
  runtime: {
    title: "Встроенная модель не отработала",
    hint: "Повторите поиск или выберите другую модель. Подробности — в тексте ошибки ниже.",
  },
};

/** Идентификатор встроенной модели проекта по отпечатку: `builtin:BAAI/bge-m3` → `BAAI/bge-m3`. */
export function builtinModelId(fingerprint: string | undefined | null): string | null {
  return fingerprint?.startsWith("builtin:") ? fingerprint.slice("builtin:".length) || null : null;
}

/** Экран для ошибки поиска. `source` — откуда берётся embed-модель проекта. */
export function screenFor(error: BarsError, source: ReturnType<typeof sourceOf>): ErrorScreen {
  if (error.kind === "model_loading") {
    const percent = error.data?.percent;
    return {
      kind: "loading",
      title: percent !== undefined && percent < 100 ? `Модель грузится, ${percent} %` : "Модель загружается в память",
      hint: "Окно повторит поиск само. Окно можно закрыть: скачивание идёт в фоне.",
    };
  }
  if (error.kind === "provider_error") {
    if (source === "builtin") {
      const reason = builtinReason(error.data?.reason);
      return { kind: "builtin", reason, ...BUILTIN_SCREENS[reason] };
    }
    return {
      kind: "provider",
      title: "Embed-провайдер не ответил",
      hint: explainProvider(error.message),
    };
  }
  return { kind: "other", title: "Поиск не выполнен", hint: error.message };
}
