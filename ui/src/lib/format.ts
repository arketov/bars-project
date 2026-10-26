// Подписи окна: имена для людей, оценка результата, числа и время по-русски.

import type { JobKind, JobPriority, ProjectInfo, ProjectState, ProjectProblems, ScoreKind } from "./types";

/** `hybrid-search.md` → «Hybrid search»; служебные имена вроде «(корень)» не трогаем. */
export function humanName(name: string): string {
  if (name.startsWith("(")) return name;
  const text = name.replace(/\.md$/i, "").replace(/[-_]+/g, " ").trim();
  return text ? text[0].toUpperCase() + text.slice(1) : name;
}

/** Корневой модуль воркера называется `.`; окно показывает «(корень)». */
export function moduleLabel(module: string): string {
  return module === "." ? "(корень)" : humanName(module);
}

export type Fit = { cls: "f-good" | "f-mid" | "f-low" | "f-cosine"; label: string; percent: number; explanation: string };

/** Полоска показывает оценку модели, а не вероятность правильного ответа (OQ-77). */
export function fit(score: number, kind: ScoreKind): Fit {
  const percent = Math.round(Math.max(0, Math.min(1, score)) * 100);
  if (kind === "cosine") return {
    cls: "f-cosine", label: "Косинусная близость", percent,
    explanation: "Смысловая близость, а не вероятность правильного ответа. Гибридный порядок учитывает также совпадения слов и точные идентификаторы; близость может возрастать вниз по списку.",
  };
  return {
    cls: score >= 0.75 ? "f-good" : score >= 0.5 ? "f-mid" : "f-low",
    label: "Оценка rerank", percent,
    explanation: "Оценка модели rerank, а не вероятность правильного ответа. Результаты упорядочены по этой оценке.",
  };
}

/** 0.18 → «0,18». */
export function decimal(value: number, digits = 2): string {
  return value.toFixed(digits).replace(".", ",");
}

/** Русское склонение: plural(1, ["документ", "документа", "документов"]). */
export function plural(count: number, forms: [string, string, string]): string {
  const n = Math.abs(count) % 100;
  const last = n % 10;
  if (n > 10 && n < 20) return forms[2];
  if (last === 1) return forms[0];
  if (last >= 2 && last <= 4) return forms[1];
  return forms[2];
}

export const documents = (n: number) => `${n} ${plural(n, ["документ", "документа", "документов"])}`;

/** «3 мин», «1 ч 20 мин», «меньше минуты». */
export function duration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "—";
  if (seconds < 60) return "меньше минуты";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} мин`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest ? `${hours} ч ${rest} мин` : `${hours} ч`;
}

/** Оценка оставшегося времени по фактической скорости; `null`, пока скорости нет (OQ-58). */
export function remainingSeconds(done: number, total: number, elapsed: number): number | null {
  if (done <= 0 || total <= done || elapsed <= 0) return null;
  return ((total - done) / done) * elapsed;
}

/** Процент 0…100 без деления на ноль. */
export function percentOf(done: number, total: number): number {
  return total > 0 ? Math.min(100, Math.round((done / total) * 100)) : 0;
}

/** Последняя часть пути для заголовка. */
export function baseName(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/").filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export const SCORE_LABEL: Record<ScoreKind, string> = { rerank: "rerank", cosine: "cosine" };

export function jobReason(kind: JobKind, priority: JobPriority, files: number): string {
  if (kind === "rebuild") return "полная переиндексация по согласию";
  if (kind === "files") return `правки файлов, ${documents(files)}`;
  return priority === "periodic" ? "первичное или фоновое обновление" : "обновление по запросу";
}

export interface DotInfo {
  cls: "ok" | "busy" | "warn" | "off";
  text: string;
}

/** Точка проекта и подпись в списке (main-window.md). */
export function problemStatus(problem: ProjectProblems): string {
  if (problem.rebuild_not_applied) return "Полная переиндексация не применена";
  if (problem.last_error) return "Проверка индекса остановлена";
  return "Индекс обновлён с ошибками в документах";
}

export function projectDot(project: ProjectInfo, progressPercent: number | null, waiting: number, problem?: ProjectProblems): DotInfo {
  if (problem && project.state === "ready") return { cls: "warn", text: problemStatus(problem) };
  return stateDot(project.state, documentsIn(project), progressPercent, waiting);
}

export function documentsIn(project: ProjectInfo): number {
  return project.modules.reduce((sum, module) => sum + module.documents, 0);
}

export function stateDot(
  state: ProjectState,
  docs: number,
  progressPercent: number | null,
  waiting: number,
): DotInfo {
  switch (state) {
    case "ready":
      return { cls: "ok", text: `актуален · ${documents(docs)}` };
    case "indexing":
      return {
        cls: "busy",
        text:
          progressPercent === null
            ? "индексация · ждёт"
            : `индексация · ${progressPercent} %${waiting ? ` · ждут ${waiting}` : ""}`,
      };
    case "reindex_required":
      return { cls: "warn", text: "нужна переиндексация: сменилась модель" };
    case "unavailable":
      return { cls: "off", text: "папка не найдена" };
  }
}

/** Запрос в одну строку: переносы (любые) становятся пробелами. */
export function singleLine(text: string): string {
  return text.replace(/\s*[\r\n]+\s*/g, " ");
}
