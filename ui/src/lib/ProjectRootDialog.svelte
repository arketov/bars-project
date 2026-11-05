<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "./app.svelte";
  import type { RootChoice } from "./api";
  let dialog: HTMLDialogElement;
  let choice = $state<RootChoice>(app.rootChoice?.suggested ?? "all");
  onMount(() => dialog.showModal());
</script>

<dialog bind:this={dialog} aria-labelledby="project-root-title" oncancel={(event) => { event.preventDefault(); app.cancelRoot(); }}>
  <form onsubmit={(event) => { event.preventDefault(); void app.confirmRoot(choice); }}>
    <h2 id="project-root-title">Что считать документацией?</h2>
    <p class="path mono">{app.rootChoice?.path}</p>
    <label class="opt">
      <input type="radio" name="root" value="all" bind:group={choice} disabled={app.rootBusy} />
      <span><b>Вся папка</b>Заметки, инструкции, руководства: bars индексирует все файлы .md в папке.</span>
    </label>
    <label class="opt">
      <input type="radio" name="root" value="docs" bind:group={choice} disabled={app.rootBusy} />
      <span><b>Папка docs/</b>Репозиторий кода: только документация из docs/.{app.rootChoice?.hasDocs ? "" : " В этой папке docs/ пока нет."}</span>
    </label>
    {#if app.rootChoice?.inside}
      {@const inside = app.rootChoice.inside}
      {@const moving = choice === "docs" ? inside.docs : inside.all}
      <p class="note" data-testid="nested-inside">Эта папка внутри проекта «{inside.name}»: её документы ({moving} файлов) уйдут из индекса «{inside.name}».</p>
    {/if}
    {#if app.rootChoice?.nested.length}
      <p class="note" data-testid="nested-found">Найдены вложенные проекты, они исключены: {app.rootChoice.nested.map((item) => item.name).join(", ")}.</p>
    {/if}
    <footer>
      <button type="button" class="ghost" disabled={app.rootBusy} onclick={() => app.cancelRoot()}>Отмена</button>
      <button type="submit" class="btn" disabled={app.rootBusy}>{app.rootBusy ? "Добавление…" : "Добавить"}</button>
    </footer>
  </form>
</dialog>

<style>
  dialog { width: min(480px, calc(100vw - 48px)); box-sizing: border-box; padding: 24px; border: 1px solid var(--line); border-radius: 14px; background: var(--panel); color: var(--text); box-shadow: 0 24px 80px #0005; }
  dialog::backdrop { background: #0006; }
  h2 { margin: 0 0 12px; font-size: 16px; }
  p { margin: 0 0 12px; font-size: 12px; color: var(--muted); overflow-wrap: anywhere; }
  .opt { display: flex; gap: 10px; align-items: flex-start; padding: 10px; margin-bottom: 8px; border: 1px solid var(--line-2); border-radius: 8px; font-size: 13px; cursor: pointer; }
  .opt span { display: grid; gap: 2px; color: var(--muted); font-size: 12px; }
  .opt b { color: var(--text); font-size: 13px; }
  .note { margin: 8px 0 0; color: var(--text); }
  footer { display: flex; justify-content: flex-end; gap: 10px; margin-top: 20px; }
</style>
