<script lang="ts">
  // Мини-граф — часть навигации (OQ-117): тот же GraphPage, что во вкладке «Граф», только карта.
  import { app } from "./app.svelte";
</script>

<div class="mini" aria-label="Мини-граф">
  {#if app.project && app.project.state !== "unavailable" && !app.workerDown}
    {#await import("./GraphPage.svelte")}<p role="status">Загрузка графа…</p>
    {:then graph}<graph.default />
    {:catch error}<p role="alert">Не удалось загрузить граф: {String(error)}</p>{/await}
  {/if}
</div>

<style>
  .mini { display: flex; flex-direction: column; flex: 1; min-height: 0; overflow: hidden; }
  p { margin: 8px; font-size: 11px; color: var(--muted); }
  .mini :global(.graph-page) { padding: 0; gap: 0; flex: 1; overflow: hidden; }
  .mini :global(.graph-page > :not(.graph-workspace):not(.graph-error)) { display: none; }
  /* Полноэкранный граф на узком окне задаёт flex:none!important и высоту карты 380px. */
  .mini :global(.graph-workspace) { flex: 1 !important; min-height: 0; grid-template-columns: minmax(0, 1fr); grid-template-rows: minmax(0, 1fr); overflow: hidden; border: 0; border-radius: 0; }
  .mini :global(.graph-map) { height: auto; min-height: 0; } .mini :global(.graph-inspector), .mini :global(.graph-legend) { display: none; }
  .mini :global(.graph-canvas) { inset: 34px 0 0; }
</style>
