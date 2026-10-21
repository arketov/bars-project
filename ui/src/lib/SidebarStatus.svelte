<script lang="ts">
  import { app } from "./app.svelte";
  import { problemStatus } from "./format";
  import { theme } from "./theme.svelte";
  import Icon from "./Icon.svelte";
  import Notifications from "./Notifications.svelte";
  /** Строка состояния: заголовок и вторая строка (время обновления, ход). */
  function statusText(): [string, string] {
    const status = app.status;
    const running = app.activity?.running;
    if (!app.project) return ["Нет проектов", ""];
    if (running && running.project === app.project.path) {
      const percent = app.progressOf(app.project.path);
      return percent === null ? ["Индексация", "обход папки"] : ["Индексация", `проект ${percent} %`];
    }
    if (app.project.state === "unavailable") return ["Папка проекта не найдена", ""];
    if (app.project.state === "reindex_required") return ["Нужна переиндексация", ""];
    if (app.waitingOf(app.project.path) > 0) return [`Ждут ${app.waitingOf(app.project.path)} документов`, "поиск работает по готовому"];
    const problem = app.activity?.problems.find(p => p.project === app.project?.path);
    if (problem) return [problemStatus(problem), "Подробнее — в ходе индексации"];
    if (status?.updated_at) {
      const time = new Date(status.updated_at).toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
      return ["Индекс актуален", `обновлён ${time}`];
    }
    return ["Индекс актуален", ""];
  }

  const status = $derived(statusText());

  const statusDot = $derived(
    !app.project
      ? "off"
      : app.project.state === "reindex_required"
        ? "warn"
        : app.project.state === "unavailable"
          ? "off"
          : app.progressOf(app.project.path) !== null || app.waitingOf(app.project.path) > 0
            ? "busy"
            : (app.activity?.problems.some((p) => p.project === app.project?.path) ?? false)
              ? "warn"
              : "ok",
  );
</script>
  <div class="statusbar">
    <div class="status">
      <span class="dot {statusDot}"></span>
      <span class="text">
        <span class="line1">{status[0]}</span>
        {#if status[1]}<span class="line2">{status[1]}</span>{/if}
      </span>
    </div>
    <div class="tools">
      <Notifications />
      <button class="iconbtn" title="Светлая / тёмная тема" onclick={() => theme.toggle()}>
        <Icon name="moon" size={14} width={2} />
      </button>
      <button class="iconbtn" title="Настройки" onclick={() => app.openSettings()}>
        <Icon name="gear" size={14} width={2} />
      </button>
    </div>
  </div>
<style>
  .statusbar { height: 38px; display: flex; align-items: center; justify-content: space-between; gap: 5px; padding: 3px 7px; border-top: 1px solid var(--line); background: var(--panel); }
  .status { display: flex; align-items: center; gap: 6px; min-width: 0; font-size: 11px; }
  .text { display: flex; flex-direction: column; min-width: 0; line-height: 1.2; }
  .line1, .line2 { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; } .line2 { font-size: 9px; color: var(--muted); }
  .tools { display: flex; gap: 1px; flex: none; }
  .iconbtn { display: grid; place-items: center; padding: 3px; border: 0; border-radius: 4px; background: none; color: var(--muted); }
  .iconbtn:hover { background: var(--panel-2); color: var(--text); } .tools :global(.bell) { padding: 3px; }
</style>