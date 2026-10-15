// Диалог согласия на переиндексацию (Т-25, reindex-consent.md): строки таблицы проектов и итог по отмеченным.

import { documents, duration } from "./format";
import { estimate } from "./settings-logic";

export interface ConsentRow {
  path: string;
  name: string;
  documents: number;
  chunks: number;
  /** Отпечаток модели, которой построен индекс, и модели из настроек. */
  indexModel: string | null;
  settingsModel: string;
}

export interface ConsentTotals {
  projects: number;
  documents: number;
  chunks: number;
  /** Суммарное время в секундах; `null`, если хотя бы у одного проекта оценки нет. */
  seconds: number | null;
  /** Почему оценки нет. */
  note: string;
}

export function totals(rows: ConsentRow[], selected: ReadonlySet<string>): ConsentTotals {
  const chosen = rows.filter((row) => selected.has(row.path));
  let seconds: number | null = 0;
  let note = "";
  for (const row of chosen) {
    const one = estimate(row.chunks, row.settingsModel);
    if (one.seconds === null) {
      seconds = null;
      note = one.note;
    } else if (seconds !== null) {
      seconds += one.seconds;
    }
  }
  return {
    projects: chosen.length,
    documents: chosen.reduce((sum, row) => sum + row.documents, 0),
    chunks: chosen.reduce((sum, row) => sum + row.chunks, 0),
    seconds,
    note,
  };
}

/** Время проекта для колонки таблицы: «≈ 11 мин», «зависит от провайдера». */
export function rowTime(row: ConsentRow): string {
  const one = estimate(row.chunks, row.settingsModel);
  return one.seconds === null ? one.note : `≈ ${duration(one.seconds)}`;
}

export function totalsText(summary: ConsentTotals): string {
  const base = `Выбрано: ${summary.projects} · ${documents(summary.documents)} · ${summary.chunks} чанков`;
  if (summary.projects === 0) return "Ничего не выбрано";
  if (summary.seconds === null) return `${base} · ${summary.note}`;
  return `${base} · ≈ ${duration(summary.seconds)}`;
}

/** Модель в настройках удалённая: текст чанков уйдёт провайдеру. */
export function isRemote(settingsModel: string): boolean {
  return !settingsModel.startsWith("builtin:");
}
