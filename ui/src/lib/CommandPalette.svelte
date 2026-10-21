<script lang="ts" module>
  export interface Choice { id: string; title: string; hint?: string; reason?: string; run: () => void | Promise<void> }
</script>
<script lang="ts">
  import { tick } from 'svelte';
  import { app } from './app.svelte';
  let dialog: HTMLDialogElement, input: HTMLInputElement;
  let opened = $state(false), query = $state(''), index = $state(0), title = $state('Список действий'), choices = $state<Choice[]>([]);
  let previous: HTMLElement | null = null;
  const rows = $derived(choices.filter(c => `${c.title} ${c.id}`.toLowerCase().includes(query.toLowerCase())));
  export function open(items: Choice[], heading = 'Список действий') {
    if (opened) { input.focus(); return; }
    previous = document.activeElement as HTMLElement; choices = items; title = heading; query = ''; index = 0; opened = true; dialog.showModal(); input.focus();
  }
  export function close(restore = true) { if (!opened) return; opened = false; dialog.close(); if (restore && previous?.isConnected) previous.focus(); }
  export function isOpen() { return opened; }
  async function execute(choice: Choice) { if (choice.reason) return; close(); try { await choice.run(); } catch(error) { app.flash(String(error),true); } }
  function key(event: KeyboardEvent) {
    if (event.isComposing || event.keyCode === 229) return;
    if (['ArrowDown','ArrowUp','Home','End'].includes(event.key)) {
      event.preventDefault(); index = event.key === 'Home' ? 0 : event.key === 'End' ? rows.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + rows.length) % Math.max(1, rows.length);
      void tick().then(() => dialog.querySelector(`[data-choice="${index}"]`)?.scrollIntoView?.({ block:'nearest' }));
    } else if (event.key === 'Enter') { event.preventDefault(); const choice = rows[index]; if (choice) void execute(choice); }
  }
</script>
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<dialog bind:this={dialog} aria-label={title} onkeydown={key} oncancel={(e) => { e.preventDefault(); close(); }}>
  <header><b>{title}</b><button aria-label="Закрыть список действий" onclick={() => close()}>×</button></header>
  <input bind:this={input} bind:value={query} oninput={() => index = 0} aria-label="Найти команду" placeholder="Название или категория" aria-activedescendant={rows[index] ? `command-${index}` : undefined} aria-controls="command-rows" />
  <div class="rows" id="command-rows" role="listbox" aria-label="Команды">
    {#each rows as choice, i}<button id={`command-${i}`} data-choice={i} role="option" aria-selected={i === index} aria-disabled={!!choice.reason} class:selected={i === index} onclick={() => void execute(choice)}><span>{choice.title}<small>{choice.reason}</small></span><kbd>{choice.hint ?? ''}</kbd></button>{/each}
    {#if !rows.length}<p>Команды не найдены</p>{/if}
  </div>
</dialog>
<style>
  dialog { width:min(640px,calc(100vw - 32px)); max-height:70vh; margin:12vh auto auto; padding:10px; background:var(--panel); color:var(--text); border:1px solid var(--line); border-radius:8px; font-size:12px; } dialog::backdrop { background:#0005; } header { display:flex; justify-content:space-between; align-items:center; margin-bottom:8px; } input { width:100%; box-sizing:border-box; font:inherit; padding:8px; } .rows { overflow:auto; max-height:48vh; margin-top:8px; } .rows button { display:flex; justify-content:space-between; gap:12px; width:100%; text-align:left; padding:7px; border:0; background:transparent; color:inherit; font:inherit; } .rows button.selected { background:var(--active-bg,var(--line)); } [aria-disabled=true] { opacity:.55; } small { display:block; color:var(--muted); } kbd { white-space:nowrap; }
</style>
