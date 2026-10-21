<script lang="ts">
  import { onMount, tick } from "svelte";
  import { app } from "./app.svelte";
  import { call, copyText, documentAction, documentEditors, folderTarget, folderAction, type FolderTarget, type DocumentEditor } from "./api";
  import { documentAt, documentItems, folderAt, folderItems, itemsFor, place, projectAt, projectItems, run, textField, topicAt, type MenuItem } from "./contextmenu";
  import type { TopicDetail } from "./new-topics";

  let items = $state<MenuItem[]>([]);
  let target: EventTarget | null = null;
  let at = $state({ left: 0, top: 0 });
  let box = $state<HTMLDivElement>();
  let document: { project: string; path: string; line: number } | null = null;
  let folder = $state<(FolderTarget & { project: string; docsRoot: string; kind: string }) | null>(null);
  let pendingFolder = $state<{ project: string; docsRoot: string } | null>(null);
  let returnFocus: HTMLElement | null = null;
  let project = $state<string | null>(null);
  let topic = $state<{ project: string; id: number } | null>(null);
  let openedCurrent: string | null = null;
  let editors = $state<DocumentEditor[]>([]);
  let submenu = $state(false);
  let generation = 0;
  let origin = { x: 0, y: 0 };
  // Меню внутри showModal должно принадлежать тому же верхнему слою WebView.
  function modalHost(node: HTMLElement) {
    const dialog = globalThis.document.querySelector('dialog[open]');
    if (dialog) dialog.append(node);
  }

  $effect(() => {
    const current = app.current;
    if (topic && (current !== topic.project || !app.topicsOpen || !app.topics.topics.some(t => t.id === topic?.id))) close();
    if (pendingFolder && (current !== pendingFolder.project || app.docsRoot !== pendingFolder.docsRoot)) close();
    if (document && current !== document.project) close();
    if (folder && (current !== folder.project || app.docsRoot !== folder.docsRoot || !app.projects.some(p => p.path === folder?.project))) close();
    if (project && (current !== openedCurrent || !app.projects.some(p => p.path === project))) close();
  });

  function close(restoreFocus = false) {
    if (restoreFocus && returnFocus?.isConnected) returnFocus.focus({ preventScroll: true });
    items = [];
    target = null;
    document = null;
    folder = null;
    pendingFolder = null;
    returnFocus = null;
    project = null;
    topic = null;
    submenu = false;
    generation++;
  }

  async function position() {
    await tick();
    if (box) at = place(origin.x, origin.y, box.offsetWidth, box.offsetHeight, innerWidth, innerHeight);
  }

  async function open(event: MouseEvent) {
    // Стандартное меню webview выключено всегда: своё — только там, где есть что показать.
    event.preventDefault();
    close();
    const version = generation;
    const current = app.current, docsRoot = app.docsRoot;
    const found = itemsFor(event.target);
    const row = topicAt(event.target);
    if (row && current && app.topicsOpen && app.topics.topics.some(t => t.id === row.id)) {
      topic = { project: current, id: row.id };
      returnFocus = row.element;
      found.splice(0, found.length,
        { action: "copyTopic", label: "Скопировать" },
        { action: "closeTopic", label: "Закрыть", disabled: app.topics.busy });
    }
    const nearest = event.target instanceof Element ? event.target.closest("[data-document-path], [data-folder-path], [data-project-path]") : null;
    const directory = !textField(event.target) && nearest?.hasAttribute("data-folder-path") && folderAt(event.target);
    if (directory && current) {
      // Контекст ОС закрепляет бэкенд; запоздавшая проверка не воскрешает меню.
      returnFocus = directory.element;
      pendingFolder = { project: current, docsRoot };
      try {
        const pinned = await folderTarget(current, docsRoot, directory.path);
        if (generation !== version || app.current !== current || app.docsRoot !== docsRoot) return;
        pendingFolder = null;
        folder = { ...pinned, project: current, docsRoot, kind: directory.kind };
        found.unshift(...folderItems(app.project?.state ?? "unavailable", pinned.missing));
      } catch (error) {
        if (generation === version && app.current === current) { close(); app.flash(String(error), true); }
        return;
      }
    }
    const path = !folder && !textField(event.target) && nearest?.hasAttribute("data-project-path") && projectAt(event.target);
    if (path && app.projects.some(p => p.path === path)) {
      project = path;
      openedCurrent = app.current;
      found.unshift(...projectItems());
    }
    const file = !folder && !project && !textField(event.target) && documentAt(event.target);
    if (file && app.current) {
      document = { ...file, project: app.current };
      found.unshift(...documentItems());
    }
    if (!found.length) return close();
    target = event.target;
    at = { left: event.clientX, top: event.clientY };
    items = found;
    origin = { x: event.clientX, y: event.clientY };
    await position();
    if (version !== generation) return;
    box?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus({ preventScroll: true });
  }

  async function showEditors() {
    if (!document) return;
    const version = generation;
    try {
      const found = await documentEditors();
      if (version !== generation) return;
      editors = [{ id: "default", label: "Программа по умолчанию" }, ...found];
      submenu = true;
      await position();
      box?.querySelector<HTMLButtonElement>(".sub button")?.focus({ preventScroll: true });
    } catch (error) { app.flash(String(error), true); }
  }

  async function pick(item: MenuItem) {
    if (item.disabled) return;
    if (item.action === "editors") { await showEditors(); return; }
    const where = target;
    const file = document;
    const directory = folder;
    const projectPath = project;
    const selectedTopic = topic;
    close(true);
    try {
      if (selectedTopic) {
        if (app.current !== selectedTopic.project || !app.topicsOpen) return;
        if (item.action === "copyTopic") {
          const detail = await call<TopicDetail>("new_topics_get", { project: selectedTopic.project, id: selectedTopic.id });
          // Название можно переименовать: копируем сохранённый запрос, а не заголовок строки.
          if (app.current !== selectedTopic.project || !app.topicsOpen) return;
          const query = detail.variants_list[0]?.text;
          if (query === undefined) throw new Error("Исходный запрос темы не найден");
          await copyText(query);
          app.flash("Запрос скопирован");
        } else if (item.action === "closeTopic" && !app.topics.busy) {
          if (await app.topics.remove(selectedTopic.id)) app.flash("Тема закрыта и удалена");
        }
      } else if (directory && ["reveal", "path", "checkProject"].includes(item.action)) {
        const action = item.action === "reveal" ? "open" : item.action === "checkProject" ? "check" : "path";
        const result = await folderAction(directory.id, action);
        if (action === "path") { await copyText(result.absolute); app.flash(result.missing ? "Путь скопирован; папка отсутствует" : "Путь скопирован"); }
        if (action === "check") app.flash(`Проверка проекта в очереди · №${result.operation_id}`);
      } else if (projectPath && item.action === "deleteProject") {
        app.requestDeleteProject(projectPath);
      } else if (projectPath && item.action === "checkProject") {
        if (!app.projects.some(p => p.path === projectPath)) return;
        await call("reindex", { cwd: projectPath, project: projectPath });
        app.flash("Проверка проекта поставлена в очередь");
      } else if (file && ["open", "editor", "reveal", "path", "check"].includes(item.action)) {
        if (app.current !== file.project) return;
        if (item.action === "open") {
          const hit = app.results?.results.find(hit => hit.path === file.path && hit.first_line === file.line);
          await app.openDocument(hit ?? { path: file.path, title: file.path, first_line: 0, last_line: 0 });
        } else {
          const action = item.action === "editor" ? "open" : item.action as "path" | "check" | "reveal";
          const path = await documentAction(file.project, file.path, action, item.editor);
          if (action === "path") { await copyText(path); app.flash("Путь скопирован"); }
          if (action === "check") app.flash("Проверка документа поставлена в очередь");
        }
      } else {
        const problem = await run(item.action, where);
        if (problem) app.flash(problem, true);
      }
    } catch (error) { app.flash(`${directory ? `${directory.kind}: ${directory.absolute} — ` : ""}${error instanceof Error ? error.message : String(error)}`, true); }
  }

  function navigate(event: KeyboardEvent) {
    if (event.key === "Escape" && (returnFocus || items.length)) { event.preventDefault(); close(true); return; }
    if (event.key === "Tab" && returnFocus && !items.length) { close(); return; }
    if ((event.shiftKey && event.key === "F10") || event.key === "ContextMenu") {
      const element = globalThis.document.activeElement;
      if (element instanceof HTMLElement && (projectAt(element) || folderAt(element) || documentAt(element) || topicAt(element))) {
        event.preventDefault();
        const rect = element.getBoundingClientRect();
        element.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: rect.left, clientY: rect.bottom }));
        return;
      }
    }
    if (!items.length || !box) return;
    if (event.key === "Escape" || event.key === "Tab") { close(event.key === "Escape"); return; }
    if (event.key === "ArrowRight") { event.preventDefault(); void showEditors(); return; }
    if (event.key === "ArrowLeft" && submenu) {
      event.preventDefault(); submenu = false;
      box.querySelector<HTMLButtonElement>('[aria-haspopup="menu"]')?.focus();
      void position(); return;
    }
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const buttons = Array.from(box.querySelectorAll<HTMLButtonElement>(submenu ? ".sub button:not(:disabled)" : ".primary button:not(:disabled)"));
    const index = buttons.indexOf(globalThis.document.activeElement as HTMLButtonElement);
    const next = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1
      : (index + (event.key === "ArrowUp" ? -1 : 1) + buttons.length) % buttons.length;
    buttons[next]?.focus();
  }

  onMount(() => {
    const onKey = navigate;
    const onScroll = (event: Event) => {
      if (!(event.target instanceof Node) || !box?.contains(event.target)) close();
    };
    addEventListener("contextmenu", open);
    addEventListener("keydown", onKey);
    const dismiss = () => close();
    addEventListener("blur", dismiss);
    addEventListener("resize", dismiss);
    addEventListener("close", dismiss, true);
    addEventListener("scroll", onScroll, true);
    return () => {
      removeEventListener("contextmenu", open);
      removeEventListener("keydown", onKey);
      removeEventListener("blur", dismiss);
      removeEventListener("resize", dismiss);
      removeEventListener("close", dismiss, true);
      removeEventListener("scroll", onScroll, true);
    };
  });
