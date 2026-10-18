<script lang="ts">
  import { tick } from "svelte";
  import { app } from "./app.svelte";
  import Icon from "./Icon.svelte";
  import type { Notice } from "./notifications";

  let open = $state(false);
  let root: HTMLDivElement;
  let trigger: HTMLButtonElement;
  let panel = $state<HTMLElement>();
  const unread = $derived(app.notifications.filter(n => n.unread).length);
  $effect(() => { if (open && app.notifications.some(n => n.unread)) app.readNotifications(); });

  async function toggle() {
    open = !open;
    if (open) { await tick(); panel?.focus(); }
  }
  function close() { open = false; trigger?.focus(); }
  function outside(event: MouseEvent) { if (open && !root.contains(event.target as Node)) open = false; }
  async function visit(item: Notice) {
    close();
    if (item.target === "models") app.openSettings("models");
    else { if (item.project) await app.select(item.project); app.openIndexing(); }
  }
</script>

<svelte:window onclick={outside} onkeydown={(event) => { if (open && event.key === "Escape") { event.preventDefault(); close(); } }} />
<div class="notifications" bind:this={root}>
  <button class="bell" bind:this={trigger} title="Уведомления" aria-label={`Уведомления, непрочитанных: ${unread}`} aria-expanded={open} aria-controls="notifications-panel" onclick={toggle}>
    <Icon name="bell" size={17} width={2} />
    {#if unread}<span class="badge">{unread > 99 ? "99+" : unread}</span>{/if}
  </button>
  {#if open}
    <section id="notifications-panel" bind:this={panel} tabindex="-1" aria-label="Центр уведомлений">
      <header><b>Уведомления</b><button onclick={() => app.clearNotifications()} disabled={!app.notifications.some(n => !n.active)}>Очистить завершённые</button><button aria-label="Закрыть уведомления" onclick={close}><Icon name="close" /></button></header>
      <div class="list">
        {#each app.notifications as item (item.id)}
          <article class:bad={item.bad} class:active={item.active}>
            <b>{item.title}</b><p>{item.text}</p>
            {#if item.active}
              {#if item.percent !== null}<progress aria-label={item.title} value={item.percent} max="100"></progress><small>{item.percent} %</small>
              {:else}<progress aria-label={item.title}></progress>{/if}
            {/if}
            {#if item.target}<button class="details" onclick={() => visit(item)}>{item.target === "models" ? "Открыть модели" : "Ход индексации"} →</button>{/if}
          </article>
        {:else}<p class="empty">Уведомлений пока нет</p>{/each}
      </div>
      <footer>История за время работы окна</footer>
    </section>
  {/if}
</div>

<style>
  .notifications { position: relative; }
  button { cursor: pointer; color: var(--muted); border: 0; background: transparent; border-radius: 8px; padding: 5px; }
  button:hover { background: var(--panel-2); color: var(--text); }
  button:disabled { opacity: .45; cursor: default; }
  .bell { position: relative; display: grid; place-items: center; }
  .badge { position: absolute; top: -9px; right: -7px; background: var(--accent, #bb3e78); color: white; border-radius: 10px; min-width: 15px; padding: 1px 3px; font-size: 10px; }
  section { position: fixed; left: 14px; bottom: 65px; width: min(410px, calc(100vw - 28px)); max-height: calc(100vh - 90px); display: flex; flex-direction: column; background: var(--panel); border: 1px solid var(--line); border-radius: 14px; box-shadow: 0 12px 40px #0003; z-index: 70; color: var(--text); font-size: 13px; }
  header { display: flex; align-items: center; gap: 6px; padding: 12px; border-bottom: 1px solid var(--line); }
  header b { flex: 1; }
  header button { font-size: 11px; }
  .list { overflow: auto; min-height: 0; }
  article { padding: 14px; border-bottom: 1px solid var(--line); overflow-wrap: anywhere; }
  article.bad { border-left: 3px solid var(--bad); }
  p { margin: 6px 0; color: var(--muted); }
  progress { width: 100%; height: 7px; accent-color: var(--accent, #bb3e78); }
  small { display: block; text-align: right; }
  .details { display: block; padding-left: 0; margin-top: 6px; color: var(--accent, #bb3e78); }
  footer, .empty { padding: 12px; color: var(--muted); }
</style>
