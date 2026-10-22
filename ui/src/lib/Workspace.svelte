<script lang="ts">
  import { onMount, mount, unmount, untrack, tick } from "svelte";
  import { createDockview, DefaultTab, themeLight, themeDark, type DockviewApi, type IDockviewPanel } from "dockview-core";
  import "dockview/dist/styles/dockview.css";
  import { app, type ViewedDocument } from "./app.svelte";
  import { theme } from "./theme.svelte";
  import WorkspacePanel from "./WorkspacePanel.svelte";
  import SidebarStatus from "./SidebarStatus.svelte";
  import Sidebar from "./Sidebar.svelte";
  import SearchSurface from "./SearchSurface.svelte";
  import GlobalSearch from "./GlobalSearch.svelte";
  import MiniGraph from "./MiniGraph.svelte";
  import Icon from "./Icon.svelte";
  import { decodeLayout, defaultLayout, defaultSidebar, documentId, encodeLayout, ensureRightZone, layoutKey, removableEmptyGroups, panelTitles, type PanelKind, type PanelParams } from "./workspace-layout";
  import { messages } from "./workspace-messages";
  import CommandPalette from './CommandPalette.svelte';
  import { workspaceCommands } from './workspace-commands';
  import { keybindings } from './keybindings.svelte';
  import UnsavedDialog from './UnsavedDialog.svelte';
  import { confirmClose, startFlushListener } from './unsaved.svelte';

  let host: HTMLDivElement, api: DockviewApi;
  let ready = $state(false), menu = $state(false), active = $state("search");
  let globalSearch: GlobalSearch, parking: HTMLDivElement, searchSurface: HTMLDivElement, navigationSurface: HTMLDivElement;
  let column: HTMLDivElement;
  let globalOpen = false;
  let palette: CommandPalette;
  let commands: ReturnType<typeof workspaceCommands> | undefined;
  let resizing = $state<{ id: string; width: number; height: number } | null>(null);
  let resizeControl = $state<HTMLDivElement>(null!);
  // Постоянная левая колонка: навигация, мини-граф, статус (OQ-117).
  let sidebar = $state({ ...defaultSidebar });
  const clampWidth = (width: number) => Math.round(Math.max(220, Math.min(width, Math.max(220, window.innerWidth - 420))));
  const clampGraph = (height: number) => Math.round(Math.max(80, Math.min(height, Math.max(80, (column?.clientHeight ?? 600) - 38 - 160))));
  function dragSplit(kind: "width" | "graph", event: PointerEvent) {
    if (event.button !== 0) return;
    const handle = event.currentTarget as HTMLElement;
    event.preventDefault();
    try { handle.setPointerCapture(event.pointerId); } catch { /* указатель уже отпущен */ }
    const start = { x: event.clientX, y: event.clientY, width: sidebar.width, graph: sidebar.graph };
    const move = (e: PointerEvent) => {
      if (kind === "width") sidebar.width = clampWidth(start.width + e.clientX - start.x);
      else sidebar.graph = clampGraph(start.graph - (e.clientY - start.y));
    };
    const up = () => { handle.removeEventListener("pointermove", move); handle.removeEventListener("pointerup", up); handle.removeEventListener("pointercancel", up); persist(); };
    handle.addEventListener("pointermove", move); handle.addEventListener("pointerup", up); handle.addEventListener("pointercancel", up);
  }
  function splitKey(kind: "width" | "graph", event: KeyboardEvent) {
    const step: Record<string, number> = kind === "width" ? { ArrowLeft: -20, ArrowRight: 20 } : { ArrowUp: 20, ArrowDown: -20 };
    const delta = step[event.key]; if (!delta) return;
    event.preventDefault();
    if (kind === "width") sidebar.width = clampWidth(sidebar.width + delta); else sidebar.graph = clampGraph(sidebar.graph + delta);
    persist();
  }
  function resizeGroup(id: string) {
    const panel = api.getPanel(id); if (!panel) return;
    const rect = panel.group.element.getBoundingClientRect();
    resizing = { id, width: rect.width, height: rect.height };
    void tick().then(() => resizeControl.focus());
  }
  function resizeKey(event: KeyboardEvent) {
    if (!resizing) return;
    const panel = api.getPanel(resizing.id); if (!panel) { resizing = null; return; }
    if (event.key === 'Escape') { event.preventDefault(); panel.group.api.setSize({ width: resizing.width, height: resizing.height }); resizing = null; panel.group.element.focus(); return; }
    if (event.key === 'Enter') { event.preventDefault(); resizing = null; panel.group.element.focus(); return; }
    if (!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(event.key)) return;
    event.preventDefault();
    const rect = panel.group.element.getBoundingClientRect();
    panel.group.api.setSize(event.key === 'ArrowLeft' || event.key === 'ArrowRight' ? { width: Math.max(220, rect.width + (event.key === 'ArrowRight' ? 20 : -20)) } : { height: Math.max(120, rect.height + (event.key === 'ArrowDown' ? 20 : -20)) });
  }
  const destinations = new Map<string, HTMLElement>();
  const surfaceOf = (kind: string) => kind === "search" ? searchSurface : navigationSurface;
  function placeSurfaces() {
    for (const kind of ["search", "navigation"] as const) {
      const surface = surfaceOf(kind);
      if (surface) (destinations.get(kind) ?? parking).append(surface);
    }
  }
  function attach(target: HTMLElement, kind: string) {
    destinations.set(kind, target);
    const surface = surfaceOf(kind);
    if (!globalOpen && surface) target.append(surface);
    return { destroy() {
      if (destinations.get(kind) !== target) return;
      destinations.delete(kind);
      const surface = surfaceOf(kind);
      if (!globalOpen && surface) parking.append(surface);
    } };
  }
  function globalChanged(opened: boolean) {
    globalOpen = opened;
    if (!opened) placeSurfaces();
  }
  let current: string | null = null, restoring = false;
  let incoming: ViewedDocument | undefined;
  let presented: ViewedDocument | null = null;
  let storageError = false;
  let previousView = "main", previousTab = "models", previousNavigation = 0;
  let previousTopics = 0;
  type PanelModel = { params: PanelParams; initial?: ViewedDocument; foreground: boolean; attach: typeof attach };
  const models = new Map<string, PanelModel>();
  let foreground = "search", documents = $state<{ id: string; title: string }[]>([]);
  function refreshPanels() {
    app.indexingOpen = !!api.getPanel("indexing");
    documents = api.panels.filter(p => p.params?.kind === "document").map(p => ({ id: p.id, title: p.title ?? p.id }));
    if (!api.getPanel(foreground)) foreground = api.getPanel("search") ? "search" : api.panels[0]?.id ?? "";
    models.forEach((model, id) => { model.foreground = id === foreground; });
  }
  function persist() {
    if (restoring || !api) return;
    try { localStorage.setItem(layoutKey(current), encodeLayout(api, sidebar)); storageError = false; }
    catch { if (!storageError) app.flash("Не удалось сохранить раскладку", true); storageError = true; }
  }
  function restore(project: string | null) {
    globalSearch?.close(false);
    restoring = true;
    current = project;
    try {
      const text = localStorage.getItem(layoutKey(project));
      const stored = text ? decodeLayout(text, project) : null;
      if (stored) { api.fromJSON(stored.layout); sidebar = { width: clampWidth(stored.sidebar.width), graph: clampGraph(stored.sidebar.graph) }; }
      else { defaultLayout(api, host.clientWidth); sidebar = { ...defaultSidebar }; }
    } catch {
      defaultLayout(api, host.clientWidth); sidebar = { ...defaultSidebar };
      app.flash("Раскладка повреждена или устарела; восстановлена штатная", true);
    } finally {
      api.getPanel("settings")?.api.close(); // вкладка настроек из прежних раскладок
      ensureRightZone(api, host.clientWidth);
      restoring = false; active = api.activePanel?.id ?? "";
    }
  }
  function open(kind: Exclude<PanelKind, "document">) {
    if (kind === "settings") { menu = false; return; } // настройки — отдельный экран в App.svelte, не вкладка
    if (kind === "topics") app.topicsOpen = true;
    let panel = api.getPanel(kind);
    if (!panel) panel = api.addPanel({ id: kind, component: "workspace", title: panelTitles[kind], params: { kind }, position: api.getPanel("search") ? { referencePanel: "search", direction: "within" } : api.groups[0] ? { referenceGroup: api.groups[0], direction: "within" } : undefined });
    panel.api.setActive();
    menu = false;
  }
  function navigate(kind: Exclude<PanelKind, "document">) {
    if (kind === "indexing") app.openIndexing();
    else if (kind === "topics") app.openTopics();
    else if (kind === "settings") app.openSettings();
    else if (kind === "logs" || kind === "mcp") app.openSettings(kind === "logs" ? "logs" : "mcp-statistics");
    open(kind);
  }
  /** Группа для документа: где уже есть документы, иначе самая правая группа без поиска. */
  function documentGroup() {
    const existing = api.panels.find(p => p.params?.kind === "document");
    if (existing) return existing.group;
    const search = api.getPanel("search")?.group;
    return api.groups.filter(g => g !== search).sort((a, b) => b.element.getBoundingClientRect().left - a.element.getBoundingClientRect().left)[0];
  }
  function documentPanel(doc: ViewedDocument) {
    if (!current) return;
    incoming = doc;
    const { path, title, first_line, last_line } = doc.hit;
    const id = documentId(path), params: PanelParams = { kind: "document", project: current, hit: { path, title, first_line, last_line } };
    let panel = api.getPanel(id);
    if (panel) panel.api.updateParameters(params);
    else {
      const target = documentGroup();
      panel = api.addPanel({ id, component: "workspace", title: doc.title, params,
        position: target ? { referenceGroup: target, direction: "within" } : api.getPanel("search") ? { referencePanel: "search", direction: "right" } : undefined,
        initialWidth: Math.max(250, host.clientWidth / 2) });
    }
    incoming = undefined;
    panel.api.setActive();
    persist();
  }
  function reset() { restoring = true; defaultLayout(api, host.clientWidth); sidebar = { ...defaultSidebar }; restoring = false; persist(); menu = false; }
  function move(position: "left" | "right" | "top" | "bottom") {
    const panel = api.activePanel;
    const target = api.panels.find(p => p.group !== panel?.group) ?? api.panels.find(p => p !== panel);
    if (panel && target) panel.api.moveTo({ group: target.group, position });
    menu = false;
  }
  // Реактивный мост старых действий в панели. Сами панели не зависят от app.view.
  $effect(() => {
    const project = app.current;
    if (ready) untrack(() => { if (project !== current) { persist(); restore(project); } });
  });
  $effect(() => {
    const view = app.view, tab = app.settingsTab, navigation = app.navigationVersion;
    if (ready) untrack(() => {
      if (view !== previousView || tab !== previousTab || navigation !== previousNavigation) {
        const presentingDocument = app.viewer && app.viewer !== presented;
        previousView = view; previousTab = tab; previousNavigation = navigation;
        if (!presentingDocument) {
          globalSearch?.close(false);
          open(view === "main" ? "search" : view === "settings" ? tab === "logs" ? "logs" : tab === "mcp-statistics" ? "mcp" : "settings" : view);
        }
      }
    });
  });
  $effect(() => { const doc = app.viewer; if (ready && doc) untrack(() => {
    presented = doc;
    const focus = globalOpen;
    if (focus) globalSearch.close(false);
    documentPanel(doc);
    if (focus) void tick().then(() => {
      const section = [...host.querySelectorAll<HTMLElement>('section[data-document-path]')].find(node => node.dataset.documentPath === doc.hit.path);
      const target = section?.querySelector<HTMLElement>('header');
      if (target) { target.tabIndex = -1; target.focus({ preventScroll: true }); }
    });
  }); });
  $effect(() => { const version = app.topicsVersion; if (ready) untrack(() => { if (version !== previousTopics) { previousTopics = version; if (app.topicsOpen) open("topics"); else api.getPanel("topics")?.api.close(); } }); });
  $effect(() => { const mode = theme.mode; if (ready) api.updateOptions({ theme: mode === "dark" ? themeDark : themeLight }); });
  $effect(() => { if (app.consentOpen || app.deletingProject) untrack(() => globalSearch?.close(false)); });
  $effect(() => { if (ready && app.models && !keybindings.available) untrack(() => { void keybindings.load(); }); });

  onMount(() => {
    let disposed = false, stopKeys: (() => void) | undefined;
    void keybindings.start().then(stop => { if (disposed) stop(); else stopKeys = stop; }).catch(error => { keybindings.error = String(error); });
    let stopFlush: (() => void) | undefined;
    void startFlushListener().then(stop => { if (disposed) stop(); else stopFlush = stop; }).catch(error => app.flash(String(error), true));
    api = createDockview(host, {
      theme: theme.mode === "dark" ? themeDark : themeLight, messages,
      defaultRenderer: "always", disableFloatingGroups: true,
      // HTML5 drag-and-drop в WebView2 перехватывает обработчик файлов Tauri (ISS-21).
      dndStrategy: "pointer",
      // Зоны не исчезают при переносе последней вкладки; лишние пустые группы убирает ensureRightZone.
      closeEmptyGroups: false,
      defaultTabComponent: "workspace-tab",
      createTabComponent: () => {
        const tab = new DefaultTab();
        return { element: tab.element, init(params) {
          tab.init(params); tab.element.dataset.workspacePanel = params.api.id;
          if (params.params.kind !== "document") return;
          const path: string = params.params.hit.path;
          tab.element.dataset.documentPath = path;
          // НФТ-5: вкладку с несохранёнными правками не закрываем молча; обычное закрытие Dockview отменяется.
          tab.element.querySelector(".dv-default-tab-action")?.addEventListener("click", (event) => {
            event.stopImmediatePropagation(); event.preventDefault();
            void confirmClose(app.current, path).then(ok => { if (ok) params.api.close(); });
          }, true);
        }, dispose() { tab.dispose(); } };
      },
      createComponent: () => {
        const element = document.createElement("div"); element.style.height = "100%";
        let component: ReturnType<typeof mount>;
        const model = $state<PanelModel>({ params: { kind: "search" }, foreground: false, attach });
        let id: string;
        return { element,
          init: ({ params, api: panel }) => { id = panel.id; models.set(id, model); model.params = params as unknown as PanelParams; model.initial = incoming; model.foreground = id === foreground; component = mount(WorkspacePanel, { target: element, props: { model } }); },
          update: ({ params }) => { model.params = params as unknown as PanelParams; model.initial = incoming; },
          dispose: () => { models.delete(id); if (component) void unmount(component); },
        };
      },
    });
    restore(app.current);
    placeSurfaces();
    foreground = api.activePanel?.id ?? "search";
    refreshPanels();
    let normalizing = false;
    const subscriptions = [api.onDidLayoutChange(() => {
      if (!restoring && !normalizing && (api.groups.length === 1 || removableEmptyGroups(api).length > 0)) {
        normalizing = true;
        try { ensureRightZone(api, host.clientWidth); } finally { normalizing = false; }
      }
      persist(); refreshPanels();
    }), api.onDidRemovePanel(panel => { if (!restoring && panel.id === "topics") app.topicsOpen = false; }), api.onDidActivePanelChange(event => {
      const panel: IDockviewPanel | undefined = event.panel;
      active = panel?.id ?? "";
      if (panel) foreground = panel.id;
      refreshPanels();
    })];
    ready = true;
    void tick().then(() => { if (!disposed) commands = workspaceCommands({ api, host, open: navigate, search: () => globalSearch.open(), reset, palette, resize: resizeGroup }); });
    return () => { disposed = true; stopKeys?.(); stopFlush?.(); commands?.dispose(); palette.close(false); globalSearch.close(false); ready = false; persist(); subscriptions.forEach(s => s.dispose()); api.dispose(); app.indexingOpen = false; };
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions: обработчик навигации дочерних ARIA-вкладок. -->
<div class="workspace" data-active-panel={active} onkeydown={(event) => {
  if (event.key === "Escape" && menu) { event.preventDefault(); menu = false; return; }
  const tab = event.target instanceof HTMLElement ? event.target.closest('[role="tab"]') : null;
  if (!tab || !["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
  const tabs = [...tab.closest('[role="tablist"]')!.querySelectorAll<HTMLElement>('[role="tab"]')];
  const index = tabs.indexOf(tab as HTMLElement);
  const next = event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1 : (index + (event.key === "ArrowRight" ? 1 : -1) + tabs.length) % tabs.length;
  event.preventDefault();
  const id = tabs[next]?.querySelector<HTMLElement>('[data-workspace-panel]')?.dataset.workspacePanel;
  if (id) api.getPanel(id)?.api.setActive();
  tabs[next]?.focus();
}} role="region" aria-label="Рабочая область">
  <nav aria-label="Панели рабочего окна">
    <div class="workspace-brand"><span class="logo">b</span><b>bars</b></div>
    <button class:active={active === "search"} onclick={() => { app.openSearch(); open("search"); }}>Поиск</button>
    <button class:active={active === "graph"} onclick={() => open("graph")}>Граф</button>
    {#if documents.length}<button class="narrow-documents" onclick={() => api.getPanel(documents.at(-1)!.id)?.api.setActive()}>Документы</button>{/if}
    <div class="panels-menu"><button aria-expanded={menu} onclick={() => menu = !menu}>Панели ▾</button>
      {#if menu}<div class="menu">
        {#each Object.entries(panelTitles) as [kind, title]}<button onclick={() => navigate(kind as Exclude<PanelKind, "document">)}>{title}</button>{/each}
        <hr />
        <button onclick={() => move("left")}>Активную панель влево</button><button onclick={() => move("right")}>Активную панель вправо</button>
        <button onclick={() => move("top")}>Активную панель вверх</button><button onclick={() => move("bottom")}>Активную панель вниз</button>
        <hr /><button onclick={() => commands?.execute('layout.reset')}>Сбросить раскладку</button>
      </div>{/if}
    </div>
    <button aria-label="Список действий" aria-keyshortcuts={keybindings.aria('commands.open')} title={`Список действий · ${keybindings.hint('commands.open')}`} onclick={() => commands?.show()}>Действия</button>
    <button class="global-launch" aria-label="Глобальный поиск" aria-keyshortcuts={keybindings.aria('search.open')} title={`Глобальный поиск · ${keybindings.hint('search.open')}`} onclick={() => commands?.execute('search.open')}><Icon name="search" size={16} /></button>
  </nav>
  <div class="body">
    <div class="column" bind:this={column} style:width={`${sidebar.width}px`} role="complementary" aria-label="Навигация">
      <div class="tree-host" use:attach={"navigation"}></div>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex: фокусируемый separator — виджет ARIA. -->
      <div class="split split-graph" role="separator" aria-orientation="horizontal" aria-label="Высота мини-графа" aria-valuenow={sidebar.graph} aria-valuemin={80} tabindex="0" onpointerdown={(e) => dragSplit("graph", e)} onkeydown={(e) => splitKey("graph", e)}></div>
      <div class="mini-graph" style:height={`${sidebar.graph}px`}><MiniGraph /></div>
      <SidebarStatus />
    </div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex: фокусируемый separator — виджет ARIA. -->
    <div class="split split-width" role="separator" aria-orientation="vertical" aria-label="Ширина навигации" aria-valuenow={sidebar.width} aria-valuemin={220} tabindex="0" onpointerdown={(e) => dragSplit("width", e)} onkeydown={(e) => splitKey("width", e)}></div>
    <div class="dock" bind:this={host}></div>
  </div>
</div>
<div hidden bind:this={parking}>
  <div class="shared-surface" bind:this={searchSurface}><SearchSurface /></div>
  <div class="shared-surface" bind:this={navigationSurface}><Sidebar /></div>
</div>
<GlobalSearch bind:this={globalSearch} search={() => searchSurface} tree={() => navigationSurface} changed={globalChanged} />
<CommandPalette bind:this={palette} />
<UnsavedDialog />
{#if resizing}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="resize-control" bind:this={resizeControl} role="dialog" aria-label="Размер группы" data-command-blocked="true" tabindex="0" onkeydown={resizeKey}>Стрелки — размер группы · Enter — принять · Escape — отменить <button onclick={() => resizing = null}>Готово</button></div>
{/if}

<style>
  .resize-control { position:fixed; bottom:14px; left:50%; transform:translateX(-50%); z-index:100; padding:10px; background:var(--panel); color:var(--text); border:1px solid var(--line); font-size:12px; }
  .workspace { height: 100vh; display: flex; flex-direction: column; min-width: 0; }
  .shared-surface { display:flex; flex-direction:column; flex:1; width:100%; height:100%; min-width:0; min-height:0; }
  .shared-surface :global(main), .shared-surface :global(aside) { flex:1; width:100%; min-height:0; min-width:0; }
  nav .global-launch { margin-left:auto; display:flex; }
  nav { display: flex; align-items: center; gap: 12px; padding: 5px 12px; border-bottom: 1px solid var(--line); }
    .workspace-brand { display: flex; align-items: center; gap: 7px; margin-right: 12px; color: var(--text); font-size: 14px; }
    .logo { width: 24px; height: 24px; display: grid; place-items: center; border-radius: 7px; background: var(--accent); color: var(--on-accent); font-size: 15px; font-weight: 800; }
  nav button { padding: 0; border: 0; background: transparent; color: var(--muted); font-size: 12px; font-style: italic; } nav button:hover, nav button.active { color: var(--accent-text); }
  .body { flex: 1; min-height: 0; display: flex; }
  .column { flex: none; display: flex; flex-direction: column; min-height: 0; background: var(--panel); }
  .tree-host { flex: 1; min-height: 0; display: flex; }
  .tree-host :global(aside) { border-right: 0; }
  .mini-graph { flex: none; min-height: 0; display: flex; flex-direction: column; overflow: hidden; }
  .split { flex: none; background: var(--line); touch-action: none; }
  .split:hover, .split:focus-visible { background: var(--accent); outline: none; }
  .split-graph { height: 1px; cursor: row-resize; position: relative; }
  .split-width { width: 1px; cursor: col-resize; position: relative; }
  /* Узкая линия, широкая зона захвата. */
  .split::after { content: ""; position: absolute; z-index: 5; }
  .split-graph::after { left: 0; right: 0; top: -3px; bottom: -3px; }
  .split-width::after { top: 0; bottom: 0; left: -3px; right: -3px; }
  .dock { flex: 1; min-height: 0; min-width: 0; }
  .panels-menu { position: relative; } .menu { position: absolute; top: 24px; left: 0; z-index: 50; min-width: 190px; padding: 5px; border: 1px solid var(--line); border-radius: 8px; background: var(--panel); box-shadow: var(--shadow); }
  .menu button { display: block; width: 100%; padding: 4px 8px; text-align: left; font-style: normal; } .menu button:hover { background: var(--accent-soft); }
  .dock :global(.dockview-theme-light), .dock :global(.dockview-theme-dark) {
    --dv-background-color: var(--bg); --dv-group-view-background-color: var(--panel); --dv-tabs-and-actions-container-background-color: var(--panel-2);
    --dv-activegroup-visiblepanel-tab-background-color: var(--panel); --dv-activegroup-visiblepanel-tab-color: var(--accent-text);
    --dv-activegroup-hiddenpanel-tab-background-color: var(--panel-2); --dv-activegroup-hiddenpanel-tab-color: var(--muted);
    --dv-inactivegroup-visiblepanel-tab-background-color: var(--panel); --dv-inactivegroup-visiblepanel-tab-color: var(--text);
    --dv-inactivegroup-hiddenpanel-tab-background-color: var(--panel-2); --dv-inactivegroup-hiddenpanel-tab-color: var(--muted);
    --dv-separator-border: var(--line); --dv-tabs-container-scrollbar-color: var(--line-2); --dv-tabs-and-actions-container-height: 28px;
    --dv-tab-font-size: 12px; --dv-drag-over-background-color: var(--accent-soft); --dv-drag-over-border-color: var(--accent);
  }
  .dock :global(.dv-tab) { font-style: italic; }
  .narrow-documents { display: none; }
  @media (max-width: 1100px) {
    /* Overlay Dockview содержит contain:layout paint: расширять нужно его, а не вложенный документ. */
    .narrow-documents { display: inline-block; }
    .dock :global(.dv-render-overlay:has(.workspace-content)) { left: 0 !important; top: 28px !important; width: 100% !important; height: calc(100% - 28px) !important; z-index: 10; background: var(--bg); }
    .dock :global(.dv-render-overlay:has(.workspace-content[data-foreground="false"])) { display: none !important; }
  }
</style>