</script>

<svelte:window onmousedown={(event) => {
  if (!(event.target instanceof Node)) return close();
  if (box ? !box.contains(event.target) : returnFocus && !returnFocus.contains(event.target)) close();
}} />

{#if items.length}
  <div
    class="cmenu"
    use:modalHost
    bind:this={box}
    role="presentation"
    tabindex="-1"
    style="left: {at.left}px; top: {at.top}px"
    onmousedown={(event) => event.preventDefault()}
  >
    <div class="primary" role="menu" aria-label="Действия">
      {#each items as item (item.action)}
        <button role="menuitem" disabled={item.disabled} aria-haspopup={item.action === "editors" ? "menu" : undefined} aria-expanded={item.action === "editors" ? submenu : undefined} onclick={() => pick(item)}>{item.label}{item.action === "editors" ? " ▸" : ""}{#if item.hint}<small>{item.hint}</small>{/if}</button>
      {/each}
    </div>
    {#if submenu}
      <div class="sub" role="menu" aria-label="Открыть в редакторе">
        {#each editors as editor (editor.id)}
          <button role="menuitem" onclick={() => pick({ action: "editor", label: editor.label, editor: editor.id })}>{editor.label}</button>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .cmenu { position: fixed; z-index: 90; display: flex; max-width: calc(100vw - 8px); max-height: calc(100vh - 8px); overflow: auto; background: var(--panel); border: 1px solid var(--line-2); border-radius: 10px; box-shadow: 0 10px 30px rgba(0,0,0,.2); padding: 5px; }
  .primary { min-width: 180px; }
  .sub { min-width: 180px; border-left: 1px solid var(--line); margin-left: 4px; padding-left: 4px; }
  @media (max-width: 480px) { .cmenu { flex-direction: column; } .sub { border-left: 0; border-top: 1px solid var(--line); margin: 4px 0 0; padding: 4px 0 0; } }
  .cmenu button { display: block; width: 100%; text-align: left; border: 0; background: none; padding: 4px 8px; border-radius: 5px; font-size: 12px; color: var(--text); }
  .cmenu button:hover, .cmenu button:focus-visible { background: var(--accent-soft); color: var(--accent-text); }
  .cmenu button:disabled { color: var(--muted); cursor: default; background: none; }
  .cmenu small { display: block; font-size: 11px; color: var(--muted); }
</style>
