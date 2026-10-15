<script lang="ts">
  import { app } from "./app.svelte";
  import { barsDir, copyText, openPath, restartApp } from "./api";
  import Icon from "./Icon.svelte";

  let { message }: { message: string } = $props();

  async function openLog() {
    const dir = await barsDir();
    if (dir) await openPath(`${dir}${dir.includes("\\") ? "\\" : "/"}logs`);
  }

  async function copyDetails() {
    await copyText(`bars: воркер не отвечает: ${message}`);
    app.flash("Подробности скопированы");
  }
</script>

<main>
  <div class="empty">
    <div class="art bad">
      <svg width="30" height="30" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 2v6M15 2v6M6 8h12v4a6 6 0 0 1-12 0zM12 18v4" /></svg>
    </div>
    <h1>Воркер bars не отвечает</h1>
    <p>
      Окно потеряло связь со службой, которая держит индексы и модели. Поиск и список проектов временно
      недоступны. Индексы на диске целы: недописанный документ в них либо старый, либо новый.
    </p>
    <div class="acts">
      <button class="btn" onclick={() => restartApp()}><Icon name="refresh" size={14} />Перезапустить воркер</button>
      <button class="ghost" onclick={openLog}>Открыть лог</button>
      <button class="ghost" onclick={copyDetails}>Копировать подробности</button>
    </div>
    <p class="faint note-line">Подробности: <span class="code">{message}</span></p>
    <p class="faint note-line">
      «Перезапустить воркер» перезапускает весь процесс bars: окно и воркер — один процесс. Агентам в это время
      отвечает <span class="code">bars-mcp</span>: «воркер недоступен, bars запустит его» (код выхода
      <span class="code">bars-cli</span> — 2).
    </p>
  </div>
</main>

<style>
  main { overflow-y: auto; min-height: 0; }
  .note-line { margin-top: 14px; font-size: 12.5px; }
</style>
