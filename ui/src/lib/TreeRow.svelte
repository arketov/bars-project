<script lang="ts">
  import { app } from "./app.svelte";
  import Icon from "./Icon.svelte";
  import FolderIcon from "./FolderIcon.svelte";
  import { appearance } from "./appearance.svelte";
  import { colorVar, documentColor, folderColor, folderIcon, keyOf } from "./appearance";
  import { treeCommandAdapter } from "./tree-commands";
  import { checkState, type TreeNode } from "./tree";
  import TreeRow from "./TreeRow.svelte";

  let { node, forceOpen = false }: { node: TreeNode; forceOpen?: boolean } = $props();

  let opened = $state(false);
  const isDir = $derived(node.kind !== "file");
  const open = $derived(forceOpen || opened);
  const check = $derived(checkState(node, app.selected));
  /** Т-38: цвет и иконка из `.bars/appearance.toml`; у документа цвет — свой или ближайшей папки. */
  const styleKey = $derived(keyOf(node.path));
  const color = $derived(colorVar(isDir ? folderColor(appearance.data, styleKey) : documentColor(appearance.data, styleKey)));
  const icon = $derived(isDir ? folderIcon(appearance.data, styleKey) : null);
  const title = $derived(node.path === "." ? "корень документации" : isDir ? `${node.path}/` : node.path.replace(/^\.\//, ""));
  /** Т-32: имя папки работает как файл — открывает её Index.md и раскрывает папку; клик по чекбоксу отмечает область поиска. */
  function openFolder() {
    if (node.indexPath) void app.openDocument({ path: node.indexPath, title: node.label, first_line: 0, last_line: 0 }, true);
    if (!open) opened = true;
  }
  function folderClick(event: MouseEvent) {
    event.preventDefault();
    openFolder();
  }
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
    } else if (event.key === ' ') { event.preventDefault(); app.toggleNode(node.path,check!=='all'); }
    else if (event.key === 'Enter') { event.preventDefault(); if (isDir) { if (node.indexPath) openFolder(); else opened=!opened; } else if (node.documentPath) void app.openDocument({path:node.documentPath,title:node.label,first_line:0,last_line:0}); }
  }
</script>

<div class="node {node.kind}" class:open>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex: строка получает фокус для ContextMenu/Shift+F10. -->
  <div class="row" use:treeCommandAdapter={node.path} data-menu-kind={isDir ? "folder" : "file"} data-menu-place="tree" data-document-path={node.documentPath} data-folder-path={isDir ? node.path : undefined} data-tree-node={node.path} data-tree-kind={node.kind} data-folder-kind={node.path === "." ? "Корневые документы" : node.kind === "module" ? "Модуль" : "Папка"} tabindex="0" role="treeitem" aria-selected={!!node.documentPath && app.viewer?.hit.path === node.documentPath} aria-expanded={isDir ? open : undefined} aria-label={title} onkeydown={keyboard}>
    {#if isDir}
      <button class="caret" aria-label={open ? "Свернуть" : "Развернуть"} onclick={() => (opened = !opened)}>
        <Icon name="chevron-right" size={10} width={2} />
      </button>
    {:else}
      <span class="spacer"></span>
    {/if}
    <label {title} class:directory={isDir} class:nested-folder={!!node.nested}>
      {#if !node.nested}
        <input
          type="checkbox"
          aria-label={title}
          checked={check === "all"}
          indeterminate={check === "some"}
          onchange={(event) => app.toggleNode(node.path, event.currentTarget.checked)}
        />
      {/if}
      {#if isDir}<span class="fi" style:color={color}><FolderIcon name={icon} size={14} /></span><!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions: клавиатура — Enter на строке. --><span class="nm folder" onclick={folderClick}>{node.path === "." ? "Корневые документы" : node.label}</span>{/if}
    </label>
    {#if !isDir && node.documentPath}
      {#if color}<span class="dot" style:background={color} aria-hidden="true"></span>{/if}
      <button class="nm filename" title={node.documentPath} onclick={() => app.openDocument({ path: node.documentPath!, title: node.label, first_line: 0, last_line: 0 })}>{node.label}</button>
    {/if}
    {#if node.nested}
      <span class="nested-mark" data-testid="nested-mark">вложенный проект</span>
      <button class="nested-open" onclick={() => void app.select(node.nested!.path)}>Открыть проект</button>
    {/if}
    <span class="ct" title={node.indexed === false ? "Ещё не в индексе" : undefined}>{node.indexed === false ? "—" : node.chunks}</span>
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
  label .folder:hover { color: var(--accent-text); }
  .fi { display: inline-flex; flex: none; }
  .dot { width: 8px; height: 8px; border-radius: 50%; flex: none; margin-right: 2px; }
  .filename { border: 0; background: none; text-align: left; color: inherit; padding: 0; cursor: pointer; }
  .filename:hover { color: var(--accent-text); }
  .nm { flex: 1; font-size: 13px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .module > .row .nm { font-weight: 600; }
  .dir > .row .nm { color: var(--muted); font-weight: 600; }
  .ct { color: var(--faint); font-size: 11.5px; font-variant-numeric: tabular-nums; }
  .kids { margin-left: 15px; padding-left: 6px; border-left: 1px solid var(--line); }
  .spacer { width: 18px; flex: none; }
  .nested-mark { color: var(--faint); font-size: 11.5px; flex: none; }
  label.nested-folder .nm { color: var(--faint); }
  .nested-open { border: 0; background: none; padding: 0 4px; color: var(--accent-text); font-size: 11.5px; cursor: pointer; flex: none; }
  .nested-open:hover { text-decoration: underline; }
</style>
