import { beforeEach, expect, it, vi } from "vitest";
import { call } from "./api";
import { AuditsStore } from "./audits.svelte";
import type { AuditFinding, AuditList, AuditSummary } from "./audits";
import { BarsError } from "./types";

vi.mock("./api", () => ({ call: vi.fn() }));
const invoke = vi.mocked(call);
const P = "C:/проект";
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const summary = (kind: AuditSummary["kind"], over: Partial<AuditSummary> = {}): AuditSummary => ({
  kind, state: "never", total: 0, new_count: 0, gone_count: 0, overlap_total: 0, schedule: "off", schedules: ["off", "daily", "weekly"], ...over,
});
const finding = (n: number, group: AuditFinding["group"] = "main"): AuditFinding => ({
  standing: "known", tag: "partial", group, reason: "Заметная часть текста скопирована", score: 0.6,
  subjects: [{ path: `docs/${n}.md`, first_line: 1, last_line: 4, heading: "", role: "manual" }], detail: "",
});
let audits: AuditSummary[];
let flash: ReturnType<typeof vi.fn<(text: string, bad?: boolean) => void>>;
let store: AuditsStore;
let pages: Record<string, AuditFinding[]>;
const listOf = (): AuditList => ({ audits, reindex_required: false, indexing: false });

beforeEach(() => {
  vi.clearAllMocks();
  audits = [summary("broken_links"), summary("duplicates")];
  pages = { duplicates: Array.from({ length: 5 }, (_, n) => finding(n)), broken_links: [] };
  flash = vi.fn<(text: string, bad?: boolean) => void>();
  store = new AuditsStore(flash);
  store.bind(P);
  invoke.mockImplementation(async (method, params) => {
    const p = params as { kind?: string; group?: string; cursor?: number };
    switch (method) {
      case "audit_list": return listOf();
      case "audit_run": return { accepted: true, already: false, queued: false };
      case "audit_cancel": return { cancelled: true };
      case "audit_schedule": return { mode: "weekly" };
      case "audit_findings": {
        const all = (pages[p.kind!] ?? []).filter((f) => !p.group || f.group === p.group);
        const from = p.cursor ?? 0;
        const items = all.slice(from, from + 2);
        return { run_id: 7, finished: 1, items, total: all.length, next: from + items.length < all.length ? from + items.length : undefined };
      }
      default: throw new Error(`unexpected ${method}`);
    }
  });
});

it("ошибка чтения не выглядит нулём, а фоновое обновление не затирает данные ошибкой", async () => {
  invoke.mockRejectedValueOnce(new BarsError("доступ запрещён"));
  await store.load();
  expect(store.error).toBe("доступ запрещён");
  expect(store.newCount).toBeNull();
  await store.load();
  expect(store.error).toBeNull();
  expect(store.newCount).toBe(0);
  invoke.mockRejectedValueOnce(new BarsError("сбой"));
  await store.load(true);
  expect(store.error).toBeNull();
  expect(store.list).not.toBeNull();
});

it("уведомление приходит только при приросте новых находок, не после каждого чтения", async () => {
  await store.load();
  expect(flash).not.toHaveBeenCalled();
  audits = [summary("broken_links", { new_count: 2, result_run_id: 1 }), summary("duplicates")];
  await store.load(true);
  expect(flash).toHaveBeenCalledTimes(1);
  expect(flash.mock.calls[0][0]).toContain("Битые ссылки");
  await store.load(true);
  expect(flash).toHaveBeenCalledTimes(1);
});

it("раскрытие читает находки порциями по курсору и переключатель перекрытия перечитывает", async () => {
  audits = [summary("broken_links"), summary("duplicates", { state: "complete", result_run_id: 7, total: 5 })];
  await store.load();
  await store.toggle("duplicates");
  expect(store.findings.duplicates.items).toHaveLength(2);
  expect(store.findings.duplicates.next).toBe(2);
  await store.loadFindings("duplicates", false);
  await store.loadFindings("duplicates", false);
  expect(store.findings.duplicates.items).toHaveLength(5);
  expect(store.findings.duplicates.next).toBeNull();
  // По умолчанию у дублей просится группа main; с переключателем — все.
  expect(invoke).toHaveBeenCalledWith("audit_findings", expect.objectContaining({ group: "main" }));
  pages.duplicates.push(finding(9, "overlap"));
  await store.setOverlap(true);
  const last = invoke.mock.calls.filter(([method]) => method === "audit_findings").at(-1)![1] as Record<string, unknown>;
  expect(last.group).toBeUndefined();
  expect(store.findings.duplicates.total).toBe(6);
});

it("раскрытый вид перечитывается, когда у него появился новый полный результат", async () => {
  audits = [summary("broken_links", { state: "complete", result_run_id: 1 }), summary("duplicates")];
  await store.load();
  await store.toggle("broken_links");
  expect(store.findings.broken_links.runId).toBe(7);
  invoke.mockClear();
  audits = [summary("broken_links", { state: "complete", result_run_id: 8 }), summary("duplicates")];
  await store.load(true);
  await settle();
  expect(invoke).toHaveBeenCalledWith("audit_findings", expect.objectContaining({ kind: "broken_links" }));
});

it("запуск: ожидание за другим аудитом объясняется, повторные нажатия не накладываются", async () => {
  invoke.mockImplementation(async (method) => {
    if (method === "audit_run") return { accepted: true, already: false, queued: true };
    return listOf();
  });
  await store.load();
  const first = store.run("duplicates");
  const second = store.run("duplicates");
  await Promise.all([first, second]);
  expect(invoke.mock.calls.filter(([method]) => method === "audit_run")).toHaveLength(1);
  expect(flash.mock.calls[0][0]).toContain("ждёт");
});

it("ответ прежнего проекта после смены проекта отбрасывается", async () => {
  let release: (value: AuditList) => void = () => {};
  invoke.mockImplementationOnce(() => new Promise((resolve) => { release = resolve as (value: AuditList) => void; }));
  const pending = store.load();
  store.bind("C:/другой");
  release({ audits: [summary("broken_links", { new_count: 9 })], reindex_required: false, indexing: false });
  await pending;
  expect(store.list).toBeNull();
  expect(store.project).toBe("C:/другой");
});

it("расписание и остановка идут через воркер и обновляют список", async () => {
  await store.load();
  await store.setSchedule("duplicates", "weekly");
  expect(invoke).toHaveBeenCalledWith("audit_schedule", { project: P, kind: "duplicates", mode: "weekly" });
  await store.cancel("duplicates");
  expect(invoke).toHaveBeenCalledWith("audit_cancel", { project: P, kind: "duplicates" });
});
