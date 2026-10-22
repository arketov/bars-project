<script lang="ts">
  // Диалог закрытия с несохранёнными правками (НФТ-5): называет документы и причину, по умолчанию — «Вернуться».
  import { tick } from "svelte";
  import { unsavedPrompt } from "./unsaved.svelte";
  import type { EditorBuffer } from "./editors.svelte";

  let back = $state<HTMLButtonElement>();
  const pending = $derived(unsavedPrompt.pending);
  $effect(() => { if (pending) void tick().then(() => back?.focus()); });
  const reason = (buffer: EditorBuffer) =>
    buffer.status === "conflict" ? buffer.message || "Файл изменён на диске"
      : buffer.status === "error" ? `Не записано: ${buffer.message}`
      : "Правки не записаны";
  function keydown(event: KeyboardEvent) {
    if (event.key === "Escape") { event.stopPropagation(); pending?.resolve("cancel"); }
  }
</script>

{#if pending}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="backdrop" role="alertdialog" aria-modal="true" aria-labelledby="unsaved-title" tabindex="-1" data-command-blocked="true" onkeydown={keydown}>
    <div class="box">
      <h2 id="unsaved-title">Есть несохранённые правки</h2>
      <p>Эти документы не записаны на диск. Если закрыть без записи, правки пропадут.</p>
      <ul>{#each pending.items as buffer (buffer.path)}<li><b>{buffer.path}</b><small>{reason(buffer)}</small></li>{/each}</ul>
      <div class="actions">
        <button bind:this={back} class="primary" onclick={() => pending.resolve("cancel")}>Вернуться</button>
        <button class="danger" onclick={() => pending.resolve("discard")}>Закрыть без записи</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop { position: fixed; inset: 0; z-index: 60; display: grid; place-items: center; background: rgb(0 0 0 / 0.35); }
  .box { width: min(460px, calc(100vw - 32px)); max-height: 80vh; overflow: auto; padding: 14px 16px; border: 1px solid var(--line); border-radius: 8px; background: var(--panel); color: var(--text); box-shadow: var(--shadow); }
  h2 { margin: 0 0 6px; font-size: 14px; } p { margin: 0 0 8px; font-size: 12px; color: var(--muted); }
  ul { margin: 0 0 12px; padding: 0; list-style: none; } li { padding: 4px 0; border-top: 1px solid var(--line); }
  b, small { display: block; overflow-wrap: anywhere; } b { font: 11.5px var(--mono); } small { font-size: 11.5px; color: var(--muted); }
  .actions { display: flex; gap: 8px; justify-content: flex-end; }
  button { padding: 5px 12px; border: 1px solid var(--line); border-radius: 5px; background: transparent; color: var(--text); }
  .primary { background: var(--accent); border-color: var(--accent); color: #fff; } .danger { color: var(--danger, #b3261e); }
</style>
