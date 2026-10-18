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
  import NoDocs from "./lib/NoDocs.svelte";
  import SearchPane from "./lib/SearchPane.svelte";
  import SettingsPage from "./lib/SettingsPage.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
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
  const viewing = $derived(app.view === "main" && !!app.viewer);
</script>

{#if app.view === "settings"}
  <SettingsPage />
{:else}
  <div class="app" class:viewing>
    <Sidebar />
    {#if app.workerDown}
      <WorkerDown message={app.workerDown} />
    {:else if app.projectsLoaded && app.projects.length === 0}
      <EmptyProjects />
    {:else if !project}
      <main></main>
    {:else if project.state === "unavailable"}
      <Missing {project} />
    {:else if app.view === "indexing"}
      <IndexingPage />
    {:else if noDocs}
      <NoDocs {project} />
    {:else}
      <SearchPane />
      {#if viewing}<Viewer />{/if}
    {/if}
  </div>
{/if}

{#if app.consentOpen}<ConsentDialog />{/if}
{#if app.deletingProject}<DeleteProjectDialog />{/if}

<ContextMenu />

{#if app.toast}
  <div class="toast" class:bad={app.toast.bad} role="status">{app.toast.text}</div>
{/if}

<style>
  .app.viewing { grid-template-columns: 300px minmax(420px, 1fr) minmax(440px, 1.1fr); }
  main { min-height: 0; }
  .toast { position: fixed; left: 50%; bottom: 22px; transform: translateX(-50%); background: var(--text); color: var(--bg); padding: 9px 16px; border-radius: 10px; font-size: 13px; box-shadow: 0 10px 30px rgba(0,0,0,.25); z-index: 80; max-width: 80vw; }
  .toast.bad { background: var(--bad); color: #fff; }
</style>
