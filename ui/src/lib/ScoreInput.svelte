<script lang="ts">
  import { decimal } from "./format";
  import { parseScore } from "./settings-logic";

  // Оценка от 0 до 1: ползунок с полем, запятая как десятичный знак (settings-window.md, «Поиск»).
  let {
    value = $bindable(),
    slider = true,
    label,
  }: { value: number; slider?: boolean; label: string } = $props();

  let text = $state(decimal(value));
  let editing = false;

  $effect(() => {
    const shown = decimal(value);
    if (!editing) text = shown;
  });

  function commit() {
    editing = false;
    const parsed = parseScore(text);
    if (Number.isFinite(parsed)) value = Math.round(Math.min(1, Math.max(0, parsed)) * 10000) / 10000;
    text = decimal(value);
  }
</script>

<div class="range" class:plain={!slider}>
  {#if slider}
    <input
      type="range"
      min="0"
      max="100"
      value={Math.round(value * 100)}
      aria-label={label}
      oninput={(event) => (value = Number(event.currentTarget.value) / 100)}
    />
  {/if}
  <input
    class="inp"
    inputmode="decimal"
    aria-label={label}
    bind:value={text}
    onfocus={() => (editing = true)}
    onblur={commit}
    onkeydown={(event) => event.key === "Enter" && commit()}
  />
</div>

<style>
  .range { display: grid; grid-template-columns: 1fr 64px; gap: 10px; align-items: center; }
  .range.plain { grid-template-columns: 70px; }
  input[type="range"] { width: 100%; accent-color: var(--accent); }
</style>
