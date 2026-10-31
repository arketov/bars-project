<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "./app.svelte";
  import { call, copyText, inTauri, openPath } from "./api";
  import Banner from "./Banner.svelte";
  import { builtinModelId, screenFor, sourceOf } from "./errors";
  import type { BarsError } from "./types";

  let { error }: { error: BarsError } = $props();

  const screen = $derived(screenFor(error, sourceOf(app.status?.model.settings)));
  const modelId = $derived(builtinModelId(app.status?.model.settings));
  const modelsDir = $derived(app.models?.models_dir ?? null);

  onMount(() => {
    // Путь папки моделей нужен кнопке «Открыть папку моделей»; без снимка кнопка не показывается.
    if (screen.kind === "builtin" && screen.reason === "files_missing" && !app.models) void app.loadModels().catch(() => {});
  });

  /** «Скачать»: запускает `model_download` для модели проекта, а не повторяет поиск; дальше окно само показывает ход. */
  async function download(): Promise<void> {
    if (!modelId) { await app.search(); return; }
    try {
      await call("model_download", { id: modelId });
    } catch (failure) {
      app.flash(failure instanceof Error ? failure.message : String(failure), true);
      return;
    }
    await app.search();
  }

  async function copyDetails(): Promise<void> {
    try {
      await copyText(`${error.kind ?? error.code}${error.data?.reason ? ` · ${error.data.reason}` : ""}: ${error.message}`);
      app.flash("Подробности скопированы");
    } catch (failure) {
      app.flash(failure instanceof Error ? failure.message : String(failure), true);
    }
  }
</script>

<Banner kind={screen.kind === "loading" ? "acc" : "bad"} title={screen.title} text={screen.hint}>
  {#if screen.kind === "loading"}
    <button class="ghost sm" onclick={() => app.search()}>Повторить сейчас</button>
  {:else if screen.kind === "provider"}
    <button class="btn sm" onclick={() => app.openSettings("models")}>Открыть настройки</button>
    <button class="ghost sm" onclick={() => app.retryModelSearch()}>Повторить</button>
  {:else if screen.kind === "builtin" && screen.reason === "network"}
    <button class="btn sm" onclick={download}>Скачать снова</button>
    <button class="ghost sm" onclick={() => app.openSettings("models")}>Выбрать другую модель</button>
  {:else if screen.kind === "builtin" && screen.reason === "files_missing"}
    <button class="btn sm" onclick={download}>Скачать заново</button>
    {#if modelsDir && inTauri()}
      <button class="ghost sm" onclick={() => openPath(modelsDir)}>Открыть папку моделей</button>
    {/if}
  {:else if screen.kind === "builtin" && screen.reason === "out_of_memory"}
    <button class="btn sm" onclick={() => app.openSettings("models")}>Выбрать устройство</button>
    <button class="ghost sm" onclick={() => app.openSettings("models")}>Выбрать другую модель</button>
    <button class="ghost sm" onclick={() => app.retryModelSearch()}>Повторить</button>
  {:else if screen.kind === "builtin"}
    <button class="btn sm" onclick={() => app.retryModelSearch()}>Повторить</button>
    <button class="ghost sm" onclick={() => app.openSettings("models")}>Выбрать другую модель</button>
    <button class="ghost sm" onclick={copyDetails}>Скопировать подробности</button>
  {:else}
    <button class="ghost sm" onclick={() => app.retryModelSearch()}>Повторить</button>
  {/if}
</Banner>
{#if screen.kind !== "loading"}
{#if screen.kind === "builtin"}
<p class="faint code-line message">{error.message}</p>
{/if}
<p class="faint code-line">Код ошибки: <span class="code">{error.kind ?? error.code}</span>{#if screen.kind === "builtin"} · <span class="code">{screen.reason}</span>{/if} · текст ошибки не содержит ключей.</p>
{/if}

<style>
  .code-line { font-size: 12px; margin: -4px 0 14px; }
  .message { margin-bottom: 4px; user-select: text; }
</style>
