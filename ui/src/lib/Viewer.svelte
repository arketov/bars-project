<script lang="ts">
  import { tick } from "svelte";
  import { app } from "./app.svelte";
  import Icon from "./Icon.svelte";
  import { renderDocument, resolveDocumentLink } from "./markdown";

  const doc = $derived(app.viewer);
  const html = $derived(
    doc && !doc.missing
      ? renderDocument(doc.text, doc.hit.first_line > 0 ? { first: doc.hit.first_line, last: doc.hit.last_line } : undefined)
      : "",
  );
  let body: HTMLElement | undefined = $state();

  function followLink(event: MouseEvent) {
    const link = (event.target as HTMLElement).closest<HTMLElement>("a[data-doc-link]");
    if (!link || !doc) return;
    event.preventDefault();
    const path = resolveDocumentLink(doc.hit.path, link.dataset.docLink ?? "");
    if (path) void app.openDocument({ path, title: path.split("/").pop() ?? path, first_line: 0, last_line: 0 });
  }

  // Документ прокручен к найденному разделу (main-window.md).
  $effect(() => {
    void html;
    void tick().then(() => body?.querySelector("#hit")?.scrollIntoView({ block: "center" }));
  });
</script>

{#if doc}
  <section class="viewer">
    <div class="vhead">
      <div class="vt"><b>{doc.title}</b><small>{doc.hit.path}</small></div>
      <button class="ghost" title="Открыть в программе, связанной с .md" onclick={() => app.openInEditor(doc.hit.path)}>
        <Icon name="external" size={14} />В редакторе
      </button>
      <button class="iconbtn" title="Закрыть" onclick={() => app.closeViewer()}><Icon name="close" size={16} /></button>
    </div>
    <div class="vbody" bind:this={body}>
      {#if doc.missing}
        <div class="gone">
          <p><b>Файл не найден, возможно, его удалили.</b></p>
          <p class="muted">Выдача могла устареть: обновите её, повторив поиск.</p>
          <button class="btn" onclick={() => { app.closeViewer(); void app.search(); }}>Обновить выдачу</button>
        </div>
      {:else}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <div class="md" onclick={followLink}>{@html html}</div>
      {/if}
    </div>
  </section>
{/if}

<style>
  .viewer { display: flex; flex-direction: column; min-width: 0; min-height: 0; background: var(--panel); border-left: 1px solid var(--line); animation: slide .25s both; }
  @keyframes slide { from { opacity: 0; transform: translateX(16px); } to { opacity: 1; transform: none; } }
  .vhead { display: flex; align-items: center; gap: 10px; padding: 14px 16px 12px 22px; border-bottom: 1px solid var(--line); }
  .vt { flex: 1; min-width: 0; }
  .vt b { display: block; font-size: 15px; }
  .vt small { display: block; color: var(--muted); font: 11.5px var(--mono); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .iconbtn { border: 1px solid transparent; background: none; color: var(--muted); padding: 5px; border-radius: 8px; display: grid; place-items: center; }
  .iconbtn:hover { background: var(--panel-2); border-color: var(--line); color: var(--text); }
  .vbody { flex: 1; overflow-y: auto; padding: 18px 28px 80px; min-height: 0; }
  .gone { padding-top: 40px; max-width: 420px; }
  .md { font-size: 14.5px; line-height: 1.7; max-width: 720px; }
  .md :global(h1) { font-size: 22px; margin: 0 0 12px; } .md :global(h2) { font-size: 17px; margin: 22px 0 8px; }
  .md :global(h3) { font-size: 15px; margin: 18px 0 6px; }
  .md :global(p) { margin: 0 0 .8em; } .md :global(ul), .md :global(ol) { margin: 0 0 .8em; padding-left: 1.3em; } .md :global(li) { margin: .2em 0; }
  .md :global(code) { font: 12.5px var(--mono); background: var(--panel-2); border: 1px solid var(--line); padding: 1px 5px; border-radius: 5px; }
  .md :global(pre) { background: var(--panel-2); border: 1px solid var(--line); border-radius: 10px; padding: 10px 12px; overflow-x: auto; }
  .md :global(pre code) { background: none; border: 0; padding: 0; }
  .md :global(table) { border-collapse: collapse; margin: 0 0 .8em; }
  .md :global(th), .md :global(td) { border: 1px solid var(--line); padding: 4px 8px; }
  .md :global(.a) { color: var(--accent-text); }
  .md :global(a.a) { text-decoration: underline; text-underline-offset: 2px; cursor: pointer; }
  .md :global(section.hit) { position: relative; background: var(--accent-soft); box-shadow: -14px 0 0 var(--accent-soft), 14px 0 0 var(--accent-soft); padding: 4px 0 6px; border-radius: 10px; }
  .md :global(section.hit::before) { content: ""; position: absolute; left: -14px; top: 0; bottom: 0; width: 3px; border-radius: 3px; background: var(--accent); }
  .md :global(.hitlabel) { display: inline-block; font: 600 11px var(--mono); color: var(--accent-text); margin-bottom: 2px; }
</style>
