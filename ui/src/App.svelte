<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { app } from "./lib/app.svelte";
  import { documentWindowTarget, inTauri } from "./lib/api";
  import ContextMenu from "./lib/ContextMenu.svelte";
  import ConsentDialog from "./lib/ConsentDialog.svelte";
  import DeleteProjectDialog from "./lib/DeleteProjectDialog.svelte";
  import EntryDialog from "./lib/EntryDialog.svelte";
  import MissingLinkDialog from "./lib/MissingLinkDialog.svelte";
  import { followLink } from "./lib/follow-link.svelte";
  import { entries } from "./lib/entries.svelte";
  import { theme } from "./lib/theme.svelte";
  import SettingsPage from "./lib/SettingsPage.svelte";
  import DocumentWindow from "./lib/DocumentWindow.svelte";
  import Workspace from "./lib/Workspace.svelte";

  // Отдельное окно документа (Т-40) узнаёт свой документ у процесса; `undefined` — ещё не известно,
  // `null` — главное окно. В браузере окно документа открывается как `?doc=<проект>&path=<путь>` для просмотра.
  let target = $state<{ project: string; path: string } | null | undefined>(undefined);
  if (!inTauri()) target = previewTarget();
  function previewTarget(): { project: string; path: string } | null {
    const query = new URLSearchParams(location.search);
    const project = query.get("project"), path = query.get("doc");
    return project && path ? { project, path } : null;
  }

  onMount(() => {
    theme.start();
    app.start();
    if (inTauri()) void documentWindowTarget().then((found) => (target = found)).catch(() => (target = null));
    // Трей и второй запуск просят открыть нужный экран.
    const stops: (() => void)[] = [];
    let disposed = false;
    const keep = (stop: () => void) => { if (disposed) stop(); else stops.push(stop); };
    if (inTauri()) {
      void import("@tauri-apps/api/event").then(({ listen }) =>
        listen<{ view: string; project: string | null }>("bars://navigate", async ({ payload }) => {
          if (payload.project) await app.select(payload.project);
          if (payload.view === "settings") app.openSettings();
          else if (payload.view === "indexing") app.openIndexing();
          else app.openSearch();
        }).then(keep),
      );
      // Ссылка внутри отдельного окна документа открывает целевой документ вкладкой основного окна (Т-40).
      void import("@tauri-apps/api/event").then(({ listen }) =>
        listen<{ project: string; path: string; first_line: number; last_line: number }>("bars://open-document", async ({ payload }) => {
          if (target) return;
          if (app.current !== payload.project) await app.select(payload.project);
          await app.openDocument({ path: payload.path, title: payload.path.split("/").pop() ?? payload.path, first_line: payload.first_line, last_line: payload.last_line });
        }).then(keep),
      );
    }
    return () => { disposed = true; stops.forEach((stop) => stop()); };
  });
  onDestroy(() => app.stop());

</script>

{#if target === undefined}
  <!-- Окно узнаёт, главное оно или документа: до ответа ничего не рисуется, иначе главное окно мигнёт раскладкой. -->
{:else if target}
  <DocumentWindow project={target.project} path={target.path} />
{:else}
<Workspace />

{#if app.view === "settings"}
  <div class="settings-screen"><SettingsPage /></div>
{/if}

{#if app.consentOpen}<ConsentDialog />{/if}
{#if app.deletingProject}<DeleteProjectDialog />{/if}
{#if entries.request}<EntryDialog />{/if}
{/if}

<ContextMenu />
{#if followLink.pending}<MissingLinkDialog />{/if}

{#if app.toast}
  <div class="toast" class:bad={app.toast.bad} role="status">{app.toast.text}</div>
{/if}

<style>
  /* Настройки — отдельный экран поверх рабочей области, как раньше; раскладка под ним не трогается. */
  .settings-screen { position: fixed; inset: 0; z-index: 50; background: var(--bg); overflow: auto; }
  .toast { position: fixed; left: 50%; bottom: 22px; transform: translateX(-50%); background: var(--text); color: var(--bg); padding: 9px 16px; border-radius: 10px; font-size: 13px; box-shadow: 0 10px 30px rgba(0,0,0,.25); z-index: 80; max-width: 80vw; }
  .toast.bad { background: var(--bad); color: #fff; }
</style>
