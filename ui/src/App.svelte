<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { app } from "./lib/app.svelte";
  import { inTauri } from "./lib/api";
  import ContextMenu from "./lib/ContextMenu.svelte";
  import ConsentDialog from "./lib/ConsentDialog.svelte";
  import DeleteProjectDialog from "./lib/DeleteProjectDialog.svelte";
  import EmptyProjects from "./lib/EmptyProjects.svelte";
  import IndexingPage from "./lib/IndexingPage.svelte";
  import Missing from "./lib/Missing.svelte";
  import NewTopicsPage from "./lib/NewTopicsPage.svelte";
  import NoDocs from "./lib/NoDocs.svelte";
  import SearchPane from "./lib/SearchPane.svelte";
  import SettingsPage from "./lib/SettingsPage.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import MainNavigation from "./lib/MainNavigation.svelte";
  import { theme } from "./lib/theme.svelte";
  import Viewer from "./lib/Viewer.svelte";
  import WorkerDown from "./lib/WorkerDown.svelte";

  onMount(() => {
    theme.start();
    app.start();
    // Трей и второй запуск просят открыть нужный экран.
    let unlisten: (() => void) | undefined;
    if (inTauri()) {
      void import("@tauri-apps/api/event").then(({ listen }) =>
        listen<{ view: string; project: string | null }>("bars://navigate", async ({ payload }) => {
          if (payload.project) await app.select(payload.project);
          if (payload.view === "settings") app.openSettings();
          else if (payload.view === "indexing") app.openIndexing();
          else app.view = "main";
        }).then((stop) => (unlisten = stop)),
      );
    }
    return () => unlisten?.();
  });
  onDestroy(() => app.stop());

  const project = $derived(app.project);
  const busy = $derived(
    !!project && (app.progressOf(project.path) !== null || app.waitingOf(project.path) > 0 || project.state === "indexing"),
  );
  const noDocs = $derived(
    !!project && project.state === "ready" && app.documentsOf(project) === 0 && !busy && app.tree.length === 0,
  );
  const viewing = $derived(app.view === "main" && (!!app.viewer || app.topicsOpen));
</script>

{#if app.view === "settings"}
  <SettingsPage />
{:else}
  <div class="app">
    <Sidebar />
    <section class="work-area" aria-label="Рабочая область">
    <MainNavigation />
    <div class="work-body" class:viewing>
    {#if app.view === "indexing" && project}
      <IndexingPage />
    {:else if app.workerDown}
      <WorkerDown message={app.workerDown} />
    {:else if app.projectsLoaded && app.projects.length === 0}
      <EmptyProjects />
    {:else if !project}
      <main></main>
    {:else if project.state === "unavailable"}
      <Missing {project} />
    {:else if app.view === 'graph'}
      {#await import('./lib/GraphPage.svelte')}
        <main role="status">Загрузка графа…</main>
      {:then graph}
        <graph.default />
      {:catch error}
        <main role="alert">Не удалось загрузить граф: {String(error)}</main>
      {/await}
    {:else}
      {#if noDocs}<NoDocs {project} />{:else}<SearchPane />{/if}
      {#if app.topicsOpen}<NewTopicsPage />{:else if viewing}<Viewer />{/if}
    {/if}
    </div>
    </section>
  </div>
{/if}

{#if app.consentOpen}<ConsentDialog />{/if}
{#if app.deletingProject}<DeleteProjectDialog />{/if}

<ContextMenu />

{#if app.toast}
  <div class="toast" class:bad={app.toast.bad} role="status">{app.toast.text}</div>
{/if}

<style>
  .work-area { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  .work-body { display: grid; grid-template-columns: minmax(0, 1fr); flex: 1; min-width: 0; min-height: 0; }
  .work-body.viewing { grid-template-columns: minmax(0, 1fr) minmax(0, 1.1fr); }
  main { min-height: 0; }
  .toast { position: fixed; left: 50%; bottom: 22px; transform: translateX(-50%); background: var(--text); color: var(--bg); padding: 9px 16px; border-radius: 10px; font-size: 13px; box-shadow: 0 10px 30px rgba(0,0,0,.25); z-index: 80; max-width: 80vw; }
  .toast.bad { background: var(--bad); color: #fff; }
</style>
