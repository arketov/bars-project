<script lang="ts">
  import { onMount } from "svelte";
  import { mcpStatistics } from "./api";
  import { selectStatistics, totals, type McpStatistic } from "./mcp-statistics";
  let { loadStatistics = mcpStatistics }: { loadStatistics?: () => Promise<McpStatistic[]> } = $props();
  let rows = $state<McpStatistic[]>([]);
  let project = $state("*");
  let session = $state("");
  let sessionQuery = $state("");
  let error = $state("");
  let busy = $state(false);
  const projects = $derived([...new Set(rows.map(r => r.project))].sort());
  const forProject = $derived(selectStatistics(rows, project, ""));
  const sessionInfo = $derived.by(() => {
    const map = new Map<string, {first:number;last:number;calls:number;errors:number}>();
    for (const row of forProject) {
      const old = map.get(row.session);
      map.set(row.session, {first:Math.min(old?.first ?? row.first,row.first),last:Math.max(old?.last ?? row.last,row.last),calls:(old?.calls ?? 0)+row.calls,errors:(old?.errors ?? 0)+row.errors});
    }
    return map;
  });
  const sessions = $derived([...sessionInfo.keys()].filter(id => id.includes(sessionQuery.trim())).slice(0,200));
  const selected = $derived(selectStatistics(rows, project, session));
  const summary = $derived(totals(selected));
  const tools = $derived([...new Set(selected.map(r => r.tool))].sort());
  const visibleSessions = $derived(session ? [session] : sessions);
  const time = (seconds: number) => new Date(seconds * 1000).toLocaleString("ru-RU");
  function period(id: string) {
    const item = sessionInfo.get(id);
    return item ? `${time(item.first)} — ${time(item.last)}` : id;
  }
  async function load() {
    busy = true; error = "";
    try {
      rows = await loadStatistics();
      if (!rows.some(r => r.project === project)) project = "*";
      if (!selectStatistics(rows, project, "").some(r => r.session === session)) session = "";
    } catch (e) { error = String(e); rows = []; }
    finally { busy = false; }
  }
  onMount(() => { void load(); });
</script>

<h1>Статистика MCP</h1>
<p class="lead">Текущий и 29 предыдущих UTC-дней · максимум 100 000 агрегатов.</p>
<p>Сессия — соединение MCP с воркером. Переподключение создаёт новую сессию. CLI и окно не учитываются.</p>
<p>Время — выполнение на воркере, без IPC и записи статистики. Ошибки до обработчика воркера в счётчики не входят.</p>
<div class="filters">
  <label>Проект <select bind:value={project} onchange={() => { session = ""; sessionQuery = ""; }} disabled={busy}><option value="*">Все проекты</option>{#each projects as path}<option value={path}>{path || "Без проекта"}</option>{/each}</select></label>
  <label>Поиск сессии <input bind:value={sessionQuery} placeholder="Часть идентификатора" oninput={() => session = ""} disabled={busy} /></label>
  <label>Сессия <select bind:value={session} disabled={busy}><option value="">Все сессии</option>{#each sessions as id}<option value={id}>{period(id)} · {id}</option>{/each}</select></label>
  <button class="ghost" onclick={load} disabled={busy}>{busy ? "Обновление…" : "Обновить"}</button>
</div>
{#if error}
  <p role="alert">{error}</p>
{:else if !busy && selected.length === 0}
  <p role="status">Пока нет вызовов для выбранных фильтров. Здесь появятся обращения MCP после запуска этой версии воркера.</p>
{:else if selected.length}
  <section aria-label="Итоги"><b>{summary.calls} вызовов · {summary.errors} ошибок · среднее {summary.average} мс</b></section>
  <h2>Инструменты</h2>
  <table><thead><tr><th>Инструмент</th><th>Вызовы</th><th>Ошибки</th><th>Среднее, мс</th></tr></thead><tbody>
    {#each tools as tool}{@const total = totals(selected.filter(r => r.tool === tool))}<tr><td>{tool}</td><td>{total.calls}</td><td>{total.errors}</td><td>{total.average}</td></tr>{/each}
  </tbody></table>
  <h2>Сессии</h2>
  <p>Показано {visibleSessions.length} из {sessionInfo.size}. Первые 200 совпадений; остальные доступны через поиск идентификатора сессии.</p>
  {#each visibleSessions as id}<section><button class="session" onclick={() => session = id}>{period(id)}</button><small>{id}</small><p>{sessionInfo.get(id)?.calls} вызовов · {sessionInfo.get(id)?.errors} ошибок</p></section>{/each}
{/if}

<style>
  .filters { display:flex; flex-wrap:wrap; gap:12px; align-items:end; margin:24px 0; }
  label { display:flex; flex-direction:column; gap:6px; min-width:160px; max-width:100%; }
  select, input { max-width:420px; width:100%; padding:8px; background:var(--bg); color:var(--text); border:1px solid var(--line); border-radius:8px; }
  h1 { font-size:22px; margin:0 0 4px; }
  .lead { color:var(--muted); }
  button { padding:8px 12px; cursor:pointer; }
  section { padding:16px; margin:12px 0; border:1px solid var(--line); border-radius:12px; }
  table { width:100%; border-collapse:collapse; text-align:left; }
  td, th { padding:12px 8px; border-bottom:1px solid var(--line); }
  small { display:block; overflow-wrap:anywhere; margin-top:8px; }
  .session { color:var(--text); background:var(--bg); border:1px solid var(--line); border-radius:8px; }
</style>
