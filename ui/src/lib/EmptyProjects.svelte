<script lang="ts">
  import { app } from "./app.svelte";
  import Icon from "./Icon.svelte";
</script>

<main>
  <div class="empty">
    <div class="art"><Icon name="folder" size={30} width={2} /></div>
    <h1>Добавьте первый проект</h1>
    <p>
      bars ищет по Markdown-документации проектов. Укажите папку проекта — bars найдёт в ней документацию,
      проиндексирует её и будет следить за правками.
    </p>
    <div class="acts"><button class="btn" onclick={() => app.addProject()}><Icon name="plus" size={14} width={2.4} />Добавить проект…</button></div>
    <div class="alt">
      <p>
        Проект появится и сам, когда вы начнёте сессию Claude Code или Codex в папке с документацией — если bars
        установлен в клиент.
      </p>
      <button class="link" onclick={() => app.openSettings("clients")}>Установить bars в клиенты →</button>
    </div>
  </div>
</main>

<style>
  main { overflow-y: auto; min-height: 0; }
  .alt { margin-top: 34px; padding-top: 20px; border-top: 1px solid var(--line); }
  .alt p { margin-bottom: 8px; }
</style>
