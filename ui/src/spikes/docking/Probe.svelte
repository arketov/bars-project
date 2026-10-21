<script lang="ts">
  import { onMount, mount, unmount, tick } from "svelte";
  import { createDockview, themeLight, themeDark, type DockviewApi } from "dockview-core";
  import { invoke } from "@tauri-apps/api/core";
  import { call, inTauri } from "../../lib/api";
  import { app } from "../../lib/app.svelte";
  import { theme } from "../../lib/theme.svelte";
  import Panel from "./Panel.svelte";
  import { defaults, save, restore, key, messages } from "./layout";

  let host = $state<HTMLDivElement>(null!);
  let api: DockviewApi;
  let ready = $state(false);
  let status = $state("Исследовательский стенд. Изменений файлов нет.");
  const child = new URLSearchParams(location.search).has("document");
  $effect(() => { if (ready) api.updateOptions({ theme: theme.mode === "dark" ? themeDark : themeLight }); });

  function reset() { defaults(api); status = "Штатная раскладка восстановлена"; }
  function persist() { try { localStorage.setItem(key, save(api)); status = "Раскладка сохранена"; } catch (e) { status = String(e); } }
  function load() {
    try { const text = localStorage.getItem(key); if (text) restore(api, text); else reset(); }
    catch { reset(); status = "Раскладка повреждена; восстановлена штатная"; }
  }
  async function readProbe() {
    const bootstrap = await invoke<{ project: string; pid: number }>("probe_info");
    const doc = await call<{ title: string; text: string }>("read", { cwd: bootstrap.project, project: bootstrap.project, path: "Тема.md" });
    if (!doc.text.includes("Общий воркер")) throw new Error("Неверный документ воркера");
    app.viewer = { title: doc.title, text: doc.text, missing: false, hit: { path: "docs/Тема.md", title: doc.title, first_line: 0, last_line: 0 } };
    return { ...bootstrap, title: doc.title };
  }
  async function openDocument() {
    if (inTauri()) await invoke("probe_open_document");
    else status = "Отдельное окно проверяется нативным Tauri-стендом";
  }
  async function selftest() {
    try {
      const worker = await readProbe();
      if (child) {
        await tick();
        if (!document.querySelector(".md")?.textContent?.includes("Общий воркер")) throw new Error("Документ не отобразился в дочернем окне");
        await invoke("probe_finish", { result: { worker, read: true, rendered: true } }); return;
      }
      const initial = api.groups.length;
      const draft = api.getPanel("draft")!;
      const input = host.querySelector('[data-kind="draft"] textarea') as HTMLTextAreaElement;
      input.value = "Состояние после переноса";
      input.dispatchEvent(new Event("input", { bubbles: true }));
      draft.api.moveTo({ group: api.getPanel("search")!.group });
      await tick();
      if (!host.contains(input) || input.value !== "Состояние после переноса") throw new Error("Потеря DOM/ввода при переносе");
      draft.api.moveTo({ group: api.getPanel("search")!.group, position: "bottom" });
      api.layout(1280, 730);
      const encoded = save(api);
      restore(api, encoded);
      if (api.panels.length !== 6 || api.groups.length !== initial + 1) throw new Error("Неверный round-trip");
      localStorage.setItem(key, "{broken"); load();
      if (!status.includes("повреждена") || api.panels.length !== 6) throw new Error("Нет восстановления после повреждения");
      persist(); load();
      const tab = [...host.querySelectorAll<HTMLElement>('[role="tab"]')].find(node => node.textContent?.includes("Дополнительно"))!;
      if (!tab) throw new Error("Нет ARIA-вкладок");
      tab.focus();
      const tabs = [...tab.closest('[role="tablist"]')!.querySelectorAll('[role="tab"]')];
      tab.dispatchEvent(new KeyboardEvent("keydown", { key: "End", bubbles: true }));
      const keyboard = tabs.length === 2 && document.activeElement === tabs.at(-1);
      if (!keyboard) throw new Error("Не работает клавиатура вкладок");
      await openDocument();
      let childResult: unknown;
      for (let n = 0; n < 150; n++) {
        childResult = await invoke("probe_child_result");
        if (childResult) break;
        await new Promise(resolve => setTimeout(resolve, 100));
      }
      if (!childResult) throw new Error("Нет ответа дочернего окна");
      if ((childResult as { worker?: { pid: number } }).worker?.pid !== worker.pid) throw new Error("Окна используют разные воркеры");
      const afterClose = await readProbe();
      await invoke("probe_finish", { result: { worker, afterClose, childResult, panels: api.panels.length, initialGroups: initial, movePreservesDOM: true, roundTrip: true, corruptRecovery: true, keyboard, userAgent: navigator.userAgent } });
    } catch (e) { await invoke("probe_finish", { result: { error: String(e), child } }); }
  }

  onMount(() => {
    theme.start();
    if (child) { void selftest(); return; }
    app.start();
    api = createDockview(host, {
      theme: themeLight,
      keyboardNavigation: true,
      messages,
      createComponent: () => {
        const element = document.createElement("div");
        element.style.height = "100%";
        let component: ReturnType<typeof mount>;
        return { element, init: ({ api: panel }) => { component = mount(Panel, { target: element, props: { kind: panel.id } }); }, dispose: () => { if (component) void unmount(component); } };
      },
    });
    load();
    ready = true;
    if (inTauri()) void selftest();
    return () => { api.dispose(); app.stop(); };
  });
</script>

{#if child}<Panel kind="document" />
{:else}
  <header><b>bars · проба docking</b><button onclick={persist}>Сохранить</button><button onclick={load}>Восстановить</button><button onclick={reset}>Сбросить</button><button onclick={() => void openDocument()}>Документ в новом окне</button><button onclick={() => theme.toggle()}>Тема</button></header>
  <p role="status">{status}</p>
  <div class="dock" bind:this={host}></div>
{/if}

<style>
  :global(body) { margin: 0; overflow: hidden; } header { display: flex; align-items: center; gap: 10px; padding: 8px; }
  p { margin: 4px 8px; font-size: 12px; } .dock { height: calc(100vh - 85px); width: 100vw; }
</style>
