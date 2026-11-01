<script lang="ts">
  import { onMount, tick } from "svelte";
  import { commandAdapter } from './command-adapters';
  import { mcpHistory, mcpHistoryControl, mcpHistoryLegacy, mcpHistoryDiagnostics, mcpHistoryOptions } from "./api";
  import { duration, incompleteness, localTime, outcomes, parameters, periodBounds, quantity, status, value, type Attempt, type HistoryFilter, type HistoryPage, type HistoryDiagnostics } from "./mcp-history";
  import type { McpStatistic } from "./mcp-statistics";
  let { loadHistory = mcpHistory, controlHistory = mcpHistoryControl, loadLegacy = mcpHistoryLegacy, loadDiagnostics = mcpHistoryDiagnostics, loadOptions = mcpHistoryOptions }: {
    loadHistory?: typeof mcpHistory; controlHistory?: typeof mcpHistoryControl;
    loadLegacy?: typeof mcpHistoryLegacy; loadDiagnostics?: typeof mcpHistoryDiagnostics;
    loadOptions?: typeof mcpHistoryOptions;
  } = $props();
  let period = $state("day"), project = $state(""), noProject = $state(false), session = $state(""), tool = $state(""), outcome = $state("");
  let page = $state<HistoryPage | null>(null), selected = $state<Attempt | null>(null);
  let filter = $state<HistoryFilter>({});
  let busy = $state(false), managing = $state(false), error = $state(""), note = $state(""), updated = $state<number | null>(null);
  let diagnostics = $state<HistoryDiagnostics | null>(null), diagnosticsError = $state("");
  let legacy = $state<McpStatistic[] | null>(null), legacyError = $state(""), legacyBusy = $state(false);
  let confirmClear = $state(false);
  let detailsHeading = $state<HTMLHeadingElement>(), clearButton = $state<HTMLButtonElement>(), cancelButton = $state<HTMLButtonElement>();
  let generation = 0, selectionVersion = 0;
  let projectHints = $state<string[]>([]), sessionHints = $state<string[]>([]);
  let optionsNote = $state("");
  let optionsVersion = 0, optionsTimer: ReturnType<typeof setTimeout>;
  const tools = ["bars_rag_search", "bars_read", "bars_outline", "bars_get_similar", "bars_projects", "bars_status", "bars_reindex", "bars_relations", "bars_paths", "bars_impact", "bars_context", "bars_explore", "bars_graph"];
  const activeFilters = $derived(period !== "day" || project !== "" || noProject || session !== "" || tool !== "" || outcome !== "");
  const diag = $derived(page ?? diagnostics);
  function hints(records: Attempt[]) {
    projectHints = [...new Set([...projectHints, ...records.map(a => a.project).filter(Boolean)])].slice(-200);
    sessionHints = [...new Set([...sessionHints, ...records.map(a => a.session)])].slice(-200);
  }
  function suggest() {
    clearTimeout(optionsTimer); const version = ++optionsVersion;
    const f = { ...periodBounds(period), ...(noProject ? { project: "" } : project ? { project } : {}) };
    const p = project, s = session;
    optionsTimer = setTimeout(async () => {
      try {
        const options = await loadOptions(f, p, s);
        if (version !== optionsVersion) return;
        projectHints = options.projects; sessionHints = options.sessions;
        optionsNote = options.projects_more || options.sessions_more ? "Первые 200 совпадений. Уточните часть пути или ID." : "Подсказки по всей истории периода; поиск по части пути или ID.";
      } catch { if (version === optionsVersion) optionsNote = "Подсказки недоступны; точный путь и ID можно ввести вручную."; }
    }, 180);
  }
  // Ответ привязан к поколению фильтра: позднее чтение не возвращает чужую запись.
  async function load(more = false, checkSession = false, resetSelection = false) {
    const version = ++generation;
    optionsVersion++; clearTimeout(optionsTimer);
    const oldSelection = resetSelection ? null : selected;
    const oldSelectionVersion = selectionVersion;
    if (resetSelection) selected = null;
    error = ""; note = ""; busy = true;
    const nextFilter: HistoryFilter = more ? { ...filter, cursor: page?.next ?? undefined } : {
      ...periodBounds(period), ...(noProject ? { project: "" } : project ? { project } : {}),
      ...(session ? { session } : {}), ...(tool ? { tool } : {}), ...(outcome ? { outcome } : {}), limit: 50,
    };
    if (!more) { page = null; updated = null; selected = null; filter = nextFilter; }
    try {
      if (checkSession && nextFilter.session) {
        const compatible = await loadHistory({ from: nextFilter.from, to: nextFilter.to, project: nextFilter.project, session: nextFilter.session, limit: 1 });
        if (version !== generation) return;
        if (!compatible.summary.calls) {
          session = ""; delete nextFilter.session;
          note = "Сессия сброшена: для выбранного проекта и периода вызовов этой сессии нет.";
        }
      }
      const result = await loadHistory(nextFilter);
      if (version !== generation) return;
      if (more && page) {
        const ids = new Set(page.records.map(a => a.id));
        page = { ...result, records: [...page.records, ...result.records.filter(a => !ids.has(a.id))] };
      } else page = result;
      hints(result.records); suggest(); updated = Math.floor(Date.now() / 1000);
      if (!more && oldSelection && oldSelectionVersion === selectionVersion) {
        const known = result.records.find(a => a.id === oldSelection.id);
        const record = known ?? (await loadHistory({ ...nextFilter, id: oldSelection.id, limit: 1 })).records[0];
        if (version !== generation || oldSelectionVersion !== selectionVersion) return;
        selected = record ?? null;
        if (!record) note = "Выбранная запись удалена, устарела или больше не соответствует фильтрам.";
      }
    } catch (e) {
      if (version !== generation) return;
      error = `История MCP не прочитана: ${String(e)}`;
      if (!more) page = null;
      try { const d = await loadDiagnostics(); if (version === generation) { diagnostics = d; diagnosticsError = ""; } }
      catch { if (version === generation) diagnosticsError = "Диагностика хранения также недоступна."; }
    } finally { if (version === generation) busy = false; }
  }
  function change(checkSession = false) { selectionVersion++; void load(false, checkSession, true); }
  function reset() { period = "day"; project = ""; noProject = false; session = ""; tool = ""; outcome = ""; change(); }
  async function open(a: Attempt) { selectionVersion++; selected = a; await tick(); detailsHeading?.focus(); }
  async function close() {
    const id = selected?.id; selectionVersion++; selected = null; await tick();
    (document.getElementById(`mcp-call-${id}`) ?? document.getElementById("mcp-list-title"))?.focus();
  }
  async function manage(enabled?: boolean, clear = false) {
    managing = true; error = "";
    optionsVersion++; clearTimeout(optionsTimer);
    ++generation; busy = false; selectionVersion++; selected = null;
    try {
      await controlHistory(enabled, clear);
      if (clear) { confirmClear = false; legacy = null; projectHints = []; sessionHints = []; }
      await load(false, false, true);
      await tick(); if (clear) clearButton?.focus();
    } catch (e) { error = `Изменение истории не сохранено: ${String(e)}`; }
    finally { managing = false; }
  }
  async function readLegacy() {
    legacyBusy = true; legacyError = "";
    const version = generation;
    try { const rows = await loadLegacy(); if (version === generation) legacy = rows; }
    catch (e) { if (version === generation) legacyError = String(e); }
    finally { legacyBusy = false; }
  }
  async function askClear() { confirmClear = true; await tick(); cancelButton?.focus(); }
  async function cancelClear() { confirmClear = false; await tick(); clearButton?.focus(); }
  onMount(() => { void load(); return () => { generation++; selectionVersion++; optionsVersion++; clearTimeout(optionsTimer); }; });
