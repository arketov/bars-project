<script lang="ts">
  import { tick } from "svelte";
  import { call } from "./api";
  import Icon from "./Icon.svelte";
  import { app, type DocumentReference, type ViewedDocument } from "./app.svelte";
  import { BarsError, type ReadOutput } from "./types";
  import { documentHeadings, renderDocument, resolveDocumentLink } from "./markdown";
  import { loadBacklinks, type Backlink, type Backlinks } from "./backlinks";
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
  let backlinks = $state<Backlinks | null>(null), backlinksError = $state(""), backlinksOpen = $state(false), backlinksHeight = $state(140);
  let section = $state<HTMLElement>();
  const clampHeight = (value: number) => Math.max(48, Math.min(value, (section?.clientHeight ?? 400) * 0.7));
  function resizeBacklinks(event: PointerEvent) {
    event.preventDefault();
    const startY = event.clientY, startHeight = backlinksHeight;
    const move = (e: PointerEvent) => backlinksHeight = clampHeight(startHeight + startY - e.clientY);
    const stop = () => { window.removeEventListener("pointermove", move); window.removeEventListener("pointerup", stop); };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
  }
  function resizeBacklinksByKey(event: KeyboardEvent) {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    backlinksHeight = clampHeight(backlinksHeight + (event.key === "ArrowUp" ? 24 : -24));
  }
  $effect(() => {
    const path = hit.path, root = project, docsRoot = app.docsRoot;
    let cancelled = false;
    backlinks = null;
    backlinksError = "";
    void loadBacklinks(root, path, docsRoot).then(result => { if (!cancelled) backlinks = result; }).catch(e => { if (!cancelled) backlinksError = e instanceof Error ? e.message : String(e); });
    return () => { cancelled = true; };
  });
  const openBacklink = (item: Backlink) => void app.openDocument({ path: item.path, title: item.title, first_line: item.lines[0] ?? 0, last_line: item.lines[0] ?? 0 });
  /** Ссылка на другой `.md` открывает его в окне; путь считается от документа, в котором ссылка стоит. */
  function followLink(event: MouseEvent) {
    const link = (event.target as HTMLElement).closest<HTMLElement>("a[data-doc-link]");
    if (!link) return;
    event.preventDefault();
    const path = resolveDocumentLink(hit.path, link.dataset.docLink ?? "");
    if (path) void app.openDocument({ path, title: path.split("/").pop() ?? path, first_line: 0, last_line: 0 });
  }
  function scroll(line: number) {
    const target = source ? body?.querySelector(`[data-line="${line}"]`) : body?.querySelector("#hit");
    target?.scrollIntoView({ block: "center" });
  }
  $effect(() => { void html; void source; void tick().then(() => scroll(hit.first_line)); });
</script>

<section class="document" bind:this={section} aria-label={hit.title} data-document-path={hit.path}>
  <header data-document-path={hit.path}><div><b>{doc?.title ?? hit.title}</b><small>{hit.path}</small></div>
    <button title={source ? "Показать Markdown" : "Показать исходник (только чтение)"} aria-label="Исходник / рендер" aria-pressed={source} onclick={() => source = !source}><Icon name="eye" size={15} /></button>
  </header>
  {#if error}<p role="alert">Не удалось прочитать документ: {error}</p>
  {:else if !doc}<p role="status">Чтение…</p>
  {:else if doc.missing}<p role="alert">Файл не найден, возможно, его удалили.</p><button onclick={() => { app.openSearch(); void app.search(); }}>Обновить выдачу</button>
  {:else}
    <div class="body" bind:this={body}>
      {#if source}<pre aria-label="Исходник Markdown, только чтение">{#each lines as line, i}<span data-line={i + 1} class:hit={i + 1 >= hit.first_line && i + 1 <= hit.last_line}>{line}{"\n"}</span>{/each}</pre>
      {:else}<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions --><div class="md" onclick={followLink}>{@html html}</div>{/if}
    </div>
    <footer class="backlinks" aria-label="Ссылаются на этот документ">
      {#if backlinksOpen}<!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_no_noninteractive_tabindex -->
        <div class="bl-grip" role="separator" aria-orientation="horizontal" aria-label="Высота панели ссылок" tabindex="0" onpointerdown={resizeBacklinks} onkeydown={resizeBacklinksByKey}></div>{/if}
      <button class="bl-head" aria-expanded={backlinksOpen} onclick={() => backlinksOpen = !backlinksOpen}>Ссылаются на этот документ{backlinks ? ` · ${backlinks.items.length}` : ""}</button>
      {#if backlinksOpen}<div class="bl-list" style:height="{backlinksHeight}px">
        {#if backlinksError}<p class="bl-note" role="alert">Не удалось получить входящие ссылки: {backlinksError}</p>
        {:else if !backlinks}<p class="bl-note" role="status">Чтение…</p>
        {:else}
          {#if backlinks.note}<p class="bl-note">{backlinks.note}</p>{/if}
          {#if backlinks.items.length}<ul>{#each backlinks.items as item (item.path)}<li><button onclick={() => openBacklink(item)} title={item.path}><span class="bl-title">{item.title}</span><small>{item.path}{item.lines.length ? ` · строки ${item.lines.join(", ")}` : ""}</small></button></li>{/each}</ul>
          {:else if !backlinks.note}<p class="bl-note">На этот документ никто не ссылается.</p>{/if}
        {/if}
      </div>{/if}
    </footer>
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
  .md :global(.a) { color: var(--accent-text); text-decoration: underline; text-underline-offset: 2px; text-decoration-thickness: 1px; }
  .md :global(span.a) { text-decoration: none; }
  .md :global(a.a) { cursor: pointer; }
  .md :global(pre) { overflow: auto; background: var(--panel-2); padding: 8px; } .md :global(table) { border-collapse: collapse; } .md :global(td), .md :global(th) { border: 1px solid var(--line); padding: 4px; }
  .md :global(.hit) { background: var(--accent-soft); border-left: 3px solid var(--accent); padding: 4px 8px; } .md :global(.hitlabel) { font-size: 10px; color: var(--accent-text); }
  .backlinks { border-top: 1px solid var(--line); background: var(--panel); position: relative; padding: 0 10px; }
  .bl-list { overflow: auto; padding-bottom: 6px; }
  .bl-grip { position: absolute; left: 0; right: 0; top: -3px; height: 6px; cursor: row-resize; touch-action: none; z-index: 1; } .bl-grip:hover, .bl-grip:focus-visible { background: var(--accent-soft); outline: none; }
  .bl-head { display: block; width: 100%; text-align: left; font-size: 11.5px; font-weight: 600; color: var(--text); padding: 4px 2px; }
  .backlinks ul { list-style: none; margin: 0; padding: 0; }
  .backlinks li button { display: block; width: 100%; text-align: left; padding: 3px 4px; color: var(--accent-text); }
  .bl-title { display: block; font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bl-note { margin: 2px; font-size: 11.5px; color: var(--muted); }
  .outline { position: absolute; right: 10px; top: 62px; width: 150px; max-height: 36%; overflow: auto; padding: 5px; border: 1px solid var(--line); border-radius: 6px; background: color-mix(in srgb, var(--panel) 90%, transparent); box-shadow: var(--shadow); }
  .outline button { display: block; width: 100%; text-align: left; font-size: 11px; padding: 2px 4px; } .outline .close { width: auto; margin-left: auto; } .outline-open { position: absolute; right: 10px; top: 62px; background: var(--panel-2); }
</style>
