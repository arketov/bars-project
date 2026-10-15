<script lang="ts">
  import { app } from "./app.svelte";
  import Icon from "./Icon.svelte";
  import { checkState, type TreeNode } from "./tree";
  import TreeRow from "./TreeRow.svelte";

  let { node, forceOpen = false }: { node: TreeNode; forceOpen?: boolean } = $props();

  let opened = $state(false);
  const isDir = $derived(node.kind !== "file");
  const open = $derived(forceOpen || opened);
  const check = $derived(checkState(node, app.selected));
  const title = $derived(node.path === "." ? "корень документации" : isDir ? `${node.path}/` : node.path.replace(/^\.\//, ""));
</script>

<div class="node {node.kind}" class:open>
  <div class="row">
    {#if isDir}
      <button class="caret" aria-label={open ? "Свернуть" : "Развернуть"} onclick={() => (opened = !opened)}>
        <Icon name="chevron-right" size={10} width={2} />
      </button>
    {:else}
      <span class="spacer"></span>
    {/if}
    <label {title}>
      <input
        type="checkbox"
        checked={check === "all"}
        indeterminate={check === "some"}
        onchange={(event) => app.toggleNode(node.path, event.currentTarget.checked)}
      />
      <span class="nm">{node.label}</span>
      <span class="ct">{node.chunks}</span>
    </label>
  </div>
  {#if isDir && open}
    <div class="kids">
      {#each node.children as child (child.path)}
        <TreeRow node={child} {forceOpen} />
      {/each}
    </div>
  {/if}
</div>

<style>
  .row { display: flex; align-items: center; gap: 4px; padding: 4px 6px; border-radius: 8px; user-select: none; }
  .row:hover { background: var(--panel-2); }
  .caret { width: 18px; height: 18px; border: 0; background: none; display: grid; place-items: center; color: var(--muted); transition: transform .15s; padding: 0; flex: none; }
  .open > .row .caret { transform: rotate(90deg); }
  label { flex: 1; display: flex; align-items: center; gap: 8px; min-width: 0; cursor: pointer; }
  .nm { flex: 1; font-size: 13px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .module > .row .nm { font-weight: 600; }
  .dir > .row .nm { color: var(--muted); font-weight: 600; }
  .ct { color: var(--faint); font-size: 11.5px; font-variant-numeric: tabular-nums; }
  .kids { margin-left: 15px; padding-left: 6px; border-left: 1px solid var(--line); }
  .spacer { width: 18px; flex: none; }
</style>