</script>

<svelte:window onkeydown={e => { if (!e.defaultPrevented && !document.querySelector('dialog[open], [role="menu"]') && (!document.querySelector('.workspace') || e.target instanceof HTMLElement && e.target.closest('.mcp-page')) && e.key === "Escape") { if (confirmClear && !managing) { e.preventDefault(); void cancelClear(); } else if (selected) { e.preventDefault(); void close(); } } }} />
<div class="mcp-page" data-command-blocked={confirmClear || undefined} use:commandAdapter={{ 'mcp.clear': { run: askClear, reason: () => managing ? 'Операция выполняется' : undefined } }}>
  <header><div><h1>Статистика MCP</h1><p class="muted">Отдельные вызовы: что запрошено, что применено и чем закончилось.</p></div><button class="ghost" onclick={() => load()} disabled={busy || managing}>Обновить</button></header>
  <p class="muted small">{updated ? `Обновлено: ${localTime(updated)}` : busy ? "Загрузка истории…" : "Свежие данные не получены"}. Даты и время — местные ({Intl.DateTimeFormat().resolvedOptions().timeZone}).</p>
  <div class="filters">
    <label>Период<select aria-label="Период" bind:value={period} onchange={() => change()} disabled={managing}><option value="day">Последние 24 часа</option><option value="today">Сегодня</option><option value="week">Последние 7 дней</option></select></label>
    <label>Проект<input aria-label="Проект" list="mcp-projects" placeholder="Все проекты / точный путь" bind:value={project} oninput={e => { project = e.currentTarget.value; suggest(); }} onchange={() => change(true)} disabled={noProject || managing} /></label>
    <label>Сессия соединения<input aria-label="Сессия соединения" list="mcp-sessions" placeholder="Все / точный ID" bind:value={session} oninput={e => { session = e.currentTarget.value; suggest(); }} onchange={() => change()} disabled={managing} /></label>
    <label>Инструмент<select aria-label="Инструмент" bind:value={tool} onchange={() => change()} disabled={managing}><option value="">Все инструменты</option>{#each tools as name}<option value={name}>{name}</option>{/each}</select></label>
    <label>Исход<select aria-label="Исход" bind:value={outcome} onchange={() => change()} disabled={managing}><option value="">Все исходы</option>{#each Object.entries(outcomes) as [key, label]}<option value={key}>{label}</option>{/each}</select></label>
  </div>
  <datalist id="mcp-projects">{#each projectHints as path}<option value={path}></option>{/each}</datalist><datalist id="mcp-sessions">{#each sessionHints as id}<option value={id}></option>{/each}</datalist>
  <div class="filter-extra"><label class="inline"><input type="checkbox" bind:checked={noProject} onchange={() => change(true)} disabled={managing} /> Без проекта</label><button class="ghost" onclick={reset} disabled={managing}>Сбросить фильтры</button><span class="muted small">Фильтр — точный путь или ID из подсказки. {optionsNote}</span></div>
  {#if note}<p role="status">{note}</p>{/if}
  {#if error}<div class="error" role="alert">{error} <button class="ghost" onclick={() => load()} disabled={busy || managing}>Повторить</button></div>{/if}
  {#if busy}<p role="status">{page ? "Загрузка следующей страницы…" : "Загрузка истории…"}</p>{/if}
  {#if diag && (diag.lost_events || diag.last_storage_error)}<p class="warning" role="status">История неполна: потеряно событий текущего процесса — {diag.lost_events}. Причина: {diag.last_storage_error === "writer_capacity_exceeded" ? "превышен лимит записи" : diag.last_storage_error ? "хранилище недоступно" : "не записана"}. Потери предыдущего запуска неизвестны.</p>{/if}
  {#if diagnosticsError && error}<p class="warning">{diagnosticsError}</p>{/if}
  {#if page}
    {#if !page.enabled}<p class="warning">Сбор выключен. Прежние записи доступны; новые вызовы не сохраняются.</p>{/if}
    <section class="summary panel" aria-label="Сводка выборки">
      <div><b>{page.summary.calls}</b><span>вызовов по фильтрам</span><small>{page.summary.success} успех · {page.summary.running} выполняется</small></div>
      <div><b>{page.summary.errors}</b><span>ошибок</span><small>{page.summary.unknown} исход неизвестен · {page.summary.empty_success} пустых успехов</small></div>
      <div><b>{page.summary.timed_calls ? duration(Math.round(page.summary.total_ms / page.summary.timed_calls)) : "Не записано"}</b><span>средняя длительность</span><small>по {page.summary.timed_calls} вызовам с известной длительностью</small></div>
    </section>
    <div class:has-detail={selected !== null} class="call-layout">
      <section class="call-list panel" aria-label="Список вызовов" aria-busy={busy}>
        <h2 id="mcp-list-title" tabindex="-1">Вызовы · {page.summary.calls}</h2>
        {#if page.records.length === 0}<div class="empty" role="status">{activeFilters ? "Нет совпадений. Измените или сбросьте фильтры." : "Вызовов за последние 24 часа пока нет. Здесь появятся обращения MCP после начала сбора; проверьте также последние 7 дней."}</div>{/if}
        {#each page.records as a (a.id)}
          <button id={`mcp-call-${a.id}`} class="call-row" data-menu-kind="row" data-menu-row="mcp" class:selected={selected?.id === a.id} aria-pressed={selected?.id === a.id} onclick={() => open(a)} use:commandAdapter={{ 'mcp.details': { run: () => open(a) } }}>
            <time>{localTime(a.started)}</time><span><strong>{a.tool}</strong><small>{a.project_name ?? (a.project || "Без проекта")}</small><small>{a.project}</small><span class:bad={a.outcome === "error"} class="status-label">{status(a)}</span></span><span class="metrics">{quantity(a)}<small>{duration(a.duration_ms)}</small></span>
          </button>
        {/each}
        <footer><span>Показано {page.records.length} из {page.summary.calls}</span>{#if page.next}<button class="ghost" onclick={() => load(true)} disabled={busy || managing}>Ещё 50</button>{/if}</footer>
      </section>
      {#if selected}
        <section class="call-detail panel" aria-label="Подробности вызова">
          <header><h2 tabindex="-1" bind:this={detailsHeading}>{selected.tool} · #{selected.id}</h2><button class="ghost" onclick={close}>Закрыть подробности</button></header>
          <dl><dt>Проект</dt><dd>{selected.project_name ?? "Имя не записано"}<br />{selected.project || "Без проекта"}</dd><dt>Сессия</dt><dd>{selected.session}</dd><dt>Начало</dt><dd>{localTime(selected.started)}</dd><dt>Окончание</dt><dd>{selected.finished === null ? selected.outcome === "running" ? "Ещё выполняется" : "Не записано" : localTime(selected.finished)}</dd><dt>Длительность</dt><dd>{duration(selected.duration_ms)}</dd></dl>
          <p class="muted small">Время на worker, без IPC, форматирования и записи истории. Сессия — соединение MCP, не чат. Доставка клиенту неизвестна.</p>
          <h3>Результат</h3><div class="result-box"><b class:bad={selected.outcome === "error"}>{status(selected)} · {quantity(selected)}</b>
            {#if selected.result?.queued}<p>Задание принято в очередь; это не завершение индексации.</p>{/if}
            {#if selected.result?.warning || selected.applied?.rerank_fallback}<p class="warning">Уточнение порядка не выполнено, использован запасной способ. Причина: {value(selected.applied?.rerank_fallback_reason)}. Фактическая шкала — в применённых параметрах.</p>{/if}
            {#if selected.error}<p class="bad">{value(selected.error.message)} · {value(selected.error.kind)} · код {value(selected.error.code)} · этап {value(selected.error.stage)}</p>{/if}
          </div>
          {#if selected.incomplete.length}<p class="warning">Неполная запись; скрыто, усечено или недоступно: {selected.incomplete.map(incompleteness).join("; ")}. Отсутствующие значения не восстанавливаются.</p>{/if}
          <h3>Параметры на момент вызова</h3>
          {#if !selected.requested && !selected.applied}<p>Параметры не записаны.</p>{:else}<table class="params"><thead><tr><th>Параметр</th><th>Запрошено</th><th>Применено / причина</th></tr></thead><tbody>{#each parameters(selected) as p}<tr><th>{p.label}</th><td>{p.requested}</td><td>{p.applied}<small>{p.reason}</small></td></tr>{/each}</tbody></table>{/if}
          <details><summary>Техническая запись JSON · версия {selected.version}</summary><pre>{JSON.stringify(selected, null, 2)}</pre></details>
        </section>
      {/if}
    </div>
  {/if}
  <details class="policy"><summary>Хранение и охват истории</summary>
    <p>Локальные запросы и пути: максимум 7 × 24 часа, 10 000 попыток / 32 МиБ данных. CLI, окно, ошибки до допуска worker и доставка клиенту не учитываются. Подробности GraphRAG могут быть неполными.</p>
    <label class="inline"><input type="checkbox" checked={page?.enabled ?? false} disabled={!page || busy || managing} onchange={e => { e.currentTarget.checked = page?.enabled ?? false; void manage(!page?.enabled); }} /> Сохранять подробную историю MCP</label>
    <p>Отключение оставляет прежние записи. Исполнение инструментов независимо от сбора истории.</p>
    <button class="ghost danger" bind:this={clearButton} onclick={askClear} disabled={busy || managing}>Очистить всю историю</button>
    {#if confirmClear}<section class="confirm" role="group" aria-label="Подтверждение очистки"><p>Удалить историю всех проектов и сессий, включая прежние агрегаты? Исполнение инструментов продолжится. Отменить удаление нельзя.</p><button class="ghost" bind:this={cancelButton} onclick={cancelClear} disabled={managing}>Отмена</button> <button class="ghost danger" onclick={() => manage(undefined, true)} disabled={managing}>Удалить всю историю</button></section>{/if}
  </details>
  <details class="legacy"><summary>Прежние агрегаты без подробностей · отдельно</summary><p class="muted">Все сохранившиеся старые агрегаты за срок хранения. Не входят в сводку и фильтры отдельных вызовов; параметры восстановить нельзя.</p><button class="ghost" onclick={readLegacy} disabled={legacyBusy || managing}>Загрузить прежние агрегаты</button>{#if legacyError}<p role="alert">{legacyError}</p>{/if}{#if legacy}<p>{legacy.length ? `${legacy.length} агрегатов` : "Прежних агрегатов нет"}</p>{#each legacy as row}<div class="legacy-row"><b>{row.tool}</b> · {row.calls} вызовов · {row.errors} ошибок · {duration(row.calls ? Math.round(row.total_ms / row.calls) : null)}<small>{row.project || "Без проекта"} · {row.session}<br />{localTime(row.first)} — {localTime(row.last)}</small></div>{/each}{/if}</details>
  <p class="muted small coverage">{page?.coverage ?? "Учитываются допущенные вызовы на worker. Отсутствие записи не доказывает отсутствие вызова."}</p>
</div>

<style>
  .mcp-page { container-type:inline-size; min-width:0; } h1 { font-size:22px; margin:0; } h2 { font-size:14px; margin:0; overflow-wrap:anywhere; } h3 { font-size:13px; margin:20px 0 9px; }
  header { display:flex; justify-content:space-between; align-items:start; gap:12px; } header p { margin:4px 0; } .muted, small { color:var(--muted); } .small, small { font-size:12px; } small { display:block; overflow-wrap:anywhere; } .panel { border:1px solid var(--line); border-radius:12px; background:var(--panel); }
  .filters { display:grid; grid-template-columns:repeat(5,minmax(0,1fr)); gap:12px; margin:20px 0 12px; } label { display:flex; flex-direction:column; gap:6px; font-size:12px; min-width:0; } select, input:not([type=checkbox]) { width:100%; min-width:0; padding:8px; background:var(--panel); color:var(--text); border:1px solid var(--line-2); border-radius:8px; } .inline { flex-direction:row; align-items:center; gap:8px; } .filter-extra { display:flex; flex-wrap:wrap; gap:12px; align-items:center; margin-bottom:18px; }
  .summary { display:grid; grid-template-columns:repeat(3,minmax(0,1fr)); margin:18px 0; } .summary>div { padding:14px 18px; border-right:1px solid var(--line); min-width:0; } .summary>div:last-child { border:0; } .summary b { display:block; font-size:25px; font-variant-numeric:tabular-nums; overflow-wrap:anywhere; } .summary span { font-size:12px; }
  .call-layout { display:grid; grid-template-columns:minmax(0,1fr); gap:16px; align-items:start; } .call-layout.has-detail { grid-template-columns:minmax(0,1.05fr) minmax(0,1fr); } .call-list { overflow:hidden; } .call-list h2 { padding:14px 16px; border-bottom:1px solid var(--line); } .call-row { display:grid; width:100%; grid-template-columns:84px minmax(0,1fr) 100px; gap:10px; padding:12px 14px; text-align:left; border:0; border-bottom:1px solid var(--line); background:transparent; color:var(--text); } .call-row:hover { background:var(--panel-2); } .call-row.selected { background:var(--accent-soft); box-shadow:inset 3px 0 var(--accent); } .call-row strong { display:block; font:600 12px var(--mono); overflow-wrap:anywhere; } time { font-size:11px; color:var(--muted); } .metrics { text-align:right; font-size:12px; } .status-label { font-size:12px; } footer { display:flex; justify-content:space-between; align-items:center; padding:12px; font-size:12px; gap:8px; }
  .call-detail { min-width:0; padding:16px; position:sticky; top:12px; } .call-detail header { align-items:center; } .call-detail header button { font-size:11px; } dl { display:grid; grid-template-columns:90px minmax(0,1fr); gap:6px; font-size:12px; } dt { color:var(--muted); } dd { margin:0; overflow-wrap:anywhere; } .result-box { padding:12px; background:var(--panel-2); border-radius:8px; overflow-wrap:anywhere; font-size:12px; } .params { width:100%; border-collapse:collapse; table-layout:fixed; font-size:11px; } th { font-weight:400; text-align:left; color:var(--muted); } td, th { padding:8px 4px; border-bottom:1px solid var(--line); overflow-wrap:anywhere; vertical-align:top; } .params small { font-size:11px; margin-top:4px; } details { margin-top:18px; font-size:12px; } summary { cursor:pointer; color:var(--accent-text); } pre { padding:12px; background:var(--panel-2); border-radius:8px; white-space:pre-wrap; overflow-wrap:anywhere; font:11px/1.6 var(--mono); }
  .error, .warning, .confirm { border:1px solid var(--line-2); background:var(--panel-2); padding:12px; border-radius:8px; overflow-wrap:anywhere; } .bad, .error { color:var(--bad); } .warning { color:var(--text); } .empty { padding:24px; color:var(--muted); } .legacy-row { padding:12px; border-bottom:1px solid var(--line); overflow-wrap:anywhere; } .coverage { margin-top:20px; } button:focus-visible, input:focus-visible, select:focus-visible, summary:focus-visible, h2:focus-visible { outline:2px solid var(--accent); outline-offset:3px; }
  @container(max-width:900px) { .filters { grid-template-columns:repeat(3,minmax(0,1fr)); } .call-layout.has-detail { grid-template-columns:minmax(0,1fr); } .has-detail .call-list { display:none; } .call-detail { position:static; } }
  @container(max-width:580px) { .filters { grid-template-columns:repeat(2,minmax(0,1fr)); } .summary>div { padding:12px; } .summary b { font-size:20px; } .call-row { grid-template-columns:66px minmax(0,1fr) 80px; padding:12px 10px; gap:6px; } }
</style>
