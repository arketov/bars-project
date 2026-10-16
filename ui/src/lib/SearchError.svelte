<script lang="ts">
  import { app } from "./app.svelte";
  import Banner from "./Banner.svelte";
  import { screenFor, sourceOf } from "./errors";
  import type { BarsError } from "./types";

  let { error }: { error: BarsError } = $props();

  const screen = $derived(screenFor(error, sourceOf(app.status?.model.settings)));
</script>

<Banner kind={screen.kind === "loading" ? "acc" : "bad"} title={screen.title} text={screen.hint}>
  {#if screen.kind === "loading"}
    <button class="ghost sm" onclick={() => app.search()}>Повторить сейчас</button>
  {:else if screen.kind === "provider"}
    <button class="btn sm" onclick={() => app.openSettings("models")}>Открыть настройки</button>
    <button class="ghost sm" onclick={() => app.search()}>Повторить</button>
  {:else if screen.kind === "model-download"}
    <button class="btn sm" onclick={() => app.search()}>Повторить</button>
    <button class="ghost sm" onclick={() => app.openSettings("models")}>Выбрать другую модель</button>
  {:else if screen.kind === "model-files"}
    <button class="btn sm" onclick={() => app.search()}>Скачать заново</button>
    <button class="ghost sm" onclick={() => app.openSettings("models")}>Выбрать другую модель</button>
  {:else}
    <button class="ghost sm" onclick={() => app.search()}>Повторить</button>
  {/if}
</Banner>
{#if screen.kind !== "loading"}
<p class="faint code-line">Код ошибки: <span class="code">{error.kind ?? error.code}</span> · текст ошибки не содержит ключей.</p>
{/if}

<style>
  .code-line { font-size: 12px; margin: -4px 0 14px; }
</style>
