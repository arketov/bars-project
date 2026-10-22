<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { app } from "./lib/app.svelte";
  import { inTauri } from "./lib/api";
  import ContextMenu from "./lib/ContextMenu.svelte";
  import ConsentDialog from "./lib/ConsentDialog.svelte";
  import DeleteProjectDialog from "./lib/DeleteProjectDialog.svelte";
  import { theme } from "./lib/theme.svelte";
  import SettingsPage from "./lib/SettingsPage.svelte";
  import Workspace from "./lib/Workspace.svelte";

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
          else app.openSearch();
        }).then((stop) => (unlisten = stop)),
      );
    }
    return () => unlisten?.();
  });
  onDestroy(() => app.stop());

</script>

<Workspace />

{#if app.view === "settings" && app.settingsTab !== "logs" && app.settingsTab !== "mcp-statistics"}
  <div class="settings-screen"><SettingsPage /></div>
{/if}

{#if app.consentOpen}<ConsentDialog />{/if}
{#if app.deletingProject}<DeleteProjectDialog />{/if}

<ContextMenu />

{#if app.toast}
  <div class="toast" class:bad={app.toast.bad} role="status">{app.toast.text}</div>
{/if}

<style>
  /* Настройки — отдельный экран поверх рабочей области, как раньше; раскладка под ним не трогается. */
  .settings-screen { position: fixed; inset: 0; z-index: 50; background: var(--bg); overflow: auto; }
  .toast { position: fixed; left: 50%; bottom: 22px; transform: translateX(-50%); background: var(--text); color: var(--bg); padding: 9px 16px; border-radius: 10px; font-size: 13px; box-shadow: 0 10px 30px rgba(0,0,0,.25); z-index: 80; max-width: 80vw; }
  .toast.bad { background: var(--bad); color: #fff; }
</style>
