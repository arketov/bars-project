import { expect, it } from "vitest";
import { NotificationCenter, operationText } from "./notifications";
import type { Operation, OperationEventsOutput } from "./types";

it("shows a scan as discovered differences, not an applied index or failed rebuild", () => {
  const result = operationText(finished({ kind: "scan", report: { ...report, index_applied: false } }));
  expect(result).toContain("Проверка завершена");
  expect(result).toContain("обновление в очереди");
  expect(result).not.toContain("не применена");
  expect(operationText(finished({ kind: "scan", report: { ...report, added: 0, changed: 0, removed: 0, index_applied: false } }))).toContain("различий не найдено");
});

const op = (patch: Partial<Operation> = {}): Operation => ({ id: 1, sequence: 0, kind: "check", source: "explicit", project: "a", generation: 1, model: null, variant: null, reranker: false, state: "running", started_at: "2026-10-18T10:00:00Z", finished_at: null, docs_done: 2, docs_total: 10, percent: null, report: null, error: null, target_available: true, ...patch });
const events = (active: Operation[] = [], completed: Operation[] = [], cursor = 0, epoch = "worker-1"): OperationEventsOutput => ({ epoch, cursor, gap: false, active, completed });
const report = { added: 1, changed: 2, removed: 3, unchanged: 4, problems: 0, index_applied: true };
const finished = (patch: Partial<Operation> = {}) => op({ sequence: 1, state: "success", finished_at: "2026-10-18T10:00:03Z", report, ...patch });

it("distinguishes independent file scans from queued index and document updates", () => {
  const c = new NotificationCenter();
  c.events(events([
    op({ id: 1, kind: "scan", state: "running" }),
    op({ id: 2, kind: "check", state: "queued" }),
    op({ id: 3, kind: "files", state: "queued" }),
  ]));
  expect(c.items.map(item => item.title).sort()).toEqual(["Обновление документов", "Обновление индекса", "Проверка файлов"]);
  c.events(events([
    op({ id: 1, kind: "scan", state: "running", docs_done: 3 }),
    op({ id: 2, kind: "check", state: "queued", queue_position: 0 }),
    op({ id: 3, kind: "files", state: "queued", queue_position: 1 }),
  ]));
  expect(c.items).toHaveLength(3);
  expect(c.items.find(item => item.operation?.id === 3)?.text).toContain("документов: 10");
});

it("updates one card without reordering or unread progress; completion requires attention", () => {
  const c = new NotificationCenter(); c.events(events([op()])); c.read();
  c.events(events([op({ docs_done: 4 })]));
  expect(c.items).toHaveLength(1); expect(c.items[0]).toMatchObject({ percent: 40, unread: false });
  c.events(events([], [finished()], 1));
  expect(c.items[0]).toMatchObject({ active: false, unread: true });
  expect(c.items[0].text).toContain("добавлено 1, изменено 2, удалено 3");
});

it("receives an operation entirely between polls and does not resurrect cleared outcomes", () => {
  const c = new NotificationCenter(); c.events(events()); c.events(events([], [finished()], 1));
  expect(c.items).toHaveLength(1); c.clear(); c.events(events([], [finished()], 1));
  expect(c.items).toHaveLength(0);
  c.events(events([], [finished({ id: 2, sequence: 2 })], 2)); expect(c.items).toHaveLength(1);
});

it("never infers success from disappearance, disconnect or new epoch; recovers same epoch", () => {
  const c = new NotificationCenter(); c.events(events([op()])); c.disconnected("Нет связи");
  expect(c.items[0].active).toBe(true); expect(c.connectionError).toBe("Нет связи");
  c.events(events([], [finished()], 1)); expect(c.items[0].unknown).toBeUndefined(); expect(c.connectionError).toBeNull();
  c.events(events([op({ id: 2 })], [], 1)); c.events(events([], [], 0, "worker-2"));
  expect(c.items.find(n => n.id === "worker-1:2")?.unknown).toBe(true);
  c.clear(); expect(c.items).toHaveLength(1);
});

