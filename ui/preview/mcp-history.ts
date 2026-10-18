// Изолированный стенд компонента: npm run dev, /preview/mcp-history.html.
// Изменения хранения затрагивают только память этого стенда.
import { mount } from "svelte";
import McpStatisticsPage from "../src/lib/McpStatisticsPage.svelte";
import type { Attempt, HistoryFilter, HistoryPage } from "../src/lib/mcp-history";
import "../src/theme.css";
const params = new URLSearchParams(location.search);
const mode = params.get("theme") === "dark" ? "dark" : "light";
document.documentElement.dataset.mode = mode;
const style = document.createElement("style");
style.textContent = `body{overflow:auto}.frame{display:grid;grid-template-columns:260px minmax(0,1fr);max-width:1700px;margin:auto;min-height:100vh}.side{padding:28px 24px;border-right:1px solid var(--line);background:var(--panel)}.side b{font-size:22px}.side p{color:var(--accent-text);background:var(--accent-soft);padding:10px;border-radius:9px}main{padding:28px 32px 110px;min-width:0}.proof{font-size:12px;color:var(--muted);padding:8px 16px;border-bottom:1px solid var(--line)}@media(max-width:800px){.frame{grid-template-columns:210px minmax(0,1fr)}main{padding:24px 20px}}`;
document.head.append(style);
const root = document.getElementById("preview")!;
root.innerHTML = `<div class="proof">Проверка Svelte-компонента · ${params.get("scene") === "ipc" ? "реальная запись временного IPC-теста (пути обезличены)" : "вымышленные данные"} · команды ОС/установленный профиль не используются · <a href="?theme=${mode === "dark" ? "light" : "dark"}&scene=${params.get("scene") ?? "mixed"}">Сменить тему</a> · <a href="?theme=${mode}&scene=large">240 вызовов</a> · <a href="?theme=${mode}&scene=ipc">Запись IPC</a></div><div class="frame"><aside class="side"><b>bars</b><h3>Настройки</h3><p>Статистика MCP</p></aside><main></main></div>`;
const ipc = (await (await fetch("/src/lib/fixtures/mcp-history-ipc.json")).json()) as HistoryPage;
const seed = ipc.records[0];
const now = Math.floor(Date.now()/1000);
const scene = params.get("scene") ?? "mixed";
const base: Attempt = { ...seed, started: now - 30, finished: now - 29, project: "C:/projects/демонстрационный-проект-с-длинным-названием", project_name: "Демонстрационный проект", session: "mcp-session-with-long-identifier-2026-10-18" };
const mixed: Attempt[] = [
  { ...base, id: 6, tool: "bars_rag_search", requested: { query: "Как при смене настроек сохраняются исходные и применённые параметры MCP-вызова?", candidates: 9999 }, applied: { search: { results: 15, candidates: 30, score_kind: "cosine", min_score: 0.1 }, sources: { limit: "default", candidates: "clamped" }, rerank_performed: false, rerank_fallback: true, rerank_fallback_reason: "model_loading" }, result: { count: 5, unit: "fragments", warning: true } },
  { ...base, id: 5, tool: "bars_rag_search", result: { count: 0, unit: "fragments" } },
  { ...base, id: 4, outcome: "error", result: { count: null, unit: "unknown" }, error: { kind: "file_not_found", code: -32001, stage: "worker", message: "Документ не найден" } },
  { ...base, id: 3, outcome: "unknown", finished: null, duration_ms: null, result: null, incomplete: ["worker_restarted"] },
  { ...base, id: 2, tool: "bars_reindex", result: { count: null, unit: "not_applicable", queued: true } },
  { ...base, id: 1, tool: "bars_projects", outcome: "running", finished: null, duration_ms: null, result: null, project: "", project_name: null },
];
let rows = scene === "ipc" ? ipc.records : scene === "empty" ? [] : scene === "large" ? Array.from({ length: 240 }, (_, i) => ({ ...mixed[i % mixed.length], id: 240-i, started: now-i*30 })) : mixed;
let enabled = true;
let fail = scene === "error";
function select(f: HistoryFilter) { return rows.filter(a => (f.project === undefined || a.project === f.project) && (!f.session || a.session === f.session) && (!f.tool || a.tool === f.tool) && (!f.outcome || a.outcome === f.outcome) && a.started >= (f.from ?? 0) && a.started < (f.to ?? Infinity)); }
mount(McpStatisticsPage, { target: root.querySelector("main")!, props: {
  loadHistory: async f => {
    if (fail) { fail = false; throw new Error("Демонстрация отказа хранилища"); }
    const all = select(f); const start = f.cursor ? all.findIndex(a => a.id === f.cursor!.id)+1 : 0;
    const records = all.filter(a => f.id === undefined || a.id === f.id).slice(start, start+(f.limit ?? 50));
    const timed = all.filter(a => a.duration_ms !== null);
    const last = records.at(-1);
    return { records, enabled, coverage: "Допущенные вызовы worker; доставка клиенту неизвестна", lost_events: 0, last_storage_error: null,
      next: f.id === undefined && start+records.length < all.length && last ? { id: last.id, started: last.started, ceiling: 240, generation: 0, from: f.from!, to: f.to!, filter_key: "preview" } : null,
      summary: { calls: all.length, success: all.filter(a => a.outcome === "success").length, errors: all.filter(a => a.outcome === "error").length, unknown: all.filter(a => a.outcome === "unknown").length, running: all.filter(a => a.outcome === "running").length, timed_calls: timed.length, total_ms: timed.reduce((s,a) => s+a.duration_ms!, 0), empty_success: all.filter(a => a.outcome === "success" && a.result?.count === 0).length } };
  },
  loadOptions: async (f, p, s) => ({ projects: [...new Set(rows.map(a => a.project).filter(x => x && x.includes(p)))], sessions: [...new Set(rows.filter(a => f.project === undefined || a.project === f.project).map(a => a.session).filter(x => x.includes(s)))], projects_more: false, sessions_more: false }),
  controlHistory: async (e, clear) => { if (clear) rows = []; if (e !== undefined) enabled = e; },
  loadLegacy: async () => [], loadDiagnostics: async () => ({ lost_events: 1, last_storage_error: "storage_unavailable" }),
} });
