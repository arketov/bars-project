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
  function keyboard(event: KeyboardEvent) {
    if (event.defaultPrevented || event.isComposing || event.ctrlKey || event.metaKey || event.altKey || event.target instanceof HTMLInputElement) return;
    const row = event.currentTarget as HTMLElement;
    const rows = [...(row.closest('.tree')?.querySelectorAll<HTMLElement>('.row') ?? [])].filter(item => !!item.getClientRects().length);
    const index = rows.indexOf(row);
    if (['ArrowDown','ArrowUp','Home','End'].includes(event.key)) {
      event.preventDefault();
      rows[event.key === 'Home' ? 0 : event.key === 'End' ? rows.length-1 : Math.max(0,Math.min(rows.length-1,index+(event.key==='ArrowDown'?1:-1)))]?.focus();
    } else if (event.key === 'ArrowRight' && isDir) {
      event.preventDefault(); if (!open) opened=true; else row.parentElement?.querySelector<HTMLElement>('.kids .row')?.focus();
    } else if (event.key === 'ArrowLeft') {
      event.preventDefault(); if (isDir && open && !forceOpen) opened=false; else row.parentElement?.parentElement?.closest('.node')?.querySelector<HTMLElement>(':scope > .row')?.focus();
    } else if (event.key === ' ' && app.searchActive) { event.preventDefault(); app.toggleNode(node.path,check!=='all'); }
    else if (event.key === 'Enter') { event.preventDefault(); if (isDir) opened=!opened; else if (node.documentPath) void app.openDocument({path:node.documentPath,title:node.label,first_line:0,last_line:0}); }
  }
</script>

<div class="node {node.kind}" class:open>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex: строка получает фокус для ContextMenu/Shift+F10. -->
  <div class="row" data-document-path={node.documentPath} data-folder-path={isDir ? node.path : undefined} data-folder-kind={node.path === "." ? "Корневые документы" : node.kind === "module" ? "Модуль" : "Папка"} tabindex="0" role="treeitem" aria-selected={!!node.documentPath && app.viewer?.hit.path === node.documentPath} aria-expanded={isDir ? open : undefined} aria-label={title} onkeydown={keyboard}>
    {#if isDir}
      <button class="caret" aria-label={open ? "Свернуть" : "Развернуть"} onclick={() => (opened = !opened)}>
        <Icon name="chevron-right" size={10} width={2} />
      </button>
    {:else}
      <span class="spacer"></span>
    {/if}
    <label {title} class:directory={isDir}>
      {#if app.searchActive}
      <input
        type="checkbox"
        aria-label={title}
        checked={check === "all"}
        indeterminate={check === "some"}
        onchange={(event) => app.toggleNode(node.path, event.currentTarget.checked)}
      />
      {/if}
      {#if isDir}<Icon name="folder" size={14} /><span class="nm">{node.path === "." ? "Корневые документы" : node.label}</span>{/if}
    </label>
    {#if !isDir && node.documentPath}
      <button class="nm filename" title={node.documentPath} onclick={() => app.openDocument({ path: node.documentPath!, title: node.label, first_line: 0, last_line: 0 })}>{node.label}</button>
    {/if}
    <span class="ct">{node.chunks}</span>
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
  .row:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .caret { width: 18px; height: 18px; border: 0; background: none; display: grid; place-items: center; color: var(--muted); transition: transform .15s; padding: 0; flex: none; }
  .open > .row .caret { transform: rotate(90deg); }
  label { display: flex; align-items: center; gap: 8px; cursor: pointer; }
  label.directory { flex: 1; min-width: 0; }
  .filename { border: 0; background: none; text-align: left; color: inherit; padding: 0; cursor: pointer; }
  .filename:hover { color: var(--accent-text); }
  .nm { flex: 1; font-size: 13px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .module > .row .nm { font-weight: 600; }
  .dir > .row .nm { color: var(--muted); font-weight: 600; }
  .ct { color: var(--faint); font-size: 11.5px; font-variant-numeric: tabular-nums; }
  .kids { margin-left: 15px; padding-left: 6px; border-left: 1px solid var(--line); }
  .spacer { width: 18px; flex: none; }
</style>