it("keeps distinct projects and retries; empty background checks stay silent", () => {
  const c = new NotificationCenter(); c.events(events([op(), op({ id: 2, project: "b" })])); c.read();
  c.events(events([op({ id: 2, project: "b" })], [finished({ state: "cancelled" })], 1));
  c.events(events([op({ id: 3 }), op({ id: 2, project: "b" })], [], 1));
  expect(c.items[0].active).toBe(true); expect(c.items.at(-1)?.active).toBe(false);
  expect(c.items).toHaveLength(3); expect(c.items.find(n => n.id === "worker-1:1")?.operation?.project).toBe("a");
  c.events(events([], [finished({ id: 4, sequence: 2, source: "periodic", report: { ...report, added: 0, changed: 0, removed: 0 } })], 2));
  expect(c.items.some(n => n.id === "worker-1:4")).toBe(false);
});

it("distinguishes confirmed pause, request, stop, partial result, unapplied rebuild and model readiness", () => {
  for (const [state, text] of [["pause_requested", "Пауза запрошена"], ["paused", "На паузе"], ["stop_requested", "Остановка запрошена"]]) expect(operationText(op({ state }))).toContain(text);
  expect(operationText(finished({ state: "partial", report: { ...report, problems: 2 } }))).toContain("с ошибками: 2");
  expect(operationText(finished({ state: "partial", report: { ...report, index_applied: false } }))).toContain("не применена");
  expect(operationText(finished({ kind: "download", report: null }))).toBe("Файлы модели проверены и готовы");
  expect(operationText(finished({ kind: "prepare", report: null }))).toBe("Модель готова");
  expect(operationText(op({ kind: "prepare", percent: 100 }))).toContain("подготовка модели");
  expect(operationText(finished({ state: "error", report: null, error: "Сбой подготовки" }))).toBe("Сбой подготовки");
});

it("versioned reading does not consume a later result; clearing retains active work", () => {
  const c = new NotificationCenter(); c.events(events([op()])); const version = c.version(c.items[0].id);
  c.events(events([], [finished()], 1)); c.read(c.items[0].id, version);
  expect(c.items[0].unread).toBe(true); c.read(c.items[0].id, c.version(c.items[0].id)); expect(c.items[0].unread).toBe(false);
  c.events(events([op({ id: 2 })], [], 1)); c.clear(); expect(c.items).toHaveLength(1); expect(c.items[0].active).toBe(true);
});

it("bounds completed history without evicting active or unknown operations; deduplicates cleared action error", () => {
  const c = new NotificationCenter(); c.events(events([op()]));
  for (let i = 0; i < 150; i++) c.error(`error:${i}`, "Ошибка", `${i}`);
  expect(c.items).toHaveLength(101); c.clear(); c.error("error:149", "Ошибка", "149"); expect(c.items).toHaveLength(1);
});

it("deduplicates an unresolved background problem, and requires attention after recovery", () => {
  const c = new NotificationCenter(); c.events(events());
  c.events(events([], [finished({ state: "error", source: "periodic", report: null, error: "Недоступно" })], 1)); c.read();
  c.events(events([], [finished({ id: 2, sequence: 2, state: "error", source: "periodic", report: null, error: "Недоступно" })], 2));
  expect(c.items).toHaveLength(1); expect(c.items[0].unread).toBe(false);
  c.clear(); c.events(events([], [finished({ id: 3, sequence: 3, state: "error", source: "periodic", report: null, error: "Недоступно" })], 3)); expect(c.items).toHaveLength(0);
  c.events(events([], [finished({ id: 4, sequence: 4, source: "periodic" })], 4));
  c.events(events([], [finished({ id: 5, sequence: 5, state: "error", source: "periodic", report: null, error: "Недоступно" })], 5));
  expect(c.items.some(n => n.bad && n.unread)).toBe(true);
});
