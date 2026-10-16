<script lang="ts">
  import { app } from "./app.svelte";
  import { projectDot } from "./format";
  import Icon from "./Icon.svelte";
  import { theme } from "./theme.svelte";
  import { filterTree, touchedModules } from "./tree";
  import TreeRow from "./TreeRow.svelte";

  let menuOpen = $state(false);

  const shown = $derived(filterTree(app.tree, app.filter));
  const touched = $derived(touchedModules(app.tree, app.selected));
  const dot = $derived(
    app.project
      ? projectDot(app.project, app.progressOf(app.project.path), app.waitingOf(app.project.path))
      : null,
  );
  const hasTree = $derived(!!app.project && app.project.state !== "unavailable");

  function pick(path: string) {
    menuOpen = false;
    app.view = "main";
    void app.select(path);
  }

  /** Строка состояния: заголовок и вторая строка (время обновления, ход). */
  function statusText(): [string, string] {
    const status = app.status;
    const running = app.activity?.running;
    if (!app.project) return ["Нет проектов", ""];
    if (running && running.project === app.project.path) {
      const percent = app.progressOf(app.project.path);
      return percent === null ? ["Индексация", "обход папки"] : ["Индексация", `проект ${percent} %`];
    }
    if (app.project.state === "unavailable") return ["Папка проекта не найдена", ""];
    if (app.project.state === "reindex_required") return ["Нужна переиндексация", ""];
    if (app.waitingOf(app.project.path) > 0) return [`Ждут ${app.waitingOf(app.project.path)} документов`, "поиск работает по готовому"];
    if (status?.updated_at) {
      const time = new Date(status.updated_at).toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
      return ["Индекс актуален", `обновлён ${time}`];
    }
    return ["Индекс актуален", ""];
  }

  const status = $derived(statusText());

  const statusDot = $derived(
    !app.project
      ? "off"
      : app.project.state === "reindex_required"
        ? "warn"
        : app.project.state === "unavailable"
          ? "off"
          : app.progressOf(app.project.path) !== null || app.waitingOf(app.project.path) > 0
            ? "busy"
            : (app.activity?.problems.some((p) => p.project === app.project?.path) ?? false)
              ? "warn"
              : "ok",
  );
</script>

<svelte:window onclick={() => (menuOpen = false)} />

