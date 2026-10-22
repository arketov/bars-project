<script lang="ts">
  // Мини-граф — часть навигации (OQ-117): та же физическая панель, что во вкладке «Граф», только карта.
  import { app } from "./app.svelte";
  import PhysicalGraph from "./graph/PhysicalGraph.svelte";
</script>

<div class="mini" aria-label="Мини-граф">
  {#if app.project && app.project.state !== "unavailable" && !app.workerDown}
    <PhysicalGraph />
  {/if}
</div>

<style>
  .mini { display: flex; flex-direction: column; flex: 1; min-height: 0; overflow: hidden; }
</style>
