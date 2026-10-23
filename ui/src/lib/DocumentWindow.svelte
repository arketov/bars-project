<script lang="ts">
  // Отдельное окно документа (Т-40): один документ с редактором, общий воркер, без дерева, поиска и вкладок.
  // Закрытие окна сначала записывает правки (НФТ-5); при неудаче решает пользователь тем же диалогом.
  import { onMount } from "svelte";
  import { app } from "./app.svelte";
  import { documentWindowClose, inTauri } from "./api";
  import UnsavedDialog from "./UnsavedDialog.svelte";
  import WorkspaceDocument from "./WorkspaceDocument.svelte";
  import { confirmClose, startFlushListener } from "./unsaved.svelte";
  import { moved, movedPath } from "./moved.svelte";
  let { project, path }: { project: string; path: string } = $props();
  const title = $derived(path.split("/").pop() ?? path);
  $effect(() => { document.title = `${title} — bars`; });
  onMount(() => {
    let disposed = false;
    const stops: (() => void)[] = [];
    stops.push(moved.subscribe(event => { if (event.project === project) path = movedPath(path, event.from, event.to); }));
    const keep = (stop: () => void) => { if (disposed) stop(); else stops.push(stop); };
    void app.bindDocumentWindow(project);
    void startFlushListener().then(keep).catch((error) => app.flash(String(error), true));
    if (inTauri()) {
      void import("@tauri-apps/api/event").then(({ listen }) =>
        listen("bars://document-close-request", async () => {
          if (await confirmClose(project, path)) await documentWindowClose();
        }).then(keep)).catch((error) => app.flash(String(error), true));
    }
    return () => { disposed = true; stops.forEach((stop) => stop()); };
  });
</script>

<main class="document-window" aria-label={title}>
  <WorkspaceDocument {project} hit={{ path, title, first_line: 0, last_line: 0 }} />
</main>
<UnsavedDialog />

<style>
  .document-window { position: fixed; inset: 0; display: flex; flex-direction: column; background: var(--panel); }
  .document-window > :global(section) { flex: 1; min-height: 0; }
</style>
