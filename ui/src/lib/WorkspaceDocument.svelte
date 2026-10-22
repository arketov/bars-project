<script lang="ts">
  import { tick } from "svelte";
  import { call } from "./api";
  import Icon from "./Icon.svelte";
  import Editor from "./Editor.svelte";
  import { editors, type EditorBuffer } from "./editors.svelte";
  import { catalog } from "./keybindings";
  import { keybindings } from "./keybindings.svelte";
  import { app, type DocumentReference, type ViewedDocument } from "./app.svelte";
  import { BarsError, type ReadOutput } from "./types";
  import { documentHeadings, renderDocument, resolveDocumentLink } from "./markdown";
  import { loadBacklinks, type Backlink, type Backlinks } from "./backlinks";
  let { project, hit, initial }: { project: string; hit: DocumentReference; initial?: ViewedDocument } = $props();
  let doc = $state<ViewedDocument | null>(null), error = $state("");
  // Т-29: документ открывается в редакторе исходного Markdown; глаз переключает его на рендер.
  let source = $state(true), outline = $state(true);
  let body = $state<HTMLDivElement>();
  let editor = $state<ReturnType<typeof Editor>>();
  let buffer = $state<EditorBuffer | null>(null);
  const text = $derived(buffer?.text ?? doc?.text ?? "");
  const range = $derived(hit.first_line > 0 ? { first: hit.first_line, last: hit.last_line } : null);
  const html = $derived(doc && !source ? renderDocument(text, range ?? undefined) : "");
  const headings = $derived(documentHeadings(text));
  const status = $derived(buffer ? statusLabel(buffer) : "");
  // Панель разметки (Т-29): по умолчанию скрыта ради плотности окна; выбор запоминается.
  const TOOLBAR_KEY = "bars-editor-toolbar";
  let toolbar = $state(false), cheat = $state(false);
  try { toolbar = localStorage.getItem(TOOLBAR_KEY) === "1"; } catch { /* хранилище недоступно */ }
  function toggleToolbar() {
    toolbar = !toolbar;
    try { localStorage.setItem(TOOLBAR_KEY, toolbar ? "1" : "0"); } catch { /* хранилище недоступно */ }
  }
  const helpers: [string, string, string?][] = [
    ["editor.heading1", "H1"], ["editor.heading2", "H2"], ["editor.heading3", "H3"],
    ["editor.bold", "Ж", "bold"], ["editor.italic", "К", "italic"], ["editor.code", "‹›"], ["editor.codeBlock", "{ }"],
    ["editor.bulletList", "•"], ["editor.numberedList", "1."], ["editor.task", "☐"], ["editor.quote", "❝"],
    ["editor.table", "▦"], ["editor.link", "↗"],
  ];
  const helperTitle = (id: string) => {
    const hint = keybindings.hint(id);
    return `${catalog.find(c => c.id === id)?.title ?? id}${hint && hint !== "Не назначено" ? ` (${hint})` : ""}`;
  };
  function helper(id: string) { editor?.run(id); editor?.focus(); }
  function statusLabel(b: EditorBuffer): string {
    switch (b.status) {
      case "dirty": return "Изменено";
      case "saving": return "Запись…";
      case "saved": return b.savedAt ? `Записано в ${new Date(b.savedAt).toLocaleTimeString("ru-RU")}` : "Записано";
      case "conflict": return "Конфликт версий";
      case "error": return "Не записано";
      case "readonly": return "Только чтение";
      default: return "";
    }
  }
  $effect(() => {
    const current = doc, path = hit.path, root = project;
    if (!current || current.missing) { buffer = null; return; }
    const attached = editors.attach(root, path, current.text);
    buffer = attached;
    return () => { editors.detach(attached); };
  });
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
    if (source) { if (line > 0) editor?.scrollToLine(line); return; }
    body?.querySelector("#hit")?.scrollIntoView({ block: "center" });
  }
  // Прокрутка к отрывку при открытии, смене режима и нового отрывка; правки текста её не двигают.
  $effect(() => { void source; void hit.first_line; void doc; void tick().then(() => scroll(hit.first_line)); });
</script>

