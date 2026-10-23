<script lang="ts">
  import { onMount } from "svelte";
  import { followLink } from "./follow-link.svelte";
  let dialog: HTMLDialogElement;
  onMount(() => dialog.showModal());
</script>
<dialog bind:this={dialog} aria-label="Создать документ по ссылке" oncancel={(e) => { e.preventDefault(); if (!followLink.busy) followLink.pending = null; }}>
  <h2>Документа пока нет</h2>
  <p>{followLink.pending?.path}</p>
  <p>Создать документ по этому пути? Папка должна существовать.</p>
  {#if followLink.error}<p role="alert">{followLink.error}</p>{/if}
  <footer><button disabled={followLink.busy} onclick={() => followLink.pending = null}>Отмена</button><button class="btn" disabled={followLink.busy} onclick={() => void followLink.create()}>Создать документ</button></footer>
</dialog>
<style>
  dialog { width: min(440px, calc(100vw - 60px)); border: 1px solid var(--line); border-radius: 14px; padding: 24px; background: var(--panel); color: var(--text); }
  dialog::backdrop { background: #0006; }
  h2 { font-size: 16px; margin: 0 0 12px; }
  p { font-size: 13px; overflow-wrap: anywhere; }
  [role="alert"] { color: var(--bad); }
  footer { display: flex; justify-content: flex-end; gap: 10px; margin-top: 20px; }
</style>
