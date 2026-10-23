<script lang="ts">
  import { tick } from "svelte";
  import { commandAdapter } from './command-adapters';
  import { app } from "./app.svelte";
  import Icon from "./Icon.svelte";
  import type { Notice } from "./notifications";

  let open = $state(false);
  let root: HTMLDivElement;
  let trigger: HTMLButtonElement;
  let panel = $state<HTMLElement>();
  let list = $state<HTMLDivElement>();
  let expanded = $state<string | null>(null);
  const unread = $derived(app.notifications.filter(n => n.unread).length);
  function visible(node: HTMLElement, item: Notice) {
    let observer: IntersectionObserver | undefined;
    function watch(value: Notice) {
      observer?.disconnect();
      const version = app.notificationVersion(value.id);
      if (typeof IntersectionObserver === "undefined") return;
      observer = new IntersectionObserver(entries => {
        if (entries.some(e => e.isIntersecting && e.intersectionRatio >= .5)) app.readNotifications(value.id, version);
      }, { root: list, threshold: .5 });
      observer.observe(node);
    }
    watch(item);
    return { update: watch, destroy: () => observer?.disconnect() };
  }
  async function toggle() { open = !open; if (open) { await tick(); panel?.focus(); } }
  function close() { open = false; trigger?.focus(); }
  function outside(event: MouseEvent) { if (open && !root.contains(event.target as Node)) close(); }
  function keyboard(event: KeyboardEvent) {
    if (!open || event.defaultPrevented || document.querySelector('dialog[open], [role="menu"]')) return;
    if (event.key === "Escape") { event.preventDefault(); close(); }
    if (event.key === "Tab") {
      const buttons = [...(panel?.querySelectorAll<HTMLElement>('button:not(:disabled), summary, [tabindex="0"]') ?? [])];
      const first = buttons[0], last = buttons.at(-1);
      if (event.shiftKey && (document.activeElement === first || document.activeElement === panel)) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && (document.activeElement === last || document.activeElement === panel)) { event.preventDefault(); first?.focus(); }
    }
  }
  function time(value?: string) { return value ? new Date(value).toLocaleString("ru-RU", { dateStyle: "short", timeStyle: "medium" }) : ""; }
  function status(item: Notice) {
    if (item.unknown) return "Неизвестно";
    const states: Record<string, string> = { queued: "В очереди", queue_paused: "Очередь на паузе", pause_requested: "Запрос паузы", paused: "На паузе", stop_requested: "Остановка", partial: "Частичный итог", error: "Ошибка", cancelled: "Отменено", success: "Готово" };
    return states[item.operation?.state ?? ""] ?? (item.bad ? "Ошибка" : item.active ? "В работе" : "Готово");
  }
  async function visit(item: Notice) { await app.visitNotification(item); close(); }
</script>

