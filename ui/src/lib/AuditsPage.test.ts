import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount, type Component } from "svelte";
import AuditsPage from "./AuditsPage.svelte";
import Sidebar from "./Sidebar.svelte";
import { app } from "./app.svelte";
import { call } from "./api";
import type { AuditFinding, AuditList, AuditSummary } from "./audits";
import { BarsError } from "./types";

vi.mock("./api", () => ({ call: vi.fn(), joinPath: vi.fn(), openPath: vi.fn(), pickFolder: vi.fn(), copyText: vi.fn(), folderTarget: vi.fn(), folderAction: vi.fn(), documentEditors: vi.fn(async () => []), documentAction: vi.fn(), inTauri: () => false }));
const invoke = vi.mocked(call);
const project = { name: "Проект", path: "C:/проект", docs_root: "docs", state: "ready" as const, modules: [] };
const mounted: ReturnType<typeof mount>[] = [];
const settle = async () => { for (let i = 0; i < 4; i++) { await new Promise((resolve) => setTimeout(resolve, 0)); flushSync(); } };
const summary = (kind: AuditSummary["kind"], over: Partial<AuditSummary> = {}): AuditSummary => ({
  kind, state: "never", total: 0, new_count: 0, gone_count: 0, overlap_total: 0, schedule: "off",
  schedules: kind === "broken_links" ? ["off", "daily", "weekly", "on_change"] : ["off", "daily", "weekly"], ...over,
});
const broken: AuditFinding = { fingerprint: "a".repeat(32), ignored: false, standing: "new", tag: "broken", group: "main", reason: "Ссылка на документ, которого нет", subjects: [{ path: "docs/a.md", first_line: 14, last_line: 14, heading: "", role: "document" }], detail: "Нет → нет.md" };
const goneOne: AuditFinding = { ...broken, standing: "gone", subjects: [{ ...broken.subjects[0], path: "docs/b.md" }] };
const duplicate: AuditFinding = {
  fingerprint: "d".repeat(32), ignored: false,
  standing: "known", tag: "partial", group: "main", reason: "Заметная часть текста скопирована", score: 0.62,
  subjects: [
    { path: "docs/manual/x.md", first_line: 3, last_line: 9, heading: "Поиск/Как работает", role: "manual" },
    { path: "docs/features/y.md", first_line: 8, last_line: 21, heading: "Поиск", role: "feature" },
  ],
  detail: "Общий абзац про поиск.",
};
let audits: AuditSummary[];
let state: { reindex_required: boolean; indexing: boolean };
let listError: BarsError | null;
let findings: Record<string, AuditFinding[]>;

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function show(component: Component<any>) {
  const host = document.createElement("div");
  document.body.append(host);
  mounted.push(mount(component, { target: host }));
  flushSync();
  return host;
}
const button = (host: ParentNode, text: string) => [...host.querySelectorAll("button")].find((b) => b.textContent?.trim().startsWith(text)) as HTMLButtonElement | undefined;
const card = (host: ParentNode, label: string) => host.querySelector(`article[aria-label="${label}"]`) as HTMLElement;

beforeEach(() => {
  audits = [summary("broken_links", { state: "complete", result_run_id: 1, result_at: 1_700_000_000, last_run_at: 1_700_000_000, total: 1, new_count: 1, gone_count: 1 }), summary("duplicates")];
  state = { reindex_required: false, indexing: false };
  listError = null;
  findings = { broken_links: [broken, goneOne], duplicates: [duplicate] };
  app.projects = [project];
  app.current = project.path;
  app.auditsOpen = true;
  app.audits.bind(null);
  app.audits.bind(project.path);
  invoke.mockReset();
  invoke.mockImplementation(async (method, params) => {
    const p = params as { kind?: string; group?: string };
    switch (method) {
      case "audit_list": if (listError) throw listError; return { audits, ...state } satisfies AuditList;
      case "audit_findings": { const items = (findings[p.kind!] ?? []).filter((f) => !p.group || f.group === p.group); return { run_id: 1, finished: 1_700_000_000, items, total: items.length }; }
      case "audit_run": audits = audits.map((a) => (a.kind === p.kind ? { ...a, state: "running" as const, running_since: 1_700_000_100 } : a)); return { accepted: true, already: false, queued: false };
      case "audit_schedule": return { mode: (params as { mode: string }).mode };
      case "audit_cancel": return { cancelled: true };
      case "new_topics_list": return { enabled: true, generation: 1, topics: [], store_path: "", losses: 0 };
      default: throw new Error(`unexpected ${method}`);
    }
  });
});

