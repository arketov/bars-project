import { describe, expect, it } from "vitest";
import {
  countsLabel,
  emptyResultLabel,
  isBusy,
  linesLabel,
  newTotal,
  roleLabel,
  stateNote,
  tagLabel,
  type AuditList,
  type AuditSummary,
} from "./audits";

const audit = (over: Partial<AuditSummary> = {}): AuditSummary => ({
  kind: "broken_links", state: "never", total: 0, new_count: 0, gone_count: 0, overlap_total: 0,
  schedule: "off", schedules: ["off", "daily", "weekly", "on_change"], ...over,
});
const list = (audits: AuditSummary[], over: Partial<AuditList> = {}): AuditList => ({ audits, reindex_required: false, indexing: false, ...over });

describe("подписи аудитов", () => {
  it("строки находки: диапазон или одна строка", () => {
    expect(linesLabel({ first_line: 14, last_line: 14 })).toBe("14");
    expect(linesLabel({ first_line: 12, last_line: 20 })).toBe("12–20");
  });

  it("роль и подвид с запасным значением", () => {
    expect(roleLabel("session_log")).toBe("журнал сессии");
    expect(roleLabel("что-то новое")).toBe("документ");
    expect(tagLabel("partial")).toBe("частичный");
    expect(tagLabel("новый")).toBe("новый");
  });

  it("число находок не показывается, пока полного запуска не было", () => {
    expect(countsLabel(audit())).toBe("");
    expect(countsLabel(audit({ result_run_id: 1, total: 0 }))).toBe("0 находок");
    expect(countsLabel(audit({ result_run_id: 1, total: 3, new_count: 1, gone_count: 2 }))).toBe("3 находки · 1 новая · 2 исчезли");
  });

  it("пустой результат называет дату и не обещает отсутствия проблем", () => {
    expect(emptyResultLabel(audit())).toBe("");
    const text = emptyResultLabel(audit({ result_run_id: 1, result_at: 1_700_000_000 }));
    expect(text).toContain("Ничего не найдено при запуске");
    expect(text).toContain("не доказывает");
  });

  it("идущий и ждущий вид заняты", () => {
    expect(isBusy(audit({ state: "running" }))).toBe(true);
    expect(isBusy(audit({ state: "queued" }))).toBe(true);
    expect(isBusy(audit({ state: "complete" }))).toBe(false);
  });

  it("пояснение: ошибка, неполный запуск, индекс не готов только у дублей", () => {
    const status = { reindex_required: false, indexing: false };
    expect(stateNote(audit({ state: "failed", error: "диск недоступен" }), status)).toBe("диск недоступен");
    expect(stateNote(audit({ state: "partial", error: "Идёт индексация" }), status)).toBe("Идёт индексация");
    expect(stateNote(audit({ kind: "duplicates", state: "complete" }), { reindex_required: true, indexing: false })).toContain("переиндексации");
    expect(stateNote(audit({ kind: "duplicates", state: "complete" }), { reindex_required: false, indexing: true })).toContain("Идёт индексация");
    expect(stateNote(audit({ kind: "broken_links", state: "complete" }), { reindex_required: true, indexing: true })).toBe("");
  });

  it("сумма новых находок по видам", () => {
    expect(newTotal(list([audit({ new_count: 2 }), audit({ kind: "duplicates", new_count: 3 })]))).toBe(5);
  });
});
