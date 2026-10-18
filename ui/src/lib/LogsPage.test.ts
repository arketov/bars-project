import { afterEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import LogsPage from "./LogsPage.svelte";
import { workerLogs, type LogSnapshot } from "./api";

vi.mock("./api", () => ({ workerLogs: vi.fn() }));
const read = vi.mocked(workerLogs);
const components: ReturnType<typeof mount>[] = [];
const empty = (): LogSnapshot => ({ directory: "test/logs", entries: [], skipped: 0, total: 0, write_error: false });
const rows = (): LogSnapshot => ({ ...empty(), total: 1, entries: [{ time: "2026-10-18T12:00:00Z", pid: 7, level: "warn", target: "bars_worker", message: "<script>Не исполнять</script>" }] });
async function settle() { await new Promise(resolve => setTimeout(resolve, 0)); flushSync(); }
function show() { const host = document.createElement("div"); document.body.append(host); components.push(mount(LogsPage, { target: host })); flushSync(); return host; }
afterEach(() => { components.splice(0).forEach(c => void unmount(c)); document.body.replaceChildren(); vi.resetAllMocks(); });

describe("Логи", () => {
  it("loads default level and renders message as text", async () => {
    read.mockResolvedValue(rows());
    const host = show(); await settle();
    expect(read).toHaveBeenCalledWith("info");
    expect(host.textContent).toContain("Предупреждение");
    expect(host.textContent).toContain("<script>Не исполнять</script>");
    expect(host.querySelector("script")).toBeNull();
  });

  it("distinguishes empty journal and filter with no matches", async () => {
    read.mockResolvedValueOnce(empty()).mockResolvedValueOnce({ ...empty(), total: 3 });
    const host = show(); await settle();
    expect(host.textContent).toContain("Журнал пока пуст");
    const select = host.querySelector("select")!;
    select.value = "error"; select.dispatchEvent(new Event("change", { bubbles: true })); await settle();
    expect(read).toHaveBeenLastCalledWith("error");
    expect(host.textContent).toContain("Нет записей выбранного уровня");
  });

  it("keeps stale rows on refresh error and reports write/corruption warnings", async () => {
    read.mockResolvedValueOnce({ ...rows(), write_error: true, skipped: 2 }).mockRejectedValueOnce(new Error("Нет доступа"));
    const host = show(); await settle();
    expect(host.textContent).toContain("Последняя запись журнала не удалась");
    expect(host.textContent).toContain("Пропущено повреждённых записей: 2");
    host.querySelector("button")!.click(); await settle();
    expect(host.textContent).toContain("Нет доступа Показаны прежние записи");
    expect(host.querySelectorAll("li")).toHaveLength(1);
  });

  it("ignores late response from a previous filter", async () => {
    let finish!: (value: LogSnapshot) => void;
    read.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; })).mockResolvedValueOnce({ ...empty(), total: 3 });
    const host = show();
    expect(host.textContent).toContain("Чтение журнала");
    const select = host.querySelector("select")!;
    select.value = "error"; select.dispatchEvent(new Event("change", { bubbles: true })); await settle();
    finish(rows()); await settle();
    expect(host.querySelectorAll("li")).toHaveLength(0);
    expect(host.textContent).toContain("Нет записей выбранного уровня");
  });
});
