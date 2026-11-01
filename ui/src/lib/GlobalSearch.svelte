<script lang="ts">
  import { onDestroy } from 'svelte';
  import { app } from './app.svelte';
  import Icon from './Icon.svelte';
  let { search, tree, changed }: { search: () => HTMLElement; tree: () => HTMLElement; changed: (open: boolean) => void } = $props();
  let dialog: HTMLDialogElement, searchHost: HTMLDivElement, treeHost: HTMLDivElement;
  let scopeOpen = $state(false), opened = $state(false);
  let previousFocus: HTMLElement | null = null;
  export function close(restore = true) {
    if (!opened) return;
    dialog.close(); opened = false; changed(false);
    if (restore && previousFocus?.isConnected) previousFocus.focus();
  }
  export function open() {
    if (opened) { searchHost.querySelector<HTMLElement>('textarea:not(:disabled)')?.focus(); return; }
    previousFocus = document.activeElement as HTMLElement;
    scopeOpen = false; opened = true; changed(true);
    treeHost.append(tree()); searchHost.append(search());
    dialog.showModal();
    (searchHost.querySelector<HTMLElement>('textarea:not(:disabled)') ?? dialog.querySelector<HTMLElement>('.close-search'))?.focus();
  }
  function trap(event: KeyboardEvent) {
    if (event.key !== 'Tab') return;
    const items = [...dialog.querySelectorAll<HTMLElement>('button,input,select,textarea,summary,[tabindex="0"],a[href]')].filter(node => !node.matches(':disabled') && !!node.getClientRects().length);
    if (event.shiftKey && document.activeElement === items[0]) { event.preventDefault(); items.at(-1)?.focus(); }
    else if (!event.shiftKey && document.activeElement === items.at(-1)) { event.preventDefault(); items[0]?.focus(); }
  }
  onDestroy(() => close(false));
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions: ограничение Tab модальным слоем. -->
<dialog bind:this={dialog} class:scope-open={scopeOpen} class:completed={app.searching || app.results !== null || app.searchError !== null || app.project?.state === 'reindex_required'} aria-label="Глобальный поиск" onkeydown={trap} oncancel={(event) => { event.preventDefault(); close(); }}>
  <div class="modal-controls"><b>{app.project?.name ?? 'bars'}</b><button class="scope-toggle" aria-expanded={scopeOpen} onclick={() => scopeOpen = !scopeOpen}>Область</button><button class="close-search" aria-label="Закрыть поиск" onclick={() => close()}><Icon name="close" size={15} /></button></div>
  <div class="modal-body"><div class="tree-host" bind:this={treeHost}></div><div class="search-host" bind:this={searchHost}></div></div>
</dialog>

<style>
  dialog { width:min(1000px,calc(100vw - 32px)); height:min(720px,calc(100vh - 32px)); max-width:none; max-height:none; margin:auto; padding:0; border:1px solid var(--line-2); border-radius:10px; background:var(--bg); color:var(--text); box-shadow:var(--shadow); overflow:hidden; }
  dialog::backdrop { background:#0005; }
  .modal-controls { display:flex; align-items:center; gap:12px; height:28px; padding:0 10px; border-bottom:1px solid var(--line); background:var(--panel-2); font-size:12px; }
  .close-search { margin-left:auto; display:flex; padding:2px; background:transparent; color:var(--muted); border:0; }
  .modal-body { height:calc(100% - 28px); display:flex; min-height:0; }
  .tree-host { width:240px; min-height:0; flex:none; background:var(--panel); border-right:1px solid var(--line); display:flex; }
  .search-host { min-width:0; min-height:0; flex:1; display:flex; }
  dialog:not(.completed) .search-host :global(.top) { margin-top:auto; margin-bottom:auto; }
  dialog:not(.completed) .search-host :global(.results) { display:none; }
  .scope-toggle { display:none; }
  @media(max-width:850px) {
    .modal-body { flex-direction:column; }
    .tree-host { display:none; width:100%; height:210px; border-bottom:1px solid var(--line); }
    .scope-open .tree-host { display:flex; }
    .scope-toggle { display:block; font-size:12px; border:0; background:transparent; color:var(--muted); padding:2px 8px; }
  }
</style>
