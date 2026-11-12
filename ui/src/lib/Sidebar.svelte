<script lang="ts">
  import { tick } from "svelte";
  import { app } from "./app.svelte";
  import { projectDot } from "./format";
  import { buildSwitcher, pathKey, topLevelProjects, type SwitcherRow } from "./projects";
  import type { ProjectInfo } from "./types";
  import Icon from "./Icon.svelte";
  import { entries } from "./entries.svelte";
  import { badgeText } from "./new-topics";
  import { filterTree, touchedModules } from "./tree";
  import TreeRow from "./TreeRow.svelte";
  import WorktreeFolder from "./WorktreeFolder.svelte";

  let menuOpen = $state(false);
  let seenMenuRequest = app.projectsMenuRequest;
  $effect(() => {
    if (app.projectsMenuRequest !== seenMenuRequest) { seenMenuRequest = app.projectsMenuRequest; menuOpen = true; }
  });

  // Меню проектов: фильтр — локальное состояние (app.filter фильтрует документы дерева), сбрасывается при закрытии.
  let filterText = $state("");
  let menuTop = $state(0);
  let projButton: HTMLButtonElement | undefined = $state();
  let menuEl: HTMLDivElement | undefined = $state();
  let filterInput: HTMLInputElement | undefined = $state();
  // Раскрытие подпроектов — в памяти окна, по пути проекта; при активном фильтре — отдельная карта, она сбрасывается вместе с текстом.
  let userOverrides = $state<Map<string, boolean>>(new Map());
  let filterOverrides = $state<Map<string, boolean>>(new Map());
  const filterActive = $derived(filterText.trim() !== "");
  const switcherRows = $derived(buildSwitcher(app.projects, { filter: filterText, current: app.current, overrides: filterActive ? filterOverrides : userOverrides }));
  const anyChevron = $derived(switcherRows.some((row) => row.hasChildren));
  $effect(() => {
    void filterText;
    filterOverrides = new Map();
  });

  function setExpanded(row: SwitcherRow<ProjectInfo>, open: boolean) {
    const next = new Map(filterActive ? filterOverrides : userOverrides);
    next.set(pathKey(row.project.path), open);
    if (filterActive) filterOverrides = next; else userOverrides = next;
  }
  const rowOf = (element: HTMLElement) => switcherRows.find((row) => row.key === element.dataset.rowKey);

  function measureMenu() {
    if (projButton) menuTop = projButton.getBoundingClientRect().bottom + 6;
  }
  $effect(() => {
    if (!menuOpen) { filterText = ""; return; }
    void tick().then(() => {
      measureMenu();
      filterInput?.focus();
      const selected = [...(menuEl?.querySelectorAll<HTMLElement>(".pitem") ?? [])].find((row) => row.dataset.projectPath === app.current);
      selected?.scrollIntoView?.({ block: "nearest" });
    });
  });

  function closeMenu(returnFocus: boolean) {
    menuOpen = false;
    if (returnFocus) projButton?.focus();
  }

  function menuItems(): HTMLElement[] {
    return [...(menuEl?.querySelectorAll<HTMLElement>('[role="menuitem"], .padd') ?? [])];
  }

  function onMenuKey(event: KeyboardEvent) {
    const target = event.target as HTMLElement;
    const inField = target === filterInput;
    const items = menuItems();
    const at = items.indexOf(target);
    const go = (item: HTMLElement | undefined) => { if (item) { item.focus(); item.scrollIntoView?.({ block: "nearest" }); } };
    switch (event.key) {
      case "Escape":
        if (filterText !== "") filterText = ""; else closeMenu(true);
        break;
      case "ArrowDown":
        go(inField ? items[0] : items[Math.min(at + 1, items.length - 1)]);
        break;
      case "ArrowUp":
        if (inField) return;
        go(at <= 0 ? items[0] : items[at - 1]);
        break;
      case "Home":
        if (inField) return;
        go(items[0]);
        break;
      case "End":
        if (inField) return;
        go(items[items.length - 1]);
        break;
      case "ArrowRight":
      case "ArrowLeft": {
        if (inField) return;
        const row = rowOf(target);
        if (!row) return;
        if (event.key === "ArrowRight") {
          if (!row.hasChildren) return;
          if (!row.expanded) setExpanded(row, true);
          else {
            const child = switcherRows[switcherRows.indexOf(row) + 1];
            go(items.find((item) => item.dataset.rowKey === child?.key));
          }
        } else if (row.hasChildren && row.expanded) {
          setExpanded(row, false);
        } else if (row.parentKey) {
          go(items.find((item) => item.dataset.rowKey === row.parentKey));
        } else return;
        break;
      }
      case "Enter":
      case " ": {
        if (inField) {
          if (event.key === " ") return;
          const first = switcherRows[0];
          if (!first) break;
          void pick(first.project.path);
        } else if (target.classList.contains("pitem") && target.dataset.projectPath) {
          void pick(target.dataset.projectPath);
        } else return; // кнопка «Добавить проект…» сработает сама
        break;
      }
      default:
        return;
    }
    event.preventDefault();
    event.stopPropagation();
  }

  const shown = $derived(filterTree(app.tree, app.filter));
  const touched = $derived(touchedModules(app.tree, app.selected));
  const dot = $derived(
    app.project
      ? projectDot(app.project, app.progressOf(app.project.path), app.waitingOf(app.project.path), app.activity?.problems.find(p => p.project === app.project?.path))
      : null,
  );
  const hasTree = $derived(!!app.project && app.project.state !== "unavailable");

  async function pick(path: string) {
    menuOpen = false;
    if (path !== app.current) await app.select(path);
    if (app.current === path) app.openSearch();
  }