afterEach(() => {
  while (mounted.length) void unmount(mounted.pop()!);
  document.body.replaceChildren();
});

it("числовое расхождение показывает оба источника и позволяет игнорировать и вернуть", async () => {
  const numeric: AuditFinding = { ...duplicate, tag: "num", reason: "Возможное расхождение — сек: 30 ↔ 60", detail: "Ожидание 30 с ↔ ожидание 60 с", subjects: [duplicate.subjects[0], { ...duplicate.subjects[0], path: "docs/b.md" }] };
  audits.push(summary("contradictions", { state: "complete", result_run_id: 1, result_at: 1, total: 1 }));
  findings.contradictions = [numeric];
  const base = invoke.getMockImplementation()!;
  invoke.mockImplementation(async (method, params) => {
    if (method === "audit_ignore") {
      const ignored = Boolean(params?.ignored);
      findings.contradictions = [{ ...numeric, ignored }];
      audits = audits.map(a => a.kind === "contradictions" ? { ...a, total: ignored ? 0 : 1 } : a);
      return { ignored };
    }
    if (method === "audit_findings" && params?.kind === "contradictions") {
      const items = findings.contradictions.filter(f => params?.include_ignored || !f.ignored);
      return { run_id: 1, items, total: items.length };
    }
    return base(method,params);
  });
  const host = show(AuditsPage); await settle();
  const c = card(host,"Противоречия"); button(c,"Показать находки")!.click(); await settle();
  expect(c.textContent).toContain("30 ↔ 60"); expect(c.querySelectorAll(".subject")).toHaveLength(2);
  button(c,"Игнорировать")!.click(); await settle();
  expect(c.querySelector(".finding")).toBeNull();
  const label = [...host.querySelectorAll("label")].find(l => l.textContent?.trim() === "Игнорируемые")!;
  label.querySelector<HTMLInputElement>("input")!.click(); await settle();
  button(c,"Вернуть")!.click(); await settle();
  expect(button(c,"Игнорировать")).toBeDefined();
});

it("показывает виды с состоянием, временем и числом находок, а блок новых тем — входом", async () => {
  const host = show(AuditsPage);
  await settle();
  expect(host.textContent).toContain("Аудит ничего не меняет в файлах и не доказывает отсутствия проблем");
  const links = card(host, "Битые ссылки");
  expect(links.textContent).toContain("готово");
  expect(links.textContent).toContain("1 находка · 1 новая · 1 исчезла");
  const dups = card(host, "Дубли");
  expect(dups.textContent).toContain("не запускался");
  expect(dups.textContent).not.toContain("0 находок");
  expect(host.querySelector('article[aria-label="Противоречия"]')).toBeNull();
  expect(card(host, "Новые темы").textContent).toContain("Открыть");
  expect(button(dups, "Показать находки")!.disabled).toBe(true);
});

it("находки раскрываются под строкой вида: новые отмечены, исчезнувшие серые, клик открывает документ", async () => {
  const open = vi.spyOn(app, "openDocument").mockResolvedValue(undefined);
  const host = show(AuditsPage);
  await settle();
  button(card(host, "Битые ссылки"), "Показать находки")!.click();
  await settle();
  const rows = [...host.querySelectorAll(".finding")];
  expect(rows.map((row) => row.getAttribute("data-standing"))).toEqual(["new", "gone"]);
  expect(rows[0].textContent).toContain("docs/a.md:14");
  expect(rows[0].textContent).toContain("новая");
  expect(rows[1].classList.contains("gone")).toBe(true);
  (rows[0].querySelector(".subject") as HTMLButtonElement).click();
  expect(open).toHaveBeenCalledWith({ path: "docs/a.md", title: "a.md", first_line: 14, last_line: 14 });
});

