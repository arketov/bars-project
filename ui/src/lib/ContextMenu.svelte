<script lang="ts">
  import { onMount, tick } from "svelte";
  import { app } from "./app.svelte";
  import { call, copyText, documentAction, documentEditors, folderTarget, folderAction, type FolderTarget } from "./api";
  import { documentAt, documentItems, folderAt, folderItems, itemsFor, place, projectAt, projectItems, run, textField, topicAt, treeEntryAt, type MenuItem } from "./contextmenu";
  import type { TopicDetail } from "./new-topics";
  import AppearancePicker from "./AppearancePicker.svelte";
  import { canStyleFolder, keyOf } from "./appearance";
  import { findNode, relativeToDocs } from "./tree";
  import { editorMenu, menuTargetAt, treeMenu } from "./menu-model";
  import { executeTreeCommand, type TreeCommandTarget } from "./tree-commands";
  import { keybindings } from "./keybindings.svelte";
  const menuId = $props.id();

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
  let treeTarget = $state<TreeCommandTarget | null>(null);
  let reason = $state('');
  // Т-38: цель выбора цвета и иконки, найденная при открытии меню, и открытая панель выбора.
  let styleTarget: { kind: "folder" | "document"; path: string; label: string } | null = null;
  let picker = $state<{ project: string; kind: "folder" | "document"; path: string; label: string; x: number; y: number } | null>(null);
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
    if (treeTarget && (current !== treeTarget.project || app.docsRoot !== treeTarget.docsRoot || !findNode(app.tree, treeTarget.path))) close();
    if (topic && (current !== topic.project || !app.topicsOpen || !app.topics.topics.some(t => t.id === topic?.id))) close();
    if (pendingFolder && (current !== pendingFolder.project || app.docsRoot !== pendingFolder.docsRoot)) close();
    if (document && current !== document.project) close();
    if (picker && current !== picker.project) picker = null;
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
    treeTarget = null;
    reason = '';
    styleTarget = null;
    submenu = false;
    generation++;
  }

  async function position() {
    await tick();
    if (box) at = place(origin.x, origin.y, box.offsetWidth, box.offsetHeight, innerWidth, innerHeight);
  }

  $effect(() => {
    // Строка причины меняет высоту меню у нижнего края окна.
    const description = reason;
    if (box && items.length && description) void position();
  });

  async function open(event: Pick<MouseEvent, 'target' | 'preventDefault' | 'clientX' | 'clientY'>) {
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
    const nearest = menuTargetAt(event.target);
    const directory = !textField(event.target) && nearest?.hasAttribute("data-folder-path") && folderAt(event.target);
    const entry = !textField(event.target) && current && nearest?.hasAttribute('data-tree-node') ? treeEntryAt(nearest) : null;
    if (directory && current) {
      // Контекст ОС закрепляет бэкенд; запоздавшая проверка не воскрешает меню.
      returnFocus = directory.element;
      pendingFolder = { project: current, docsRoot };
      try {
        const pinned = entry && (app.workerDown || !app.project) ? null : await folderTarget(current, docsRoot, directory.path);
        if (generation !== version || app.current !== current || app.docsRoot !== docsRoot) return;
        pendingFolder = null;
        folder = pinned ? { ...pinned, project: current, docsRoot, kind: directory.kind } : null;
        if (!entry && pinned) found.unshift(...folderItems(app.project?.state ?? "unavailable", pinned.missing));
        const key = keyOf(directory.path);
        if (canStyleFolder(key)) {
          styleTarget = { kind: "folder", path: key, label: key.split("/").pop() ?? key };
          if (!entry) found.push({ action: "appearance", label: "Цвет и иконка…" });
        }
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
    const file = !folder && !project && !textField(event.target) && nearest?.hasAttribute('data-document-path') && documentAt(nearest);
    if (file && app.current) {
      document = { ...file, project: app.current };
      if (!entry) found.unshift(...documentItems());
      const key = keyOf(relativeToDocs(file.path, docsRoot));
      styleTarget = { kind: "document", path: key, label: key.split("/").pop() ?? key };
      if (!entry) found.push({ action: "appearance", label: "Цвет…" });
    }
    if (entry && current && nearest) {
      treeTarget = { project: current, docsRoot, path: entry.path, element: nearest, pinned: folder ?? undefined };
      returnFocus = nearest;
      found.splice(0, found.length, ...treeMenu({ ...entry, state: app.project?.state ?? 'starting',
        workerDown: !!app.workerDown, missing: folder?.missing, canStyle: !!styleTarget,
        copySelection: !!itemsFor(event.target).length, mac: /Mac/.test(navigator.platform) }, id => keybindings.binding(id) ? keybindings.hint(id) : ''));
    }
    if (!found.length) return close();
    target = event.target;
    at = { left: event.clientX, top: event.clientY };
    items = found;
    origin = { x: event.clientX, y: event.clientY };
    await position();
    if (version !== generation) return;
    box?.querySelector<HTMLButtonElement>('button:not([aria-disabled="true"]):not(.danger)')?.focus({ preventScroll: true });
  }

  async function showEditors() {
    if (!document) return;
    const version = generation;
    try {
      const found = await documentEditors();
      if (version !== generation) return;
      items = items.map(item => item.action === 'editors' ? { ...item, children: editorMenu(found) } : item);
      submenu = true;
      await position();
      box?.querySelector<HTMLButtonElement>(".sub button")?.focus({ preventScroll: true });
    } catch (error) { app.flash(String(error), true); }
  }

  async function pick(item: MenuItem) {
    if (item.disabled) return;
    if (item.action === "editors") { await showEditors(); return; }
    if (treeTarget && (item.id || item.action === 'editor') && item.action !== 'copy') {
      const captured = treeTarget;
      close(true);
      try { await executeTreeCommand(item.id ?? 'target.openExternal', captured, item.editor); }
      catch (error) { app.flash(String(error instanceof Error ? error.message : error), true); }
      return;
    }
    if (item.action === "appearance" && styleTarget && app.current) {
      const style = styleTarget, owner = app.current;
      close();
      picker = { ...style, project: owner, x: origin.x, y: origin.y };
      return;
    }
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
      } else if (file && ["open", "newWindow", "editor", "reveal", "path", "check"].includes(item.action)) {
        if (app.current !== file.project) return;
        if (item.action === "newWindow") {
          await app.openInNewWindow(file.path);
        } else if (item.action === "open") {
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
    if (event.key === 'Escape' && submenu) {
      event.preventDefault(); submenu = false;
      box?.querySelector<HTMLButtonElement>('[aria-haspopup="menu"]')?.focus(); void position(); return;
    }
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
    if (event.key === "Escape" || event.key === "Tab") { event.preventDefault(); close(true); return; }
    if (event.key === "ArrowRight" && globalThis.document.activeElement?.getAttribute('aria-haspopup') === 'menu') {
      event.preventDefault();
      if (globalThis.document.activeElement.getAttribute('aria-disabled') !== 'true') void showEditors();
      return;
    }
    if (event.key === "ArrowLeft" && submenu) {
      event.preventDefault(); submenu = false;
      box.querySelector<HTMLButtonElement>('[aria-haspopup="menu"]')?.focus();
      void position(); return;
    }
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const buttons = Array.from(box.querySelectorAll<HTMLButtonElement>(submenu ? ".sub button" : ".primary button"));
    const index = buttons.indexOf(globalThis.document.activeElement as HTMLButtonElement);
    const next = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1
      : (index + (event.key === "ArrowUp" ? -1 : 1) + buttons.length) % buttons.length;
    buttons[next]?.focus();
  }

  onMount(() => {
    const chooseEditor = async (event: Event) => {
      const captured = (event as CustomEvent<TreeCommandTarget>).detail;
      const rect = captured.element.getBoundingClientRect();
      const opening = open({ target: captured.element, preventDefault() {}, clientX: rect.left, clientY: rect.bottom });
      const version = generation;
      await opening;
      if (version === generation && treeTarget?.project === captured.project && treeTarget.path === captured.path) await showEditors();
    };
    const style = (event: Event) => {
      const target = (event as CustomEvent<TreeCommandTarget>).detail;
      const rect = target.element.getBoundingClientRect();
      const kind = target.element.dataset.treeKind === 'file' ? 'document' : 'folder';
      close();
      picker = { project: target.project, kind, path: keyOf(target.path), label: target.path.split('/').pop()!, x: rect.left, y: rect.bottom };
    };
    addEventListener('bars-tree-style', style);
    addEventListener('bars-tree-editors', chooseEditor);
    const onKey = navigate;
    const onScroll = (event: Event) => {
      if (!(event.target instanceof Node) || !box?.contains(event.target)) close();
    };
    addEventListener("contextmenu", open);
    addEventListener("keydown", onKey);
    const dismiss = () => close();
    const dialogClosed = (event: Event) => {
      // Отложенный close списка команд не закрывает уже открытое им меню дерева.
      if (event.target instanceof Node && (box ? event.target.contains(box) : !!returnFocus && event.target.contains(returnFocus))) close();
    };
    addEventListener("blur", dismiss);
    addEventListener("resize", dismiss);
    addEventListener("close", dialogClosed, true);
    addEventListener("scroll", onScroll, true);
    return () => {
      removeEventListener('bars-tree-style', style);
      removeEventListener('bars-tree-editors', chooseEditor);
      removeEventListener("contextmenu", open);
      removeEventListener("keydown", onKey);
      removeEventListener("blur", dismiss);
      removeEventListener("resize", dismiss);
      removeEventListener("close", dialogClosed, true);
      removeEventListener("scroll", onScroll, true);
    };
  });
</script>

<svelte:window onmousedown={(event) => {
  if (!(event.target instanceof Node)) return close();
  if (box ? !box.contains(event.target) : returnFocus && !returnFocus.contains(event.target)) close();
}} />

{#if picker}
  <AppearancePicker kind={picker.kind} path={picker.path} label={picker.label} x={picker.x} y={picker.y} onclose={() => (picker = null)} />
{/if}

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
      {#each items as item, index (item.action)}
        {#if index > 0 && item.group && item.group !== items[index - 1].group}<div class="separator" role="separator"></div>{/if}
        <button role="menuitem" class:danger={item.danger} aria-disabled={item.disabled ? 'true' : undefined} aria-describedby={item.disabled ? `${menuId}-${index}` : undefined} title={item.hint}
          aria-haspopup={item.action === "editors" ? "menu" : undefined} aria-expanded={item.action === "editors" ? submenu : undefined}
          onfocus={() => reason = item.disabled ? item.hint ?? '' : ''} onmouseenter={() => reason = item.disabled ? item.hint ?? '' : ''} onmouseleave={() => reason = ''}
          onclick={() => pick(item)}><span>{item.label}{#if item.hint && !item.disabled}<small>{item.hint}</small>{/if}</span><span class="shortcut">{item.shortcut ?? ''}{item.action === "editors" ? " ▸" : ''}</span></button>
        {#if item.disabled && item.hint}<span class="sr-only" id={`${menuId}-${index}`}>{item.hint}</span>{/if}
      {/each}
      {#if reason}<div class="reason">Недоступно: {reason}</div>{/if}
    </div>
    {#if submenu}
      <div class="sub" role="menu" aria-label="Открыть в редакторе">
        {#each items.find(item => item.action === 'editors')?.children ?? [] as editor (editor.editor)}
          <button role="menuitem" onclick={() => pick(editor)}>{editor.label}</button>
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
  .cmenu button { display: flex; align-items: center; justify-content: space-between; gap: 16px; width: 100%; text-align: left; border: 0; background: none; padding: 4px 8px; border-radius: 6px; font-size: 12px; color: var(--text); }
  .cmenu button:hover, .cmenu button:focus-visible { background: var(--accent-soft); color: var(--accent-text); }
  .cmenu button.danger { color: var(--bad); }
  .cmenu button.danger:hover, .cmenu button.danger:focus-visible { background: var(--bad-soft); color: var(--bad); }
  .cmenu button[aria-disabled="true"] { color: var(--muted); opacity: .7; cursor: default; }
  .cmenu button[aria-disabled="true"]:hover, .cmenu button[aria-disabled="true"]:focus-visible { background: var(--panel-2); color: var(--muted); }
  .separator { height: 1px; background: var(--line); margin: 4px 2px; }
  .shortcut { font-size: 11px; color: var(--muted); white-space: nowrap; }
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .reason { font-size: 11px; line-height: 1.5; color: var(--muted); padding: 6px 8px 4px; border-top: 1px solid var(--line); margin-top: 4px; }
  .cmenu small { display: block; font-size: 11px; color: var(--muted); }
</style>
