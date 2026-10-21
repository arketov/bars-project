<script lang="ts">
  import { onMount } from "svelte";
  import { app, type ViewedDocument } from "./app.svelte";
  import { inTauri } from "./api";
  import type { PanelParams } from "./workspace-layout";
  import DocumentPanel from "./WorkspaceDocument.svelte";
  import SettingsPage from "./SettingsPage.svelte";
  import LogsPage from "./LogsPage.svelte";
  import McpStatisticsPage from "./McpStatisticsPage.svelte";
  import IndexingPage from "./IndexingPage.svelte";
  import NewTopicsPage from "./NewTopicsPage.svelte";
  import WorkerDown from "./WorkerDown.svelte";
  import EmptyProjects from "./EmptyProjects.svelte";
  import Missing from "./Missing.svelte";
  let { model }: { model: { params: PanelParams; initial?: ViewedDocument; foreground: boolean; reserveFooter: boolean; attach: (target: HTMLElement, kind: string) => { destroy(): void } } } = $props();
  function attach(target: HTMLElement, kind: string) { return model.attach(target, kind); }
  const project = $derived(app.project);
  const kind = $derived(model.params.kind);
  let graphTools = $state(false);
  onMount(() => { if (model.params.kind === "indexing") void app.loadIndexingHistory(); });
</script>

<div class="workspace-content" class:reserve-footer={model.reserveFooter} data-panel-kind={kind} data-foreground={model.foreground}>
  {#if kind === "navigation" || kind === "search"}<div class="shared-host" use:attach={kind}></div>
  {:else if kind === "settings"}<SettingsPage embedded />
  {:else if kind === "logs"}<div class="scroll"><LogsPage /></div>
  {:else if kind === "mcp"}<div class="scroll"><McpStatisticsPage /></div>
  {:else if kind === "document" && model.params.hit && model.params.project}
    <DocumentPanel project={model.params.project} hit={model.params.hit} initial={model.initial} />
  {:else if kind === "indexing" && project}<IndexingPage />
  {:else if app.workerDown}<WorkerDown message={app.workerDown} />
  {:else if app.projectsLoaded && app.projects.length === 0}<EmptyProjects />
  {:else if !project}<div></div>
  {:else if project.state === "unavailable"}<Missing {project} />
  {:else if kind === "topics"}<NewTopicsPage />
  {:else if kind === "graph"}
    <div class="graph-shell" class:compact={!graphTools}>
    <button class="graph-options" aria-expanded={graphTools} onclick={() => graphTools = !graphTools}>{graphTools ? "Скрыть параметры" : "Параметры графа"}</button>
    {#if !inTauri() && !graphTools}<small class="fixture">Демонстрационные данные</small>{/if}
    {#await import("./GraphPage.svelte")}<p role="status">Загрузка графа…</p>
    {:then graph}<graph.default />
    {:catch error}<p role="alert">Не удалось загрузить граф: {String(error)}</p>{/await}
    </div>
  {/if}
</div>

<style>
  .workspace-content { height: 100%; min-width: 0; min-height: 0; display: flex; flex-direction: column; overflow: hidden; }
  .reserve-footer { padding-bottom: 38px; }
  .workspace-content :global(aside), .workspace-content :global(main), .workspace-content :global(.viewer), .workspace-content :global(.topics-page) { flex: 1; min-width: 0; min-height: 0; }
  .scroll { overflow: auto; height: 100%; padding: 12px; }
  .shared-host { display: flex; flex: 1; min-height: 0; min-width: 0; }
  .graph-shell { display: flex; flex-direction: column; flex: 1; min-height: 0; overflow: auto; }
  .graph-options { align-self: flex-end; border: 0; background: transparent; color: var(--muted); font-size: 11px; padding: 3px 8px; }
  .fixture { font-size: 10px; color: var(--muted); padding: 0 6px; }
  .compact :global(.graph-page) { padding: 0; gap: 0; flex: 1; overflow: hidden; }
  .compact :global(.graph-page > :not(.graph-workspace):not(.graph-error)) { display: none; }
  /* Полноэкранный граф на узком окне задаёт flex:none!important и высоту карты 380px. */
  .compact :global(.graph-workspace) { flex: 1 !important; min-height: 0; grid-template-columns: minmax(0, 1fr); grid-template-rows: minmax(0, 1fr); overflow: hidden; border: 0; border-radius: 0; }
  .compact :global(.graph-map) { height: auto; min-height: 0; } .compact :global(.graph-inspector), .compact :global(.graph-legend) { display: none; }
  .compact :global(.graph-canvas) { inset: 34px 0 0; }
</style>
