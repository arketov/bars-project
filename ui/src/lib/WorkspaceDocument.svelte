<script lang="ts">
  import { tick } from "svelte";
  import { call } from "./api";
  import Icon from "./Icon.svelte";
  import { app, type DocumentReference, type ViewedDocument } from "./app.svelte";
  import { BarsError, type ReadOutput } from "./types";
  import { documentHeadings, renderDocument } from "./markdown";
  let { project, hit, initial }: { project: string; hit: DocumentReference; initial?: ViewedDocument } = $props();
  let doc = $state<ViewedDocument | null>(null), error = $state("");
  let source = $state(false), outline = $state(true);
  let body = $state<HTMLDivElement>();
  const html = $derived(doc ? renderDocument(doc.text, hit.first_line > 0 ? { first: hit.first_line, last: hit.last_line } : undefined) : "");
  const lines = $derived(doc?.text.split("\n") ?? []);
  const headings = $derived(documentHeadings(doc?.text ?? ""));
  $effect(() => {
    const reference = hit, path = project, seed = initial;
    let cancelled = false;
    error = "";
    if (seed && seed.hit.path === reference.path) doc = seed;
    else {
      doc = null;
      void call<ReadOutput>("read", { cwd: path, project: path, path: `./${reference.path}` }).then(result => {
        if (!cancelled) doc = { hit: reference, title: result.title, text: result.text, missing: false };
      }).catch(e => {
        if (cancelled) return;
        if (e instanceof BarsError && e.kind === "file_not_found") doc = { hit: reference, title: reference.title, text: "", missing: true };
        else error = String(e);
      });
    }
    return () => { cancelled = true; };
  });
  function scroll(line: number) {
    const target = source ? body?.querySelector(`[data-line="${line}"]`) : body?.querySelector("#hit");
    target?.scrollIntoView({ block: "center" });
  }
  $effect(() => { void html; void source; void tick().then(() => scroll(hit.first_line)); });
</script>

<section class="document" aria-label={hit.title} data-document-path={hit.path}>
  <header data-document-path={hit.path}><div><b>{doc?.title ?? hit.title}</b><small>{hit.path}</small></div>
    <button title={source ? "Показать Markdown" : "Показать исходник (только чтение)"} aria-label="Исходник / рендер" aria-pressed={source} onclick={() => source = !source}><Icon name="eye" size={15} /></button>
  </header>
  {#if error}<p role="alert">Не удалось прочитать документ: {error}</p>
  {:else if !doc}<p role="status">Чтение…</p>
  {:else if doc.missing}<p role="alert">Файл не найден, возможно, его удалили.</p><button onclick={() => { app.openSearch(); void app.search(); }}>Обновить выдачу</button>
  {:else}
    <div class="body" bind:this={body}>
      {#if source}<pre aria-label="Исходник Markdown, только чтение">{#each lines as line, i}<span data-line={i + 1} class:hit={i + 1 >= hit.first_line && i + 1 <= hit.last_line}>{line}{"\n"}</span>{/each}</pre>
      {:else}<div class="md">{@html html}</div>{/if}
    </div>
    {#if outline}<nav class="outline" aria-label="Структура документа"><button class="close" aria-label="Скрыть структуру" onclick={() => outline = false}>×</button>{#each headings as h}<button onclick={() => { source = true; void tick().then(() => scroll(h.line)); }}>{h.label}</button>{/each}</nav>
    {:else}<button class="outline-open" title="Показать структуру" onclick={() => outline = true}>☷</button>{/if}
  {/if}
</section>

<style>
  .document { position: relative; display: flex; flex-direction: column; height: 100%; background: var(--panel); min-width: 0; min-height: 0; }
  header { display: flex; gap: 8px; align-items: center; border-bottom: 1px solid var(--line); padding: 6px 10px; }
  header div { flex: 1; min-width: 0; } b, small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; } b { font-size: 13px; } small { font: 10px var(--mono); color: var(--muted); }
  button { border: 0; background: transparent; color: var(--muted); padding: 3px 6px; border-radius: 4px; } button:hover { background: var(--accent-soft); color: var(--accent-text); }
  .body { flex: 1; overflow: auto; padding: 10px 14px 60px; min-height: 0; }
  pre { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; font: 12px/1.65 var(--mono); } pre span { display: block; } pre .hit { background: var(--accent-soft); }
  .md { font-size: 12px; line-height: 1.65; overflow-wrap: anywhere; } .md :global(h1) { font-size: 14px; } .md :global(h2), .md :global(h3) { font-size: 13px; }
  .md :global(pre) { overflow: auto; background: var(--panel-2); padding: 8px; } .md :global(table) { border-collapse: collapse; } .md :global(td), .md :global(th) { border: 1px solid var(--line); padding: 4px; }
  .md :global(.hit) { background: var(--accent-soft); border-left: 3px solid var(--accent); padding: 4px 8px; } .md :global(.hitlabel) { font-size: 10px; color: var(--accent-text); }
  .outline { position: absolute; right: 10px; top: 62px; width: 150px; max-height: 36%; overflow: auto; padding: 5px; border: 1px solid var(--line); border-radius: 6px; background: color-mix(in srgb, var(--panel) 90%, transparent); box-shadow: var(--shadow); }
  .outline button { display: block; width: 100%; text-align: left; font-size: 11px; padding: 2px 4px; } .outline .close { width: auto; margin-left: auto; } .outline-open { position: absolute; right: 10px; top: 62px; background: var(--panel-2); }
</style>