<svelte:window onclick={outside} onkeydown={keyboard} />
<div class="notifications" bind:this={root} use:commandAdapter={{ 'notifications.open': { run: async () => { if (!open) await toggle(); else panel?.focus(); } } }}>
  <button class="bell" bind:this={trigger} title="Уведомления" aria-label={`Уведомления, непрочитанных: ${unread}`} aria-expanded={open} aria-controls="notifications-panel" onclick={toggle}>
    <Icon name="bell" size={17} width={2} />
    {#if unread}<span class="badge">{unread > 99 ? "99+" : unread}</span>{/if}
  </button>
  {#if open}
    <section id="notifications-panel" bind:this={panel} tabindex="-1" aria-label="Центр уведомлений">
      <header><b>Уведомления</b><button aria-label="Закрыть уведомления" onclick={close}><Icon name="close" /></button></header>
      <nav aria-label="Действия уведомлений"><button onclick={() => app.readNotifications()} disabled={!unread}>Прочитать все</button><button onclick={() => app.clearNotifications()} disabled={!app.notifications.some(n => !n.active && !n.unknown)}>Очистить завершённые</button></nav>
      {#if app.notificationError}<div class="connection" role="status">{app.notificationError}<button onclick={() => app.refresh(true)}>Проверить связь</button></div>{/if}
      {#if app.notificationHistoryWarning}<div class="connection" role="status">{app.notificationHistoryWarning}</div>{/if}
      <div class="list" bind:this={list}>
        {#each app.notifications as item (item.id)}
          <article class:bad={item.bad} class:active={item.active} class:unread={item.unread} use:visible={item} use:commandAdapter={{ 'notification.details': { run: () => { expanded = expanded === item.id ? null : item.id; }, reason: () => item.operation ? undefined : 'Нет данных операции' }, 'notification.retry': { run: () => app.retryNotification(item), reason: () => item.operation?.target_available && !item.active && !item.unknown && ['error','partial','cancelled'].includes(item.operation.state) ? undefined : 'Повтор недоступен' } }}>
            <div class="notice-icon" class:ok={item.operation?.state === "success"} class:warn={item.unknown || item.operation?.state === "cancelled"}><Icon name={item.unknown ? "warn" : item.bad ? "error" : item.active ? "refresh" : item.operation?.state === "cancelled" ? "stop" : "check"} size={16} /></div>
            <div class="content">
            <div class="heading"><b>{item.title}</b><span class="status">{status(item)}</span></div>
            {#if item.project || item.operation?.model}<div class="subject">{item.project ?? item.operation?.model}{#if item.operation?.variant}<span class="variant"> · {item.operation.variant}</span>{/if}</div>{/if}
            <p class="result">{item.text}</p>
            {#if item.active && !item.unknown && !["queued", "queue_paused"].includes(item.operation?.state ?? "")}
              {#if item.percent !== null}<progress aria-label={item.title} value={item.percent} max="100"></progress><small>{item.percent} %</small>
              {:else}<progress aria-label={item.title}></progress>{/if}
            {/if}
            <div class="meta">{#if item.updatedAt}<time datetime={item.updatedAt}>{time(item.updatedAt)}</time>{/if}<div class="actions">
              {#if item.operation}<button class="details" aria-expanded={expanded === item.id} onclick={() => expanded = expanded === item.id ? null : item.id}>Подробнее</button>{/if}
              {#if item.target === "audits"}<button class="details" onclick={() => visit(item)}>Открыть аудиты →</button>{/if}
              {#if item.operation && !item.active && !item.unknown && ["error", "partial", "cancelled"].includes(item.operation.state)}<button disabled={!item.operation.target_available} onclick={() => app.retryNotification(item)}>Повторить</button>{/if}
            </div></div>
            {#if expanded === item.id && item.operation}
              {@const op = item.operation}
              <div class="operation-details">
                <p>ID: {item.epoch}:{op.id}</p><p>Начало: {time(op.started_at)}</p><p>Окончание: {op.finished_at ? time(op.finished_at) : "Подтверждённого итога нет"}</p>
                {#if op.finished_at}<p>Длительность: {Math.max(0, (Date.parse(op.finished_at) - Date.parse(op.started_at)) / 1000).toLocaleString("ru-RU")} с · с постановки в очередь</p>{/if}
                <p>Причина: {op.source === "explicit" ? "Явный запрос" : op.source === "watch" ? "Изменения файлов" : "Периодическая проверка"}</p>
                {#if op.report}<p>Добавлено {op.report.added} · изменено {op.report.changed} · удалено {op.report.removed} · без изменений {op.report.unchanged} · ошибок {op.report.problems}</p><p>{op.report.index_applied ? "Результат применён к индексу" : "Сохранён прежний индекс"}</p>{/if}
                {#if op.error}<p>{op.error}</p>{/if}
                {#if op.report?.problem_paths?.length}<p>Файлы с ошибками этой попытки ({op.report.problem_paths.length} из {op.report.problems}):</p><ul>{#each op.report.problem_paths as path}<li>{path}</li>{/each}</ul>{/if}
                {#if !op.target_available || item.unknown}<p>Прежняя цель недоступна или исход неизвестен. Сохранён результат этой попытки.</p>{:else}<button class="details" onclick={() => visit(item)}>{item.project ? "Ход индексации проекта" : "Открыть модели"} →</button>{/if}
              </div>
            {/if}
            </div>
          </article>
        {:else}<div class="empty"><Icon name="bell" size={25} /><b>Уведомлений пока нет</b><p>Здесь появятся ход и результаты индексации и подготовки моделей.</p></div>{/each}
      </div>
      <footer>История за время работы окна · активные и неизвестные исходы сохраняются</footer>
    </section>
  {/if}
</div>

<style>
  .notifications { position: relative; }
  button { cursor: pointer; color: var(--muted); border: 0; background: transparent; border-radius: 8px; padding: 6px 8px; font: inherit; }
  button:hover { background: var(--panel-2); color: var(--text); }
  button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  button:disabled { opacity: .45; cursor: default; }
  .bell { position: relative; display: grid; place-items: center; }
  .badge { position: absolute; top: -9px; right: -7px; background: var(--accent, #bb3e78); color: white; border-radius: 10px; min-width: 15px; padding: 1px 3px; font-size: 10px; }
  section { position: fixed; left: 14px; bottom: 65px; width: min(440px, calc(100vw - 28px)); max-height: calc(100vh - 90px); display: flex; flex-direction: column; background: var(--panel); border: 1px solid var(--line); border-radius: 14px; box-shadow: 0 12px 40px #0003; z-index: 70; color: var(--text); font-size: 13px; }
  header { display: flex; justify-content: space-between; align-items: start; gap: 6px; padding: 16px 16px 8px; }
  header b { font-size: 16px; }
  nav { display: flex; justify-content: space-between; padding: 4px 8px 10px; border-bottom: 1px solid var(--line); font-size: 12px; }
  .list { overflow: auto; min-height: 0; overscroll-behavior: contain; }
  article { display: grid; grid-template-columns: 28px minmax(0, 1fr); gap: 10px; padding: 14px 16px; border-bottom: 1px solid var(--line); overflow-wrap: anywhere; border-left: 3px solid transparent; }
  .notice-icon { display: grid; place-items: center; width: 28px; height: 28px; border-radius: 8px; color: var(--accent-text); background: var(--accent-soft); }
  .notice-icon.ok { color: var(--good); background: var(--good-soft); } .bad .notice-icon { color: var(--bad); background: var(--bad-soft); }
  .notice-icon.warn { color: var(--mid); background: var(--mid-soft); }
  article.unread { border-left-color: var(--accent); } article.bad { background: color-mix(in srgb, var(--bad) 5%, var(--panel)); }
  .heading { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; }
  .status, time, small { color: var(--muted); font-size: 11px; } .status { flex-shrink: 0; }
  .subject { margin-top: 4px; font-weight: 600; line-height: 1.4; }
  .variant { font-weight: 400; color: var(--muted); font-size: 11px; }
  p { margin: 5px 0; color: var(--muted); line-height: 1.4; font-size: 12px; } p.result { color: var(--text); }
  progress { width: 100%; height: 6px; accent-color: var(--accent, #bb3e78); } small { display: block; text-align: right; }
  time { display: block; } .meta { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 4px; margin-top: 5px; } .actions { display: flex; gap: 4px; font-size: 12px; }
  .details { color: var(--accent-text, #bb3e78); } .operation-details { background: var(--panel-2); padding: 10px; border-radius: 8px; margin-top: 8px; }
  .connection { padding: 12px; background: var(--panel-2); line-height: 1.5; } .connection button { display: block; }
  footer { padding: 12px 16px; color: var(--muted); font-size: 11px; border-top: 1px solid var(--line); }
  .empty { padding: 36px 25px; color: var(--muted); text-align: center; } .empty b { display: block; color: var(--text); margin-top: 12px; }
</style>