<aside>
  <div class="brand">
    <div class="logo">b</div>
    <div><b>bars</b><small>поиск по документации проектов</small></div>
  </div>

  <div class="projwrap" class:open={menuOpen}>
    <button
      class="proj"
      onclick={(event) => {
        event.stopPropagation();
        menuOpen = !menuOpen;
      }}
    >
      {#if app.project && dot}
        <span class="dot {dot.cls}"></span>
        <span class="pn"><b>{app.project.name}</b><small>{app.project.path}{app.project.docs_root === "." ? "" : `\\${app.project.docs_root}`}</small></span>
      {:else}
        <span class="dot off"></span>
        <span class="pn"><b>Нет проектов</b><small>добавьте первый проект</small></span>
      {/if}
      <Icon name="chevron-down" size={14} />
    </button>
    {#if menuOpen}
      <div class="projmenu" role="menu" tabindex="-1" onclick={(event) => event.stopPropagation()} onkeydown={() => {}}>
        {#each app.projects as project (project.path)}
          {@const d = projectDot(project, app.progressOf(project.path), app.waitingOf(project.path))}
          <div
            class="pitem"
            class:on={project.path === app.current}
            role="menuitem"
            tabindex="0"
            onclick={() => pick(project.path)}
            onkeydown={(event) => event.key === "Enter" && pick(project.path)}
          >
            <span class="dot {d.cls}"></span>
            <span class="pn"><b>{project.name}</b><small>{d.text}</small></span>
          </div>
        {/each}
        {#if app.projects.length}<div class="pmenu-sep"></div>{/if}
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
    <label class="filter">
      <Icon name="search" size={14} width={2.4} />
      <input placeholder="Фильтр модулей и файлов" autocomplete="off" bind:value={app.filter} />
    </label>

    <div class="tree">
      {#each shown as node (node.path)}
        <TreeRow {node} forceOpen={app.filter.trim() !== ""} />
      {/each}
      {#if app.tree.length === 0}
        <p class="none">В индексе пока нет документов.</p>
      {:else if shown.length === 0}
        <p class="none">Ничего не найдено.</p>
      {/if}
    </div>

    <div class="sfoot">
      <span>
        {touched.length
          ? `Отмечено модулей: ${touched.length} из ${app.tree.length}`
          : "Ничего не отмечено — весь проект"}
      </span>
      <button class="link" onclick={() => app.clearSelection()}>Сбросить</button>
    </div>
  {:else}
    <div class="plist">
      <div class="plabel">Проекты</div>
      {#each app.projects as project (project.path)}
        {@const d = projectDot(project, app.progressOf(project.path), app.waitingOf(project.path))}
        <div
          class="litem"
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

  <div class="sfoot">
    <div class="status">
      <span class="dot {statusDot}"></span>
      <span class="text">
        <span class="line1">{status[0]}</span>
        {#if status[1]}<span class="line2">{status[1]}</span>{/if}
      </span>
    </div>
    <div class="tools">
      <button class="iconbtn" title="Светлая / тёмная тема" onclick={() => theme.toggle()}>
        <Icon name="moon" size={17} width={2} />
      </button>
      <button class="iconbtn" title="Настройки" onclick={() => app.openSettings()}>
        <Icon name="gear" size={17} width={2} />
      </button>
    </div>
  </div>
</aside>

<style>
  aside { background: var(--panel); border-right: 1px solid var(--line); display: flex; flex-direction: column; min-height: 0; }
  .brand { padding: 18px 18px 12px; display: flex; align-items: center; gap: 10px; }
  .logo { width: 30px; height: 30px; border-radius: 9px; background: var(--accent); color: var(--on-accent); display: grid; place-items: center; font-weight: 800; font-size: 16px; letter-spacing: -.5px; }
  .brand b { font-size: 15px; }
  .brand small { display: block; color: var(--muted); font-size: 11.5px; margin-top: -2px; }
  .projwrap { position: relative; margin: 0 14px 10px; }
  .proj { width: 100%; display: flex; align-items: center; gap: 10px; text-align: left; padding: 8px 10px; border-radius: 10px; border: 1px solid var(--line); background: var(--panel-2); transition: border-color .15s; }
  .proj:hover { border-color: var(--line-2); }
  .pn { flex: 1; min-width: 0; }
  .pn b { display: block; font-size: 13.5px; font-weight: 600; }
  .pn small { display: block; color: var(--muted); font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .proj :global(svg) { color: var(--faint); flex: none; }
  .projmenu { position: absolute; left: 0; right: 0; top: calc(100% + 6px); z-index: 20; background: var(--panel); border: 1px solid var(--line-2); border-radius: 12px; box-shadow: 0 10px 30px rgba(0,0,0,.16); padding: 6px; }
  .pitem { display: flex; align-items: center; gap: 10px; padding: 7px 8px; border-radius: 8px; cursor: pointer; }
  .pitem:hover { background: var(--panel-2); }
  .pitem.on { background: var(--accent-soft); }
  .pitem .pn b { font-size: 13px; }
  .pitem .pn small { white-space: normal; }
  .pitem.on .pn b { color: var(--accent-text); }
  .pmenu-sep { height: 1px; background: var(--line); margin: 6px 4px; }
  .padd { display: flex; align-items: center; gap: 8px; width: 100%; border: 0; background: none; padding: 7px 8px; border-radius: 8px; color: var(--accent-text); font-weight: 600; font-size: 13px; }
  .padd:hover { background: var(--panel-2); }
  .filter { margin: 0 14px 8px; position: relative; display: block; }
  .filter input { width: 100%; padding: 8px 10px 8px 32px; border-radius: 9px; border: 1px solid var(--line); background: var(--panel-2); outline: none; }
  .filter input:focus { border-color: var(--accent); background: var(--panel); }
  .filter :global(svg) { position: absolute; left: 10px; top: 10px; color: var(--faint); }
  .tree { flex: 1; overflow-y: auto; padding: 2px 8px 10px; min-height: 0; }
  .none { color: var(--muted); font-size: 12.5px; padding: 8px 10px; margin: 0; }
  .plist { padding: 0 8px; flex: 1; overflow-y: auto; min-height: 0; }
  .plabel { font-size: 11px; font-weight: 700; letter-spacing: .04em; text-transform: uppercase; color: var(--faint); padding: 6px 10px 4px; }
  .litem { display: flex; align-items: center; gap: 10px; padding: 8px 10px; border-radius: 9px; cursor: pointer; }
  .litem:hover { background: var(--panel-2); }
  .litem.on { background: var(--accent-soft); }
  .litem.on .pn b { color: var(--accent-text); }
  .ladd { display: flex; align-items: center; gap: 8px; width: calc(100% - 16px); margin: 6px 8px 10px; border: 1px dashed var(--line-2); background: none; padding: 8px 10px; border-radius: 9px; color: var(--accent-text); font-weight: 600; font-size: 13px; }
  .ladd:hover { background: var(--panel-2); }
  .status { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .status .text { display: flex; flex-direction: column; min-width: 0; line-height: 1.3; }
  .status .line1 { color: var(--text); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .status .line2 { font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .tools { display: flex; gap: 2px; }
  .iconbtn { border: 1px solid transparent; background: none; color: var(--muted); padding: 5px; border-radius: 8px; display: grid; place-items: center; }
  .iconbtn:hover { background: var(--panel-2); border-color: var(--line); color: var(--text); }
</style>
