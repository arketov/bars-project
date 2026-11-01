<script lang="ts">
  import Sidebar from "../../lib/Sidebar.svelte";
  import SearchPane from "../../lib/SearchPane.svelte";
  import Viewer from "../../lib/Viewer.svelte";
  import GraphTab from "../../lib/GraphTab.svelte";
  let { kind }: { kind: string } = $props();
  let draft = $state("Проверка сохранности ввода при переносе");
</script>

<div class="probe-panel" data-kind={kind}>
  {#if kind === "tree"}<Sidebar />
  {:else if kind === "search"}<SearchPane />
  {:else if kind === "document"}<Viewer />
  {:else if kind === "graph"}<GraphTab />
  {:else}<label>Черновик стенда (без записи файла)<textarea bind:value={draft}></textarea></label>{/if}
</div>

<style>
  .probe-panel { height: 100%; min-width: 0; min-height: 0; display: flex; overflow: auto; }
  .probe-panel :global(aside), .probe-panel :global(main), .probe-panel :global(.viewer) { flex: 1; width: 100%; min-width: 0; }
  label { padding: 12px; } textarea { display: block; width: 230px; height: 100px; }
</style>