<section class="document" bind:this={section} aria-label={hit.title} data-document-path={hit.path}>
  <header data-document-path={hit.path}><div><b>{doc?.title ?? hit.title}</b><small>{hit.path}</small></div>
    {#if status}<span class="status" role="status" data-status={buffer?.status}>{status}</span>{/if}
    {#if source && buffer && buffer.status !== "readonly"}<button class="tool-toggle" title="Панель разметки" aria-label="Панель разметки" aria-pressed={toolbar} onclick={toggleToolbar}>Aa</button>{/if}
    <button title={source ? "Показать рендер" : "Показать исходник"} aria-label="Исходник / рендер" aria-pressed={source} onclick={() => { source = !source; void buffer?.save(); }}><Icon name="eye" size={15} /></button>
  </header>
  {#if error}<p role="alert">Не удалось прочитать документ: {error}</p>
  {:else if !doc}<p role="status">Чтение…</p>
  {:else if doc.missing}<p role="alert">Файл не найден, возможно, его удалили.</p><button onclick={() => { app.openSearch(); void app.search(); }}>Обновить выдачу</button>
  {:else}
    {#if buffer && (buffer.status === "error" || buffer.status === "conflict" || buffer.status === "readonly")}
      <p class="banner" role="alert" data-status={buffer.status}>{buffer.message}
        {#if buffer.status === "error"} Черновик остался в окне, он не записан. <button onclick={() => void buffer?.retry()}>Повторить</button>
        {:else if buffer.status === "conflict" && buffer.diskRevision === null} Правки остались в окне, пока вы не выберете.
          <button onclick={() => void buffer?.keepMine()}>Записать заново</button>
          <button onclick={() => void buffer?.takeDisk()}>Отбросить правки</button>
        {:else if buffer.status === "conflict"} Правки в окне не записываются, пока вы не выберете версию.
          <button onclick={() => void buffer?.keepMine()}>Оставить мою версию</button>
          <button onclick={() => void buffer?.takeDisk()}>Взять версию с диска</button>
          <button onclick={() => void buffer?.saveBeside()}>Сохранить мою рядом</button>{/if}
      </p>
    {/if}
    {#if buffer?.notice}<p class="banner notice" role="status">{buffer.notice} <button aria-label="Скрыть сообщение" onclick={() => { if (buffer) buffer.notice = ""; }}>×</button></p>{/if}
    {#if source && toolbar && buffer && buffer.status !== "readonly"}
      <div class="tools" role="toolbar" aria-label="Помощники разметки">
        {#each helpers as [id, text, style]}<button class={style} title={helperTitle(id)} aria-label={helperTitle(id)} onmousedown={(e) => e.preventDefault()} onclick={() => helper(id)}>{text}</button>{/each}
        <button class="help" title="Шпаргалка по синтаксису" aria-label="Шпаргалка по синтаксису" aria-expanded={cheat} onmousedown={(e) => e.preventDefault()} onclick={() => cheat = !cheat}>?</button>
      </div>
      {#if cheat}<div class="cheat" role="note"><code># Заголовок</code> · <code>**жирный**</code> · <code>*курсив*</code> · <code>`код`</code> · <code>- пункт</code> · <code>1. пункт</code> · <code>- [ ] задача</code> · <code>&gt; цитата</code> · <code>[текст](путь.md)</code> · <code>| а | б |</code> — таблица; <code>Tab</code> переходит по ячейкам, <code>Enter</code> продолжает список.</div>{/if}
    {/if}
    <div class="body" class:editing={source} bind:this={body}>
      {#if source}<Editor bind:this={editor} text={text} readonly={!buffer || buffer.status === "readonly" || buffer.status === "loading"} hit={range} oninput={(value) => buffer?.edit(value)} />
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
  .body.editing { padding: 0; overflow: hidden; }
  .tools { display: flex; flex-wrap: wrap; gap: 2px; padding: 3px 8px; border-bottom: 1px solid var(--line); }
  .tools button { min-width: 26px; font-size: 12px; } .tools .bold { font-weight: 700; } .tools .italic { font-style: italic; }
  .tool-toggle[aria-pressed="true"] { background: var(--accent-soft); color: var(--accent-text); } .tool-toggle { font-size: 11px; }
  .cheat { padding: 6px 10px; font-size: 11.5px; line-height: 1.7; background: var(--panel-2); border-bottom: 1px solid var(--line); } .cheat code { font: 11px var(--mono); }
  .status { font-size: 11px; color: var(--muted); white-space: nowrap; } .status[data-status="error"], .status[data-status="conflict"] { color: var(--danger, #b3261e); }
  .banner { margin: 0; padding: 6px 10px; font-size: 11.5px; background: var(--accent-soft); border-bottom: 1px solid var(--line); } .banner button { color: var(--accent-text); text-decoration: underline; }
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