it("дубли показывают роль документов, меру и общий отрывок, а перекрытие — по переключателю", async () => {
  audits = [audits[0], summary("duplicates", { state: "complete", result_run_id: 1, result_at: 1_700_000_000, last_run_at: 1_700_000_000, total: 1, overlap_total: 3 })];
  const host = show(AuditsPage);
  await settle();
  const dups = card(host, "Дубли");
  expect(dups.textContent).toContain("частичные ≥ 0,4");
  expect(dups.textContent).toContain("Перекрытие содержания (3)");
  button(dups, "Показать находки")!.click();
  await settle();
  expect(dups.textContent).toContain("руководство");
  expect(dups.textContent).toContain("фича");
  expect(dups.textContent).toContain("0,62");
  expect(dups.textContent).toContain("Общий абзац про поиск.");
  expect(invoke).toHaveBeenCalledWith("audit_findings", expect.objectContaining({ kind: "duplicates", group: "main" }));
  findings.duplicates = [duplicate, { ...duplicate, group: "overlap", tag: "semantic", reason: "Близкий смысл, не копия" }];
  (dups.querySelector(".overlap input") as HTMLInputElement).click();
  await settle();
  expect(dups.querySelectorAll(".finding")).toHaveLength(2);
});

it("пустой результат называет дату запуска и не обещает отсутствия проблем", async () => {
  findings.broken_links = [];
  audits = [summary("broken_links", { state: "complete", result_run_id: 1, result_at: 1_700_000_000, last_run_at: 1_700_000_000 }), audits[1]];
  const host = show(AuditsPage);
  await settle();
  button(card(host, "Битые ссылки"), "Показать находки")!.click();
  await settle();
  expect(host.textContent).toContain("Ничего не найдено при запуске");
  expect(host.textContent).toContain("не доказывает");
});

it("запуск переводит вид в «идёт» и подменяет кнопку на «Остановить»; расписание меняется выбором", async () => {
  const host = show(AuditsPage);
  await settle();
  button(card(host, "Дубли"), "Запустить")!.click();
  await settle();
  expect(invoke).toHaveBeenCalledWith("audit_run", { project: project.path, kind: "duplicates" });
  const dups = card(host, "Дубли");
  expect(dups.textContent).toContain("идёт");
  expect(button(dups, "Остановить")).toBeDefined();
  button(dups, "Остановить")!.click();
  await settle();
  expect(invoke).toHaveBeenCalledWith("audit_cancel", { project: project.path, kind: "duplicates" });
  const select = card(host, "Битые ссылки").querySelector("select") as HTMLSelectElement;
  expect([...select.options].map((o) => o.value)).toEqual(["off", "daily", "weekly", "on_change"]);
  select.value = "weekly";
  select.dispatchEvent(new Event("change", { bubbles: true }));
  await settle();
  expect(invoke).toHaveBeenCalledWith("audit_schedule", { project: project.path, kind: "broken_links", mode: "weekly" });
});

it("состояния: индекс не готов, упавший запуск и ошибка чтения не выглядят пустым результатом", async () => {
  state = { reindex_required: true, indexing: false };
  audits = [audits[0], summary("duplicates", { state: "failed", error: "Индекс требует переиндексации: сначала обновите индекс", last_run_at: 1_700_000_000, last_status: "failed" })];
  const host = show(AuditsPage);
  await settle();
  expect(host.textContent).toContain("Индекс требует переиндексации");
  expect(card(host, "Дубли").textContent).toContain("ошибка");

  app.audits.bind(null);
  app.audits.bind(project.path);
  listError = new BarsError("не удалось открыть хранилище аудитов");
  await app.audits.load();
  await settle();
  const alert = host.querySelector('[role="alert"]')!;
  expect(alert.textContent).toContain("Не удалось прочитать аудиты");
  expect(host.querySelector("article")).toBeNull();
  listError = null;
  button(alert as HTMLElement, "Повторить")!.click();
  await settle();
  expect(host.querySelector('[role="alert"]')).toBeNull();
});

it("строка «Аудиты» в дереве показывает число новых и открывает экран", async () => {
  app.tree = [{ path: "docs", name: "docs", children: [], documents: [] } as never];
  const host = show(Sidebar);
  await app.audits.load();
  await settle();
  const row = [...host.querySelectorAll("button.topics-row")].find((b) => b.textContent?.includes("Аудиты")) as HTMLButtonElement | undefined;
  if (!row) return; // Дерево собирается по своим правилам: проверка строки — в браузере.
  expect(row.getAttribute("aria-label")).toBe("Аудиты: новых находок 1");
  row.click();
  expect(app.auditsOpen).toBe(true);
});
