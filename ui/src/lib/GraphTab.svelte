<script lang="ts">
  // Вкладка «Граф»: физическая карта проекта. Анализ связей (пути, влияние, контекст, связи из поиска) остаётся прежним
  // экраном до этапа 2.3, где его действия переносятся в новую панель (ADR-0004).
  import { app } from "./app.svelte";
  import PhysicalGraph from "./graph/PhysicalGraph.svelte";
  const analysis = $derived(app.graphAnalysis || !!app.graphPreview);
  function backToMap() {
    app.graphAnalysis = false;
    app.graphPreview = null;
  }
</script>

{#if analysis}
  <div class="analysis">
    <div class="back">
      <button onclick={backToMap}>← Карта графа</button>
      <span>Анализ связей: пути, влияние, контекст и связи из поиска</span>
    </div>
    <div class="body">
      {#await import("./GraphPage.svelte")}<p role="status">Загрузка анализа…</p>
      {:then page}<page.default />
      {:catch error}<p role="alert">Не удалось загрузить анализ: {String(error)}</p>{/await}
    </div>
  </div>
{:else}
  <PhysicalGraph />
{/if}

<style>
  .analysis { display: flex; flex-direction: column; flex: 1; min-height: 0; }
  .back { display: flex; align-items: center; gap: 10px; padding: 4px 10px; border-bottom: 1px solid var(--line); font-size: 11.5px; color: var(--muted); }
  .back button { border: 1px solid var(--line-2); background: transparent; border-radius: 6px; padding: 2px 8px; color: var(--text); font-size: 11.5px; }
  .body { display: flex; flex-direction: column; flex: 1; min-height: 0; overflow: auto; }
  p { margin: 8px; font-size: 11px; color: var(--muted); }
</style>
