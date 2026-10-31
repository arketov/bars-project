<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "./app.svelte";
  let dialog: HTMLDialogElement;
  let field: HTMLInputElement;
  let name = $state(app.renamingProject?.name ?? "");
  onMount(() => { dialog.showModal(); field.focus(); field.select(); });
</script>

<dialog bind:this={dialog} aria-labelledby="rename-project-title" oncancel={(event) => { event.preventDefault(); app.cancelRenameProject(); }}>
  <form onsubmit={(event) => { event.preventDefault(); void app.renameProject(name); }}>
    <h2 id="rename-project-title">Переименовать проект</h2>
    <label>Имя проекта<input bind:this={field} bind:value={name} disabled={app.renameBusy} autocomplete="off" /></label>
    <p>Имя в bars и инструментах. Папка и путь останутся прежними.</p>
    <p class="path mono">{app.renamingProject?.path}</p>
    {#if app.renameError}<p role="alert" class="error">{app.renameError}</p>{/if}
    <footer>
      <button type="button" class="ghost" disabled={app.renameBusy} onclick={() => app.cancelRenameProject()}>Отмена</button>
      <button type="submit" class="btn" disabled={app.renameBusy || !name.trim()}>{app.renameBusy ? "Сохранение…" : "Сохранить"}</button>
    </footer>
  </form>
</dialog>

<style>
  dialog { width: min(440px, calc(100vw - 48px)); box-sizing: border-box; padding: 24px; border: 1px solid var(--line); border-radius: 14px; background: var(--panel); color: var(--text); box-shadow: 0 24px 80px #0005; }
  dialog::backdrop { background: #0006; }
  h2 { margin: 0 0 16px; font-size: 16px; }
  label { display: grid; gap: 6px; font-size: 13px; }
  input { min-width: 0; padding: 8px; border: 1px solid var(--line-2); border-radius: 8px; background: var(--panel-2); color: var(--text); }
  p { font-size: 12px; color: var(--muted); overflow-wrap: anywhere; }
  .error { color: var(--bad); }
  footer { display: flex; justify-content: flex-end; gap: 10px; margin-top: 20px; }
</style>
