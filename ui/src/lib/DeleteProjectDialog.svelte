<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "./app.svelte";
  let dialog: HTMLDialogElement;
  let cancel: HTMLButtonElement;
  onMount(() => { dialog.showModal(); cancel.focus(); });
</script>

<dialog bind:this={dialog} aria-labelledby="delete-title" oncancel={(event) => { event.preventDefault(); app.cancelDeletion(); }}>
  <h2 id="delete-title">Удалить проект «{app.deletingProject?.name}»?</h2>
  <p class="path mono">{app.deletingProject?.path}</p>
  <p>Удалятся регистрация, индекс, локальные настройки и задания bars. Отменить удаление нельзя.</p>
  <p><b>Документация и BARS_INSTRUCTION.md останутся на диске.</b> Общие модели и другие проекты сохранятся.</p>
  {#if app.deletingProject?.state === "unavailable"}
    <p>Папка недоступна. Запись исчезнет из bars; рабочие файлы на отключённом диске останутся.</p>
  {/if}
  {#if app.deleteBusy}<p role="status">Удаление… Ожидаем остановки индексации и закрытия индекса.</p>{/if}
  {#if app.deleteError}<p role="alert" class="error">{app.deleteError}</p>{/if}
  <footer>
    <button class="ghost" bind:this={cancel} disabled={app.deleteBusy} onclick={() => app.cancelDeletion()}>Отмена</button>
    <button class="btn danger" disabled={app.deleteBusy} onclick={() => app.confirmDeletion()}>Удалить проект</button>
  </footer>
</dialog>

<style>
  dialog { max-width: 480px; width: calc(100vw - 60px); padding: 28px; border: 1px solid var(--line); border-radius: 16px; background: var(--panel); color: var(--text); box-shadow: 0 24px 80px #0005; }
  dialog::backdrop { background: #0006; }
  h2 { margin: 0 0 12px; }
  p { line-height: 1.6; }
  .path { overflow-wrap: anywhere; font-size: 12px; }
  footer { display: flex; justify-content: flex-end; gap: 12px; margin-top: 24px; }
  .btn.danger { background: var(--bad); color: white; }
  .error { color: var(--bad); }
</style>
