// Как окно называет ошибки воркера по-человечески (docs/features/window/error-states.md).

import type { BarsError } from "./types";

export type ErrorScreen =
  | { kind: "loading"; title: string; hint: string }
  | { kind: "provider"; title: string; hint: string }
  | { kind: "model-files"; title: string; hint: string }
  | { kind: "model-download"; title: string; hint: string }
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

/** Ошибка встроенной модели: нет связи с huggingface.co или файлов нет (OQ-48: оба случая приходят как `provider_error`). */
export function explainBuiltin(message: string): { kind: "model-download" | "model-files"; hint: string } {
  const text = message.toLowerCase();
  if (/huggingface|скача|загруз|download|network|сет[ьи]|соединени/.test(text)) {
    return {
      kind: "model-download",
      hint: "Нет связи с huggingface.co. Проверьте сеть и повторите; уже скачанная часть сохранена.",
    };
  }
  return {
    kind: "model-files",
    hint: "В ~/.bars/models нет нужных файлов или не сходится размер. Скачайте модель заново.",
  };
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
      const builtin = explainBuiltin(error.message);
      return {
        kind: builtin.kind,
        title: builtin.kind === "model-download" ? "Не удалось скачать embed-модель" : "Файлы модели не найдены или повреждены",
        hint: builtin.hint,
      };
    }
    return {
      kind: "provider",
      title: "Embed-провайдер не ответил",
      hint: explainProvider(error.message),
    };
  }
  return { kind: "other", title: "Поиск не выполнен", hint: error.message };
}
