import { afterEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import McpStatisticsPage from "./McpStatisticsPage.svelte";
import realIpcPage from "./fixtures/mcp-history-ipc.json";
import { parameters, quantity, status, periodBounds, value, type Attempt, type HistoryFilter, type HistoryPage } from "./mcp-history";
const mounted: ReturnType<typeof mount>[] = [];
export const attempt = (id = 1, overrides: Partial<Attempt> = {}): Attempt => ({
  version: 1, id, generation: 0, epoch: "worker-1", session: "session-a", tool: "bars_rag_search",
  project: "C:/project", project_name: "Проект", started: 1800000000, finished: 1800000001, duration_ms: 120,
  outcome: "success", requested: { query: "Как работает поиск?", min_score: null, candidates: 999 },
  applied: { search: { results: 15, candidates: 30, min_score: 0.1, score_kind: "cosine" }, sources: { limit: "default", candidates: "clamped" } },
  result: { count: 0, unit: "fragments" }, error: null, incomplete: [], delivery: "unknown", ...overrides,
});
export const historyPage = (records = [attempt()], extra: Partial<HistoryPage> = {}): HistoryPage => ({
  records, next: null, enabled: true, lost_events: 0, last_storage_error: null, coverage: "Только допущенные MCP-вызовы",
  summary: { calls: records.length, success: records.length, errors: 0, unknown: 0, running: 0, total_ms: 120 * records.length, timed_calls: records.length, empty_success: records.length }, ...extra,
});
const read = vi.fn<(f: HistoryFilter) => Promise<HistoryPage>>();
const control = vi.fn<(enabled?: boolean, clear?: boolean) => Promise<void>>();
const legacy = vi.fn().mockResolvedValue([]);
const diagnostics = vi.fn().mockResolvedValue({ lost_events: 3, last_storage_error: "storage_unavailable" });
const options = vi.fn().mockResolvedValue({ projects: ["C:/distant"], sessions: ["distant-session"], projects_more: false, sessions_more: true });
async function settle() { await new Promise(r => setTimeout(r, 0)); flushSync(); }
function show() { const host = document.createElement("div"); document.body.append(host); mounted.push(mount(McpStatisticsPage, { target: host, props: { loadHistory: read, controlHistory: control, loadLegacy: legacy, loadDiagnostics: diagnostics, loadOptions: options } })); flushSync(); return host; }
const button = (h: HTMLElement, name: string) => [...h.querySelectorAll("button")].find(b => b.textContent?.trim() === name)!;
async function change(h: HTMLElement, label: string, val: string) { const el = h.querySelector<HTMLInputElement>(`[aria-label="${label}"]`)!; el.value = val; el.dispatchEvent(new Event(el.tagName === "INPUT" ? "input" : "change", { bubbles: true })); if (el.tagName === "INPUT") el.dispatchEvent(new Event("change", { bubbles: true })); flushSync(); await settle(); }
afterEach(async () => { for (const c of mounted.splice(0)) await unmount(c); document.body.replaceChildren(); vi.clearAllMocks(); read.mockReset(); control.mockReset(); });
describe("Анализ MCP", () => {
  it("реальная запись MCP/IPC: инструмент, вход, применённый путь, успех, 18 строк", async () => {
    read.mockResolvedValue(realIpcPage as HistoryPage); const h = show(); await settle();
    h.querySelector<HTMLButtonElement>(".call-row")!.click(); await settle();
    const detail = h.querySelector('[aria-label="Подробности вызова"]')!;
    expect(detail.textContent).toContain("bars_read"); expect(detail.textContent).toContain("guide.md");
    expect(detail.textContent).toContain("Документ целиком"); expect(detail.textContent).toContain("Успешно · 18 строк");
    expect(detail.textContent).toContain(realIpcPage.records[0].session);
  });
  it("подсказки ищут по всей истории, а не загруженной странице", async () => {
    read.mockResolvedValue(historyPage()); const h = show(); await settle();
    await new Promise(r => setTimeout(r, 220)); flushSync();
    expect(h.querySelector('#mcp-sessions option')?.getAttribute("value")).toBe("distant-session");
    expect(h.textContent).toContain("Первые 200 совпадений");
  });
  it("показывает четыре ответа автора без JSON, нулевой успех и причины параметров; возвращает фокус", async () => {
    read.mockResolvedValue(historyPage()); const h = show(); await settle(); h.querySelector<HTMLButtonElement>(".call-row")!.click(); await settle();
    const detail = h.querySelector('[aria-label="Подробности вызова"]')!;
    expect(detail.textContent).toContain("Как работает поиск?"); expect(detail.textContent).toContain("Пустой ответ · 0 фрагментов");
    expect(detail.textContent).toContain("Умолчание на момент вызова"); expect(detail.textContent).toContain("Ограничено"); expect(detail.textContent).toContain("null (явно)");
    expect(document.activeElement?.tagName).toBe("H2"); window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })); await settle();
    expect(h.querySelector('[aria-label="Подробности вызова"]')).toBeNull(); expect(document.activeElement?.id).toBe("mcp-call-1");
  });
  it("совмещает серверные фильтры периода/проекта/сессии/инструмента/исхода", async () => {
    read.mockResolvedValue(historyPage()); const h = show(); await settle();
    await change(h, "Период", "week"); await change(h, "Проект", "C:/other"); await change(h, "Сессия соединения", "session-b"); await change(h, "Инструмент", "bars_read"); await change(h, "Исход", "error");
    expect(read.mock.lastCall![0]).toMatchObject({ project: "C:/other", session: "session-b", tool: "bars_read", outcome: "error", limit: 50 });
    const f = read.mock.lastCall![0]; expect(f.to! - f.from!).toBe(7 * 86400);
    button(h, "Сбросить фильтры").click(); await settle(); expect(read.mock.lastCall![0].session).toBeUndefined();
  });
  it("сохраняет совместимую сессию; сбрасывает несовместимую с объяснением, включая Без проекта", async () => {
    read.mockResolvedValue(historyPage()); const h = show(); await settle(); await change(h, "Сессия соединения", "session-a"); await change(h, "Проект", "p");
    expect(read.mock.lastCall![0].session).toBe("session-a");
    read.mockImplementation(async f => f.limit === 1 ? historyPage([]) : historyPage());
    const check = h.querySelector<HTMLInputElement>('input[type="checkbox"]')!; check.click(); await settle();
    expect(read.mock.lastCall![0]).toMatchObject({ project: "" }); expect(read.mock.lastCall![0].session).toBeUndefined(); expect(h.textContent).toContain("Сессия сброшена");
  });
  it("игнорирует поздний ответ прежнего фильтра", async () => {
    let resolve!: (p: HistoryPage) => void; read.mockReturnValueOnce(new Promise(r => resolve = r)).mockResolvedValue(historyPage([attempt(2, { tool: "bars_read" })]));
    const h = show(); await change(h, "Инструмент", "bars_read"); resolve(historyPage([attempt(1)])); await settle();
    expect(h.querySelector(".call-row")?.textContent).toContain("bars_read"); expect(h.querySelectorAll(".call-row")).toHaveLength(1);
  });
  it("пагинация большого списка: курсор и сводка сервера, без полной загрузки и дублей", async () => {
    const cursor = { started: 5, id: 51, ceiling: 10000, generation: 0, from: 1, to: 100, filter_key: "scope" };
    const first = historyPage(Array.from({ length: 50 }, (_, i) => attempt(10000-i)), { next: cursor }); first.summary.calls = 10000;
    read.mockResolvedValueOnce(first).mockResolvedValueOnce({ ...first, records: [attempt(9951), attempt(9950)], next: null });
    const h = show(); await settle(); expect(read).toHaveBeenCalledTimes(1); button(h, "Ещё 50").click(); await settle();
    expect(read.mock.lastCall![0].cursor).toEqual(cursor); expect(h.querySelectorAll(".call-row")).toHaveLength(51); expect(h.textContent).toContain("Показано 51 из 10000");
  });
  it("обновляет выбранный ID вне первой страницы и сообщает вытеснение", async () => {
    read.mockResolvedValueOnce(historyPage()); const h = show(); await settle(); h.querySelector<HTMLButtonElement>(".call-row")!.click(); await settle();
    read.mockResolvedValueOnce(historyPage([attempt(2)])).mockResolvedValueOnce(historyPage([attempt(1, { duration_ms: 999 })])); button(h, "Обновить").click(); await settle();
    expect(read.mock.lastCall![0].id).toBe(1); expect(h.querySelector('[aria-label="Подробности вызова"]')?.textContent).toContain("999 мс");
    read.mockResolvedValueOnce(historyPage([attempt(2)])).mockResolvedValueOnce(historyPage([])); button(h, "Обновить").click(); await settle(); expect(h.textContent).toContain("Выбранная запись удалена");
  });
  it("закрытие записи вне первой страницы возвращает фокус к списку", async () => {
    read.mockResolvedValueOnce(historyPage()); const h = show(); await settle(); h.querySelector<HTMLButtonElement>(".call-row")!.click(); await settle();
    read.mockResolvedValueOnce(historyPage([attempt(2)])).mockResolvedValueOnce(historyPage([attempt(1)])); button(h, "Обновить").click(); await settle();
    button(h, "Закрыть подробности").click(); await settle(); expect(document.activeElement?.id).toBe("mcp-list-title");
  });
  it("ошибка чтения отличается от пустоты; диагностика и успешный повтор", async () => {
    read.mockRejectedValueOnce("SQLite").mockResolvedValueOnce(historyPage([])); const h = show(); await settle();
    expect(h.querySelector('[role="alert"]')?.textContent).toContain("не прочитана"); expect(h.textContent).toContain("потеряно событий текущего процесса — 3"); expect(h.querySelector(".empty")).toBeNull();
    button(h, "Повторить").click(); await settle(); expect(h.querySelector('[role="alert"]')).toBeNull(); expect(h.querySelector(".empty")).not.toBeNull();
  });
  it("отдельные исходы ошибки, неизвестности и очереди", async () => {
    read.mockResolvedValue(historyPage([attempt(1, { outcome: "error", result: { count: null, unit: "unknown" }, error: { code: 1, kind: "not_found", message: "Файл не найден", stage: "worker" } }), attempt(2, { outcome: "unknown", duration_ms: null, finished: null, result: null, incomplete: ["interrupted"] }), attempt(3, { result: { count: null, unit: "not_applicable", queued: true } })]));
    const h = show(); await settle(); expect(h.textContent).toContain("Исход неизвестен"); expect(h.textContent).toContain("В очереди"); expect(h.textContent).toContain("Количество не применяется");
    h.querySelector<HTMLButtonElement>(".call-row")!.click(); await settle(); expect(h.textContent).toContain("Файл не найден");
  });
  it("очистка только после подтверждения; отказ не выдаётся за успех; переключение подтверждается сервером", async () => {
    read.mockResolvedValue(historyPage()); control.mockRejectedValueOnce("Отказ").mockResolvedValue(undefined); const h = show(); await settle();
    button(h, "Очистить всю историю").click(); await settle(); expect(control).not.toHaveBeenCalled(); expect(document.activeElement?.textContent).toBe("Отмена");
    button(h, "Удалить всю историю").click(); await settle(); expect(control).toHaveBeenCalledWith(undefined, true); expect(h.textContent).toContain("не сохранено");
    button(h, "Удалить всю историю").click(); await settle(); expect(h.querySelector('[aria-label="Подтверждение очистки"]')).toBeNull();
    const enabled = h.querySelector<HTMLInputElement>(".policy input")!; enabled.click(); await settle(); expect(control).toHaveBeenLastCalledWith(false, false); expect(enabled.checked).toBe(true);
  });
  it("прежние агрегаты отдельно от новой сводки", async () => {
    read.mockResolvedValue(historyPage()); legacy.mockResolvedValueOnce([{ session: "old", project: "p", tool: "bars_read", calls: 999, errors: 1, total_ms: 200, first: 1, last: 2 }]);
    const h = show(); await settle(); button(h, "Загрузить прежние агрегаты").click(); await settle(); expect(h.querySelector(".legacy")?.textContent).toContain("999 вызовов"); expect(h.querySelector(".summary")?.textContent).not.toContain("999");
  });
});
describe("Формат подробностей", () => {
  it("missing, null, пустое, неизвестное, неприменимое различимы", () => {
    expect(new Set([value(undefined), value(null), value(""), value([])]) .size).toBe(4);
    expect(quantity(attempt(1, { result: { count: null, unit: "unknown" } }))).not.toContain("0");
    expect(quantity(attempt(1, { result: { count: null, unit: "not_applicable" } }))).toContain("не применяется");
    expect(status(attempt())).toBe("Пустой ответ"); expect(parameters(attempt()).find(p => p.key === "limit")?.requested).toBe("Не передано");
  });
  it("Сегодня начинается в местную полночь", () => {
    const date = new Date(2026, 9, 18, 15, 30); expect(periodBounds("today", date).from).toBe(new Date(2026, 9, 18).getTime()/1000);
  });
});