</script>

<svelte:window onclick={() => (menuOpen = false)} onresize={() => menuOpen && measureMenu()} />

<aside>
  <div class="projwrap" class:open={menuOpen}>
    <button
      class="proj"
      bind:this={projButton}
      data-project-path={app.project?.path}
      onclick={(event) => {
        event.stopPropagation();
        menuOpen = !menuOpen;
      }}
    >
      {#if app.project && dot}
        <span class="dot {dot.cls}"></span>
        <span class="pn"><b>{app.project.qualified_name ?? app.project.name}</b><small>{app.project.path}{app.project.docs_root === "." ? "" : `\\${app.project.docs_root}`}</small></span>
      {:else}
        <span class="dot off"></span>
        <span class="pn"><b>Нет проектов</b><small>добавьте первый проект</small></span>
      {/if}
      <Icon name="chevron-down" size={14} />
    </button>
    {#if menuOpen}
      <div
        class="projmenu"
        role="menu"
        tabindex="-1"
        bind:this={menuEl}
        style="--projmenu-top: {menuTop}px; max-height: calc(100vh - var(--projmenu-top) - 16px)"
        onclick={(event) => event.stopPropagation()}
        onkeydown={onMenuKey}
      >
        <label class="pfilter">
          <Icon name="search" size={14} width={2.4} />
          <input bind:this={filterInput} bind:value={filterText} placeholder="Фильтр проектов" aria-label="Фильтр проектов" autocomplete="off" />
        </label>
        <div class="pscroll">
          {#each switcherRows as row (row.key)}
            {@const project = row.project}
            {@const d = projectDot(project, app.progressOf(project.path), app.waitingOf(project.path), app.activity?.problems.find(p => p.project === project.path))}
            <!-- Клавиши строк обрабатывает меню целиком (onMenuKey) -->
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              class="pitem"
              data-project-path={project.path}
              data-row-key={row.key}
              data-depth={row.depth}
              class:on={project.path === app.current}
              class:ctx={row.context}
              role="menuitem"
              tabindex="-1"
              title={project.qualified_name || undefined}
              style="padding-left: {8 + row.depth * 18}px"
              onclick={() => pick(project.path)}
            >
              {#if row.hasChildren}
                <button
                  class="chev"
                  class:open={row.expanded}
                  tabindex="-1"
                  aria-expanded={row.expanded}
                  aria-label={row.expanded ? "Свернуть подпроекты" : "Раскрыть подпроекты"}
                  onmousedown={(event) => event.preventDefault()}
                  onclick={(event) => { event.stopPropagation(); setExpanded(row, !row.expanded); }}
                ><Icon name="chevron-right" size={10} width={3} /></button>
              {:else if anyChevron}
                <span class="chev none" aria-hidden="true"></span>
              {/if}
              <span class="dot {d.cls}"></span>
              <span class="pn"><b>{row.label}</b><small>{d.text}</small></span>
            </div>
          {/each}
          {#if app.projects.length && switcherRows.length === 0}<p class="none" role="status">Ничего не найдено</p>{/if}
        </div>
        <div class="pmenu-sep"></div>
        <button
          class="padd"
          onclick={() => {
            menuOpen = false;
            void app.addProject();
          }}
        >
          <Icon name="plus" size={14} width={2.4} />Добавить проект…
        </button>
      </div>
    {/if}
  </div>

  {#if hasTree}
    <div class="filter-row"><label class="filter">
      <Icon name="search" size={14} width={2.4} />
      <input placeholder="Фильтр документов" aria-label="Фильтр документов" autocomplete="off" bind:value={app.filter} />
    </label>
    <button class="reset-scope" aria-label="Сбросить область поиска" title={(touched.length ? `Отмечено модулей: ${touched.length} из ${app.tree.length}` : "Ничего не отмечено — весь проект") + ". Сбросить область поиска"} onclick={() => app.clearSelection()}><Icon name="refresh" size={13} /></button>
    <button class="reset-scope" aria-label="Новый документ в корне документации" title="Новый документ в корне документации" onclick={() => entries.open("newDoc", ".")}><Icon name="doc" size={13} /></button>
    <button class="reset-scope" aria-label="Новая папка верхнего уровня" title="Новая папка в корне документации — папка верхнего уровня (модуль)" onclick={() => entries.open("newDir", ".")}><Icon name="folder" size={13} /></button></div>

    {#if app.selectionHint}<p class="hint" role="status" data-testid="selection-hint">{app.selectionHint}</p>{/if}
    <div class="tree" role="tree" aria-label="Документы проекта">
      {#each app.treeProblems as problem}<p class="none" role="status">{problem.path}{problem.path ? ": " : ""}{problem.reason}</p>{/each}
      <button
        class="topics-row"
        class:on={app.topicsOpen}
        aria-current={app.topicsOpen ? "true" : undefined}
        aria-label={app.topics.count === null ? "Новые темы" : `Новые темы: активных ${app.topics.count}`}
        title="Вопросы, для которых не нашлись материалы"
        onclick={() => app.openTopics()}
      >
        <Icon name="new-topics" size={15} width={1.7} />
        <span class="nm">Новые темы</span>
        <span class="ct" class:zero={app.topics.count === 0}>{badgeText(app.topics.count)}</span>
      </button>
      <button
        class="topics-row"
        class:on={app.auditsOpen}
        aria-current={app.auditsOpen ? "true" : undefined}
        aria-label={app.audits.newCount ? `Аудиты: новых находок ${app.audits.newCount}` : "Аудиты"}
        title="Битые ссылки и повторы в документации"
        onclick={() => app.openAudits()}
      >
        <Icon name="check" size={15} width={1.7} />
        <span class="nm">Аудиты</span>
        <span class="ct" class:zero={!app.audits.newCount}>{app.audits.newCount ? app.audits.newCount : ""}</span>
      </button>
      {#each shown as node (node.path)}
        <TreeRow {node} forceOpen={app.filter.trim() !== ""} />
      {/each}
      {#if app.worktrees.length}<WorktreeFolder />{/if}
      {#if app.tree.length === 0}
        <p class="none">В папке документации пока нет документов и папок.</p>
      {:else if shown.length === 0}
        <p class="none">Ничего не найдено.</p>
      {/if}
    </div>

  {:else}
    <div class="plist">
      <div class="plabel">Проекты</div>
      {#each topLevelProjects(app.projects) as project (project.path)}
        {@const d = projectDot(project, app.progressOf(project.path), app.waitingOf(project.path), app.activity?.problems.find(p => p.project === project.path))}
        <div
          class="litem"
          data-project-path={project.path}
          class:on={project.path === app.current}
          role="button"
          tabindex="0"
          onclick={() => pick(project.path)}
          onkeydown={(event) => event.key === "Enter" && pick(project.path)}
        >
          <span class="dot {d.cls}"></span>
          <span class="pn"><b>{project.name}</b><small>{d.text}</small></span>
        </div>
      {:else}
        <div class="none">Пока ни одного</div>
      {/each}
      <button class="ladd" onclick={() => app.addProject()}><Icon name="plus" size={14} width={2.4} />Добавить проект…</button>
    </div>
  {/if}

</aside>

<style>
  .topics-row { display: flex; align-items: center; gap: 8px; width: 100%; padding: 5px 8px 5px 24px; margin-bottom: 4px; border: 0; border-radius: 8px; background: none; color: inherit; text-align: left; font-size: 13px; }
  .topics-row:hover { background: var(--panel-2); }
  .topics-row.on { background: var(--accent-soft); color: var(--accent-text); }
  .topics-row:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .topics-row :global(svg) { flex: none; color: var(--muted); }
  .topics-row.on :global(svg) { color: var(--accent-text); }
  .topics-row .nm { flex: 1; min-width: 0; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .topics-row .ct { color: var(--faint); font-size: 11.5px; font-variant-numeric: tabular-nums; }
  .topics-row .ct:not(.zero) { color: var(--accent-text); font-weight: 600; }
  aside { background: var(--panel); border-right: 1px solid var(--line); display: flex; flex-direction: column; min-height: 0; }
  .projwrap { position: relative; margin: 8px 10px; }
  .proj { width: 100%; display: flex; align-items: center; gap: 10px; text-align: left; padding: 8px 10px; border-radius: 10px; border: 1px solid var(--line); background: var(--panel-2); transition: border-color .15s; }
  .proj:hover { border-color: var(--line-2); }
  .pn { flex: 1; min-width: 0; }
  .pn b { display: block; font-size: 13.5px; font-weight: 600; }
  .pn small { display: block; color: var(--muted); font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .proj :global(svg) { color: var(--faint); flex: none; }
  .projmenu { position: absolute; left: 0; right: 0; top: calc(100% + 6px); z-index: 20; display: flex; flex-direction: column; background: var(--panel); border: 1px solid var(--line-2); border-radius: 12px; box-shadow: 0 10px 30px rgba(0,0,0,.16); padding: 6px; }
  .pfilter { flex: none; position: relative; display: block; margin: 0 2px 6px; }
  .pfilter input { width: 100%; height: 28px; padding: 2px 8px 2px 26px; font-size: 12px; border-radius: 6px; border: 1px solid var(--line); background: var(--panel-2); outline: none; }
  .pfilter input:focus { border-color: var(--accent); background: var(--panel); }
  .pfilter :global(svg) { position: absolute; left: 8px; top: 7px; color: var(--faint); }
  .pscroll { flex: 1 1 auto; min-height: 0; overflow-y: auto; }
  .pitem:focus-visible, .padd:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .pitem { display: flex; align-items: center; gap: 10px; padding: 7px 8px; border-radius: 8px; cursor: pointer; }
  .pitem:hover { background: var(--panel-2); }
  .pitem.on { background: var(--accent-soft); }
  .pitem .pn b { font-size: 13px; }
  .pitem .pn small { white-space: normal; }
  .pitem.on .pn b { color: var(--accent-text); }
  .pitem.ctx .pn b, .pitem.ctx .pn small { color: var(--faint); font-weight: 500; }
  .chev { flex: none; width: 16px; height: 16px; display: grid; place-items: center; border: 0; background: none; padding: 0; color: var(--muted); border-radius: 4px; transition: transform .12s; }
  .chev:hover { background: var(--line); }
  .chev.open { transform: rotate(90deg); }
  .chev.none { visibility: hidden; }
  .pmenu-sep { flex: none; height: 1px; background: var(--line); margin: 6px 4px; }
  .padd { flex: none; display: flex; align-items: center; gap: 8px; width: 100%; border: 0; background: none; padding: 7px 8px; border-radius: 8px; color: var(--accent-text); font-weight: 600; font-size: 13px; }
  .padd:hover { background: var(--panel-2); }
  .filter-row { display: flex; align-items: center; gap: 4px; margin: 0 10px 5px; }
  .filter { position: relative; display: block; flex: 1; min-width: 0; }
  .filter input { width: 100%; height: 24px; padding: 2px 6px 2px 24px; font-size: 11px; border-radius: 5px; border: 1px solid var(--line); background: var(--panel-2); outline: none; }
  .filter input:focus { border-color: var(--accent); background: var(--panel); }
  .filter :global(svg) { position: absolute; left: 6px; top: 5px; color: var(--faint); }
  .reset-scope { display: grid; place-items: center; flex: none; width: 24px; height: 24px; padding: 0; border: 0; border-radius: 4px; background: transparent; color: var(--muted); } .reset-scope:hover { background: var(--accent-soft); color: var(--accent-text); }
  .tree { flex: 1; overflow-y: auto; padding: 2px 8px 10px; min-height: 0; }
  .none { color: var(--muted); font-size: 12.5px; padding: 8px 10px; margin: 0; }
  .hint { margin: 0 10px 5px; padding: 4px 8px; border-radius: 6px; background: var(--mid-soft); color: var(--mid); font-size: 11.5px; }
  .plist { padding: 0 8px; flex: 1; overflow-y: auto; min-height: 0; }
  .plabel { font-size: 11px; font-weight: 700; letter-spacing: .04em; text-transform: uppercase; color: var(--faint); padding: 6px 10px 4px; }
  .litem { display: flex; align-items: center; gap: 10px; padding: 8px 10px; border-radius: 9px; cursor: pointer; }
  .litem:hover { background: var(--panel-2); }
  .litem.on { background: var(--accent-soft); }
  .litem.on .pn b { color: var(--accent-text); }
  .ladd { display: flex; align-items: center; gap: 8px; width: calc(100% - 16px); margin: 6px 8px 10px; border: 1px dashed var(--line-2); background: none; padding: 8px 10px; border-radius: 9px; color: var(--accent-text); font-weight: 600; font-size: 13px; }
  .ladd:hover { background: var(--panel-2); }
</style>
