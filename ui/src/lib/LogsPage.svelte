<script lang="ts">
  import { onMount } from "svelte";
  import { commandAdapter } from './command-adapters';
  import { copyText, openPath } from './api';
  import { workerLogs, type LogLevel, type LogSnapshot } from "./api";

  let level = $state<LogLevel>("info");
  let data = $state<LogSnapshot | null>(null);
  let loading = $state(false);
  let error = $state("");
  let request = 0;
  const names: Record<LogLevel, string> = { debug: "Отладка", info: "Информация", warn: "Предупреждение", error: "Ошибка" };

  async function refresh() {
    const id = ++request;
    loading = true;
    error = "";
    try {
      const response = await workerLogs(level);
      if (id === request) data = response;
    } catch (e) {
      if (id === request) error = e instanceof Error ? e.message : String(e);
    } finally {
      if (id === request) loading = false;
    }
  }

  function changeLevel(event: Event) {
    level = (event.currentTarget as HTMLSelectElement).value as LogLevel;
    data = null;
    void refresh();
  }

  onMount(() => { void refresh(); return () => { request++; }; });
</script>

<section aria-label="Логи воркера" use:commandAdapter={{
  'logs.refresh': { run: refresh, reason: () => loading ? 'Журнал загружается' : undefined },
  'logs.copy': { run: async () => { await copyText(data!.entries.map(e => `${e.time} ${e.level} ${e.message}`).join('\n')); }, reason: () => data ? undefined : 'Журнал ещё не прочитан' },
  'logs.reveal': { run: async () => { await openPath(data!.directory); }, reason: () => data ? undefined : 'Папка журнала ещё неизвестна' }
}}>
  <h1>Логи</h1>
  <p class="lead">События воркера. Последние 500 записей выбранного уровня, новые сверху.</p>
  <p class="path">{data?.directory ?? "~/.bars/logs"} · текущий файл и 3 архива по 2 МиБ</p>
  <div class="controls">
    <label>Уровень
      <select value={level} onchange={changeLevel}>
        <option value="debug">Отладка и выше</option>
        <option value="info">Информация и выше</option>
        <option value="warn">Предупреждения и ошибки</option>
        <option value="error">Только ошибки</option>
      </select>
    </label>
    <button class="ghost" onclick={() => void refresh()} disabled={loading}>Обновить</button>
  </div>
  {#if loading}<p role="status">Чтение журнала…</p>{/if}
  {#if error}<p role="alert" class="bad">{error}{data ? " Показаны прежние записи." : ""}</p>{/if}
  {#if data}
    {#if data.write_error}<p role="alert" class="bad">Последняя запись журнала не удалась. Проверьте свободное место и доступ к папке логов.</p>{/if}
    {#if data.skipped}<p role="status">Пропущено повреждённых записей: {data.skipped}.</p>{/if}
    {#if !data.entries.length}
      <p role="status">{data.total ? "Нет записей выбранного уровня." : "Журнал пока пуст."}</p>
    {:else}
      <ol aria-label="Записи журнала" aria-busy={loading}>
        {#each data.entries as entry}
          <li class:warning={entry.level === "warn"} class:failure={entry.level === "error"}>
            <div class="meta"><time datetime={entry.time}>{new Date(entry.time).toLocaleString("ru-RU")}</time> · <b>{names[entry.level]}</b> · PID {entry.pid}</div>
            <div class="target">{entry.target}</div>
            <p class="message">{entry.message}</p>
          </li>
        {/each}
      </ol>
    {/if}
  {/if}
</section>

<style>
  h1 { font-size: 22px; margin: 0 0 4px; }
  .lead, .path, .meta, .target { color: var(--muted); }
  .lead { margin: 0 0 12px; }
  .path { font: 12px var(--mono); overflow-wrap: anywhere; }
  .controls { display: flex; flex-wrap: wrap; gap: 12px; align-items: end; margin: 18px 0; }
  label { display: flex; flex-direction: column; gap: 5px; font-size: 12px; }
  select { padding: 8px; border: 1px solid var(--line); border-radius: 8px; background: var(--panel); color: var(--text); }
  ol { list-style: none; padding: 0; }
  li { background: var(--panel); border: 1px solid var(--line); border-radius: 10px; padding: 14px 16px; margin: 10px 0; }
  li.warning { border-left: 4px solid var(--mid); }
  li.failure { border-left: 4px solid var(--bad); }
  .meta { font-size: 12px; }
  .target { font: 11px var(--mono); margin-top: 5px; overflow-wrap: anywhere; }
  .message { white-space: pre-wrap; overflow-wrap: anywhere; font: 12px/1.6 var(--mono); margin: 10px 0 0; }
  .bad { color: var(--bad); }
</style>
