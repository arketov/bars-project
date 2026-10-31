<script lang="ts">
  import { app } from "./app.svelte";
  import Icon from "./Icon.svelte";
  import type { ProjectInfo } from "./types";

  let { project }: { project: ProjectInfo } = $props();
</script>

<main>
  <div class="empty">
    <div class="art low"><Icon name="folder" size={30} width={2} /></div>
    <h1>Папка проекта {project.name} не найдена</h1>
    <p class="mono muted path">{project.path}</p>
    <p>
      Папка могла переехать, лежать на отключённом диске или в ещё не загруженной папке OneDrive. bars ничего не
      удаляет: проект остаётся в списке, индекс цел. Как только папка вернётся, при ближайшей проверке проект
      возобновится сам.
    </p>
    <div class="acts">
      <button class="btn" disabled={app.checking} onclick={() => app.recheck()}>{app.checking ? "Проверяем…" : "Проверить сейчас"}</button>
      <button class="ghost" onclick={() => app.relocateProject(project)}>Добавить проект на новом месте…</button>
      <button class="ghost danger" onclick={() => app.removeProject(project.path)}>Убрать из bars</button>
    </div>
    <p class="faint note-line">
      «Убрать» только снимает проект с регистрации; папка <span class="mono">.bars/</span> и файлы проекта остаются.
      Перенесённая папка с <span class="mono">.bars/</span> подхватывается как есть — без повторной индексации, если
      модель та же.
    </p>
    <p class="faint note-line">Агентам по этому проекту отвечает ошибка <span class="code">project_unavailable</span>.</p>
  </div>
</main>

<style>
  main { overflow-y: auto; min-height: 0; }
  .path { margin: 0 0 6px; }
  .note-line { margin-top: 14px; font-size: 12.5px; }
</style>
