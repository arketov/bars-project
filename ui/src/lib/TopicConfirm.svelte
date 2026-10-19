<script lang="ts">
  import { onMount } from "svelte";

  // Подтверждение разрушающего действия над «Новыми темами»: удаление темы и очистка списка (design.md, «Состояния экрана»).
  // Фокус стоит на «Отмене»; Escape и «Отмена» ничего не меняют.
  let {
    title,
    text,
    confirmLabel,
    busy = false,
    onconfirm,
    oncancel,
  }: { title: string; text: string; confirmLabel: string; busy?: boolean; onconfirm: () => void; oncancel: () => void } = $props();

  let dialog: HTMLDialogElement;
  let cancel: HTMLButtonElement;
  onMount(() => {
    dialog.showModal();
    cancel.focus();
  });
</script>

<dialog bind:this={dialog} aria-labelledby="topic-confirm-title" oncancel={(event) => { event.preventDefault(); if (!busy) oncancel(); }}>
  <h2 id="topic-confirm-title">{title}</h2>
  <p>{text}</p>
  <footer>
    <button class="ghost" bind:this={cancel} disabled={busy} onclick={oncancel}>Отмена</button>
    <button class="btn danger" disabled={busy} onclick={onconfirm}>{confirmLabel}</button>
  </footer>
</dialog>

<style>
  dialog { max-width: 460px; width: calc(100vw - 60px); padding: 24px 26px; border: 1px solid var(--line-2); border-radius: 16px; background: var(--panel); color: var(--text); box-shadow: 0 24px 80px #0005; }
  dialog::backdrop { background: #0006; }
  h2 { margin: 0 0 10px; font-size: 17px; }
  p { line-height: 1.6; color: var(--muted); margin: 0; }
  footer { display: flex; justify-content: flex-end; gap: 12px; margin-top: 22px; }
  .btn.danger { background: var(--bad); color: #fff; }
</style>
