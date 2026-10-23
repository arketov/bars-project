<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "./app.svelte";
  import { destinations, entries, isDirectory, parentNodePath } from "./entries.svelte";

  let dialog = $state<HTMLDialogElement>();
  let field = $state<HTMLInputElement | HTMLSelectElement>();
  const request = $derived(entries.request);
  const node = $derived(request?.node);
  const folder = $derived(node ? isDirectory(node) : false);
  const targets = $derived(request && node ? destinations(app.tree, node) : []);
  let value = $state("");

  const where = $derived(node && node.path !== "." ? `«${node.label}»` : "корне документации");
  const titles = {
    newDoc: () => `Новый документ в ${where}`,
    newDir: () => `Новая папка в ${where}`,
    rename: () => `Переименовать ${folder ? "папку" : "документ"} «${node?.label}»`,
    move: () => `Переместить ${folder ? "папку" : "документ"} «${node?.label}»`,
    trash: () => `Удалить ${folder ? "папку" : "документ"} «${node?.label}»?`,
  } as const;
  const submitLabel = { newDoc: "Создать", newDir: "Создать", rename: "Переименовать", move: "Переместить", trash: "В корзину" } as const;
  const warnsAboutLinks = $derived(!!request && request.action !== "newDoc" && request.action !== "newDir" && request.docs > 0);
  const unchanged = $derived(
    !!request && ((request.action === "rename" && (value.trim() === "" || value.trim() === request.name)) ||
      ((request.action === "newDoc" || request.action === "newDir") && value.trim() === "") ||
      (request.action === "move" && value === parentNodePath(request.node))),
  );

  onMount(() => {
    if (!request) return;
    value = request.action === "rename" ? request.name : request.action === "move" ? parentNodePath(request.node) : "";
    dialog?.showModal();
    queueMicrotask(() => {
      field?.focus();
      if (field instanceof HTMLInputElement && request.action === "rename") field.setSelectionRange(0, request.name.replace(/\.md$/i, "").length);
    });
  });

  function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!unchanged) void entries.submit(value);
  }
</script>

{#if request && node}
  <dialog bind:this={dialog} aria-labelledby="entry-title" oncancel={(event) => { event.preventDefault(); entries.cancel(); }}>
    <form onsubmit={submit}>
      <h2 id="entry-title">{titles[request.action]()}</h2>
      {#if entries.report}
        <p>Перемещение выполнено. Не удалось обновить:</p>
        <ul>{#each entries.report as failure}<li><b>{failure.path}</b>: {failure.reason}</li>{/each}</ul>
      {:else if request.action === "newDoc" || request.action === "newDir" || request.action === "rename"}
        <label>
          {request.action === "newDir" ? "Имя папки" : request.action === "rename" && folder ? "Новое имя папки" : "Имя документа"}
          <input bind:this={field} bind:value autocomplete="off" spellcheck="false" aria-invalid={entries.error ? "true" : undefined} disabled={entries.busy} />
        </label>
        {#if request.action === "newDoc"}<p class="hint">Расширение .md добавится само. В файле будут заголовок и <code>title</code> из имени.</p>{/if}
        {#if request.action === "newDir"}<p class="hint">В папке сразу появится <code>index.md</code>: клик по имени папки открывает его.</p>{/if}
      {:else if request.action === "move"}
        <label>
          Куда перенести
          <select bind:this={field} bind:value disabled={entries.busy}>
            {#each targets as target (target.path)}
              <option value={target.path}>{"  ".repeat(target.depth)}{target.label}</option>
            {/each}
          </select>
        </label>
      {:else}
        <p>{folder ? `Папка вместе с документами внутри (${request.docs}) попадёт` : "Документ попадёт"} в корзину. Достать его можно из корзины Windows.</p>
      {/if}
      {#if !entries.report && warnsAboutLinks}
        <p class="hint warn">{request.action === "trash" ? "Ссылки на удалённые документы станут битыми." : "Перед перемещением bars покажет план обновления ссылок."}</p>
      {/if}
      {#if !entries.report && entries.preview && entries.previewValue === value}
        <p role="status">Документов с правками ссылок: {entries.preview.documents.length}.</p>
        <ul>{#each entries.preview.documents as doc}<li>{doc.path}: {doc.edits}</li>{/each}</ul>
        {#if entries.preview.warnings.length}<p class="warn">Ссылки, меняющие адресат из-за нового набора имён:</p><ul>{#each entries.preview.warnings as warning}<li>{warning.path}:{warning.line}</li>{/each}</ul>{/if}
      {/if}
      {#if entries.error}<p role="alert" class="error">{entries.error}</p>{/if}
      <footer>
        <button type="button" class="ghost" disabled={entries.busy} onclick={() => entries.cancel()}>{entries.report ? "Закрыть" : "Отмена"}</button>
        {#if !entries.report}<button type="submit" class="btn" class:danger={request.action === "trash"} disabled={entries.busy || unchanged}>{(request.action === "rename" || request.action === "move") && (!entries.preview || entries.previewValue !== value) ? "Проверить ссылки" : submitLabel[request.action]}</button>{/if}
      </footer>
    </form>
  </dialog>
{/if}

<style>
  dialog { max-width: 460px; width: calc(100vw - 60px); padding: 24px; border: 1px solid var(--line); border-radius: 14px; background: var(--panel); color: var(--text); box-shadow: 0 24px 80px #0005; }
  dialog::backdrop { background: #0006; }
  h2 { margin: 0 0 12px; font-size: 16px; overflow-wrap: anywhere; }
  label { display: grid; gap: 6px; font-size: 12.5px; color: var(--muted); }
  input, select { font-size: 13px; padding: 6px 8px; border-radius: 8px; border: 1px solid var(--line-2); background: var(--panel-2); color: var(--text); }
  input:focus, select:focus { outline: 2px solid var(--accent); outline-offset: -1px; }
  p { line-height: 1.55; font-size: 13px; margin: 10px 0 0; }
  .hint { color: var(--muted); font-size: 12px; }
  .hint.warn { color: var(--text); }
  .error { color: var(--bad); }
  ul { font-size: 12px; padding-left: 20px; max-height: 220px; overflow: auto; overflow-wrap: anywhere; }
  footer { display: flex; justify-content: flex-end; gap: 10px; margin-top: 20px; }
  .btn.danger { background: var(--bad); color: white; }
</style>
