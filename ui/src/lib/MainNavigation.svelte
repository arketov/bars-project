<script lang="ts">
  import { app } from "./app.svelte";
</script>

<nav aria-label="Разделы главного окна">
  <button class:active={app.view === "main"} aria-current={app.view === "main" ? "page" : undefined} onclick={() => app.view = "main"}>Поиск</button>
  <button class:active={app.view === "graph"} aria-current={app.view === "graph" ? "page" : undefined} onclick={() => app.view = "graph"}>Граф</button>
  <button title="Редактор запланирован, пока не реализован" onclick={() => app.flash("Редактор запланирован; пока не реализован")}>Редактор</button>
</nav>

<style>
  nav { display: flex; align-items: center; gap: 12px; padding: 8px 16px 0; }
  button { padding: 0; border: 0; background: transparent; color: var(--muted); font-size: 12px; font-style: italic; }
  button:hover, button.active { color: var(--accent-text); }
  button:focus-visible { outline: 2px solid var(--accent); outline-offset: 3px; border-radius: 2px; }
  @media (max-width: 760px) { nav { padding: 8px 8px 0; } }
</style>
