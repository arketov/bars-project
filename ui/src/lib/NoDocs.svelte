<script lang="ts">
  import { app } from "./app.svelte";
  import { openPath } from "./api";
  import type { ProjectInfo } from "./types";

  let { project }: { project: ProjectInfo } = $props();

  const root = $derived(project.docs_root === "." ? project.path : `${project.path}\${project.docs_root}`);
</script>

<main>
  <div class="empty">
    <div class="art mid">
      <svg width="30" height="30" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" /><path d="M14 3v5h5" /></svg>
    </div>
    <h1>В проекте {project.name} нет документации</h1>
    <p>
      Корень документации — <span class="mono">{project.docs_root}</span> — не содержит ни одного <span class="mono">.md</span>.
      Проект остаётся в списке, слежение включено: как только файлы появятся, bars проиндексирует их.
    </p>
    <div class="code block muted">{root}</div>
    <div class="acts">
      <button class="btn" onclick={() => app.chooseDocsRoot(project)}>Выбрать другую папку документации…</button>
      <button class="ghost" onclick={() => openPath(project.path)}>Открыть папку проекта</button>
      <button class="ghost danger" onclick={() => app.removeProject(project.path)}>Убрать из bars</button>
    </div>
    <p class="faint note-line">
      Другая папка сохраняется в <span class="mono">.bars/config.toml</span> проекта (ключ корня документации);
      поддерживается и корень проекта, <span class="mono">.</span>.
    </p>
  </div>
</main>

<style>
  main { overflow-y: auto; min-height: 0; }
  .note-line { margin-top: 14px; font-size: 12.5px; }
</style>
