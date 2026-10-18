<script lang="ts">
  import { onMount, tick } from "svelte";
  import { app } from "./app.svelte";
  import { call, copyText, documentAction, documentEditors, type DocumentEditor } from "./api";
  import { documentAt, documentItems, itemsFor, place, projectAt, projectItems, run, textField, type MenuItem } from "./contextmenu";

  let items = $state<MenuItem[]>([]);
  let target: EventTarget | null = null;
  let at = $state({ left: 0, top: 0 });
  let box = $state<HTMLDivElement>();
  let document: { project: string; path: string; line: number } | null = null;
  let project = $state<string | null>(null);
  let openedCurrent: string | null = null;
  let editors = $state<DocumentEditor[]>([]);
  let submenu = $state(false);
  let generation = 0;
  let origin = { x: 0, y: 0 };

  $effect(() => {
    const current = app.current;
    if (document && current !== document.project) close();
    if (project && (current !== openedCurrent || !app.projects.some(p => p.path === project))) close();
  });

  function close() {
    items = [];
    target = null;
    document = null;
    project = null;
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
    const found = itemsFor(event.target);
    const path = !textField(event.target) && projectAt(event.target);
    if (path && app.projects.some(p => p.path === path)) {
      project = path;
      openedCurrent = app.current;
      found.unshift(...projectItems());
    }
    const file = !project && !textField(event.target) && documentAt(event.target);
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
    const projectPath = project;
    close();
    try {
      if (projectPath && item.action === "deleteProject") {
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
    } catch (error) { app.flash(error instanceof Error ? error.message : String(error), true); }
  }

  function navigate(event: KeyboardEvent) {
    if ((event.shiftKey && event.key === "F10") || event.key === "ContextMenu") {
      const element = globalThis.document.activeElement;
      if (element instanceof HTMLElement && projectAt(element)) {
        event.preventDefault();
        const rect = element.getBoundingClientRect();
        element.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: rect.left, clientY: rect.bottom }));
        return;
      }
    }
    if (!items.length || !box) return;
    if (event.key === "Escape" || event.key === "Tab") { close(); return; }
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
    addEventListener("blur", close);
    addEventListener("resize", close);
    addEventListener("scroll", onScroll, true);
    return () => {
      removeEventListener("contextmenu", open);
      removeEventListener("keydown", onKey);
      removeEventListener("blur", close);
      removeEventListener("resize", close);
      removeEventListener("scroll", onScroll, true);
    };
  });
</script>

<svelte:window onmousedown={(event) => box && (!(event.target instanceof Node) || !box.contains(event.target)) && close()} />

{#if items.length}
  <div
    class="cmenu"
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
  .primary { min-width: 215px; }
  .sub { min-width: 210px; border-left: 1px solid var(--line); margin-left: 4px; padding-left: 4px; }
  @media (max-width: 480px) { .cmenu { flex-direction: column; } .sub { border-left: 0; border-top: 1px solid var(--line); margin: 4px 0 0; padding: 4px 0 0; } }
  .cmenu button { display: block; width: 100%; text-align: left; border: 0; background: none; padding: 7px 10px; border-radius: 7px; font-size: 13px; color: var(--text); }
  .cmenu button:hover, .cmenu button:focus-visible { background: var(--accent-soft); color: var(--accent-text); }
  .cmenu button:disabled { color: var(--muted); cursor: default; background: none; }
  .cmenu small { display: block; font-size: 11px; color: var(--muted); }
</style>
