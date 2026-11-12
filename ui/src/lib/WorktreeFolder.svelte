<script lang="ts">
  // Папка «worktree» в дереве проекта-владельца: по папке на каждый живой worktree с веткой и состоянием
  // дельты; внутри — обычное дерево корня документации worktree (worktrees.md, «Окно», «Дерево»).
  import { app } from "./app.svelte";
  import Icon from "./Icon.svelte";
  import TreeRow from "./TreeRow.svelte";
  import { filterTree } from "./tree";
  import type { WorktreeStatus } from "./types";

  const ONLY_MAIN = "только у основного checkout";
  let open = $state(false);
  let expanded = $state<ReadonlySet<string>>(new Set());

  /** Подпись состояния дельты: «не проиндексирован», «строится 12 из 40», «готов, 2 отличаются», «ошибка». */
  export function deltaLabel(item: WorktreeStatus): string {
    const delta = item.delta;
    switch (delta.state) {
      case "none": return "не проиндексирован";
      case "building": return delta.done !== undefined && delta.total !== undefined ? `строится ${delta.done} из ${delta.total}` : "строится";
      case "ready": return delta.documents > 0 ? `готов, ${delta.documents} отличаются` : "готов";
      case "error": return "ошибка";
    }
  }

  async function toggleWorktree(name: string) {
    const next = new Set(expanded);
    if (next.has(name)) {
      next.delete(name);
      expanded = next;
      app.closeWorktree(name);
      return;
    }
    next.add(name);
    expanded = next;
    await app.openWorktree(name);
  }
</script>

<div class="node wt-folder" class:open data-testid="worktree-folder">
  <div class="row head">
    <button class="caret" aria-label={open ? "Свернуть worktree" : "Развернуть worktree"} aria-expanded={open} onclick={() => (open = !open)}>
      <Icon name="chevron-right" size={10} width={2} />
    </button>
    <span class="ico" aria-hidden="true"><Icon name="branch" size={14} /></span>
    <button class="nm" onclick={() => (open = !open)}>worktree</button>
    <span class="ct">{app.worktrees.length}</span>
  </div>
  {#if open}
    <div class="kids">
      {#each app.worktrees as item (item.name)}
        {@const isOpen = expanded.has(item.name)}
        <div class="node wt-item" class:open={isOpen} data-testid="worktree-item" data-worktree={item.name}>
          <div class="row" role="treeitem" aria-selected={app.focusWt === item.name} aria-expanded={isOpen} aria-label={`worktree ${item.name}`}>
            <button class="caret" aria-label={isOpen ? "Свернуть" : "Развернуть"} onclick={() => void toggleWorktree(item.name)}>
              <Icon name="chevron-right" size={10} width={2} />
            </button>
            <span class="ico" aria-hidden="true"><Icon name="branch" size={14} /></span>
            <button class="nm" title={item.path} onclick={() => void toggleWorktree(item.name)}>{item.name}</button>
            <span class="tag">{item.branch ?? "detached"}</span>
            <span class="st" data-state={item.delta.state} title={item.delta.error ?? undefined}>{deltaLabel(item)}</span>
          </div>
          {#if isOpen}
            <div class="kids">
              <!-- Граф, аудиты и «Новые темы» есть только у основного checkout (ТР-9): пункты неактивны с причиной. -->
              {#each ["Новые темы", "Аудиты", "Граф"] as label}
                <button class="off" disabled title={ONLY_MAIN} data-testid="worktree-disabled">{label}<small>{ONLY_MAIN}</small></button>
              {/each}
              {#each filterTree(app.wtTrees[item.name] ?? [], app.filter) as node (node.path)}
                <TreeRow {node} wt={item.name} forceOpen={app.filter.trim() !== ""} />
              {/each}
              {#if (app.wtTrees[item.name] ?? []).length === 0}
                <p class="none">{item.delta.state === "building" ? "Дерево строится…" : "В папке документации worktree пока ничего нет."}</p>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .row { display: flex; align-items: center; gap: 4px; padding: 4px 6px; border-radius: 8px; user-select: none; }
  .row:hover { background: var(--panel-2); }
  .caret { width: 18px; height: 18px; border: 0; background: none; display: grid; place-items: center; color: var(--muted); transition: transform .15s; padding: 0; flex: none; }
  .open > .row .caret { transform: rotate(90deg); }
  .ico { display: inline-flex; flex: none; color: var(--muted); }
  .nm { flex: 1; min-width: 0; border: 0; background: none; padding: 0; text-align: left; color: inherit; font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; cursor: pointer; }
  .head .nm { font-weight: 600; }
  .ct { color: var(--faint); font-size: 11.5px; font-variant-numeric: tabular-nums; }
  .tag { flex: none; padding: 0 6px; border: 1px solid var(--accent); border-radius: 999px; background: var(--accent-soft); color: var(--accent-text); font-size: 11px; line-height: 16px; }
  .st { flex: none; color: var(--faint); font-size: 11px; }
  .st[data-state="ready"] { color: var(--good); }
  .st[data-state="building"] { color: var(--mid); }
  .st[data-state="error"] { color: var(--bad); }
  .kids { margin-left: 15px; padding-left: 6px; border-left: 1px solid var(--line); }
  .off { display: flex; align-items: baseline; gap: 8px; width: 100%; padding: 3px 8px; border: 0; background: none; color: var(--faint); font-size: 12.5px; text-align: left; cursor: not-allowed; }
  .off small { font-size: 11px; }
  .none { color: var(--muted); font-size: 12px; padding: 4px 8px; margin: 0; }
</style>
