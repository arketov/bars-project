<script lang="ts">
  // Выбор цвета и иконки (Т-38): небольшая панель у строки дерева; выбор применяется сразу.
  import { onMount } from "svelte";
  import { app } from "./app.svelte";
  import { appearance } from "./appearance.svelte";
  import { COLORS, COLOR_TITLE, ICONS, ICON_TITLE, colorVar, validColor, validIcon, type ColorId, type IconId } from "./appearance";
  import FolderIcon from "./FolderIcon.svelte";

  let { kind, path, label, x, y, onclose }: {
    kind: "folder" | "document"; path: string; label: string; x: number; y: number; onclose: () => void;
  } = $props();

  let box = $state<HTMLDivElement>();
  let at = $state({ left: 0, top: 0 });
  const entry = $derived((kind === "folder" ? appearance.data.folders : appearance.data.documents)[path]);
  const color = $derived(validColor(entry?.color));
  const icon = $derived(validIcon(entry?.icon));

  async function save(nextColor: ColorId | null, nextIcon: IconId | null) {
    const problem = await appearance.set(kind, path, nextColor, kind === "folder" ? nextIcon : null);
    if (problem) app.flash(problem, true);
  }

  function arrows(event: KeyboardEvent) {
    if (event.key === "Escape") { event.preventDefault(); onclose(); return; }
    if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key) || !(event.target instanceof HTMLElement)) return;
    const group = event.target.closest<HTMLElement>("[data-grid]");
    if (!group) return;
    const buttons = [...group.querySelectorAll<HTMLButtonElement>("button")];
    const index = buttons.indexOf(event.target as HTMLButtonElement);
    const step = event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 1;
    event.preventDefault();
    buttons[(index + step + buttons.length) % buttons.length]?.focus();
  }

  onMount(() => {
    const width = box?.offsetWidth ?? 0, height = box?.offsetHeight ?? 0;
    at = { left: Math.max(4, Math.min(x, innerWidth - width - 4)), top: Math.max(4, Math.min(y, innerHeight - height - 4)) };
    box?.querySelector<HTMLButtonElement>("button[aria-pressed='true'], button")?.focus({ preventScroll: true });
  });
</script>

<svelte:window onmousedown={(event) => { if (box && event.target instanceof Node && !box.contains(event.target)) onclose(); }} onblur={onclose} />

<div bind:this={box} class="picker" role="dialog" aria-label={`Цвет${kind === "folder" ? " и иконка" : ""}: ${label}`} tabindex="-1" style="left: {at.left}px; top: {at.top}px" onkeydown={arrows}>
  <p class="name" title={path}>{label}</p>
  <div class="grid colors" role="group" aria-label="Цвет" data-grid>
    <button class="none" class:on={!color} aria-pressed={!color} title="Без цвета" aria-label="Без цвета" onclick={() => save(null, icon)}></button>
    {#each COLORS as id (id)}
      <button class="swatch" class:on={color === id} aria-pressed={color === id} title={COLOR_TITLE[id]} aria-label={COLOR_TITLE[id]} style="background: {colorVar(id)}" onclick={() => save(id, icon)}></button>
    {/each}
  </div>
  {#if kind === "folder"}
    <div class="grid icons" role="group" aria-label="Иконка" data-grid>
      {#each ICONS as id (id)}
        <button class="ic" class:on={(icon ?? "folder") === id} aria-pressed={(icon ?? "folder") === id} title={ICON_TITLE[id]} aria-label={ICON_TITLE[id]} style="color: {colorVar(color) ?? 'var(--muted)'}" onclick={() => save(color, id === "folder" ? null : id)}><FolderIcon name={id} size={16} /></button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .picker { position: fixed; z-index: 1000; width: 216px; padding: 8px; display: grid; gap: 8px; background: var(--panel); border: 1px solid var(--line); border-radius: 10px; box-shadow: var(--shadow); font-size: 12px; }
  .name { margin: 0; padding: 0 2px; color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .grid { display: grid; gap: 6px; }
  .colors { grid-template-columns: repeat(5, 1fr); }
  .icons { grid-template-columns: repeat(5, 1fr); }
  button { cursor: pointer; border: 1px solid var(--line-2); background: var(--panel); border-radius: 8px; padding: 0; }
  button:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .swatch, .none { height: 28px; }
  .swatch { border-color: transparent; }
  .none { background: linear-gradient(135deg, transparent 46%, var(--faint) 46% 54%, transparent 54%), var(--panel); }
  .ic { height: 30px; display: grid; place-items: center; }
  .on { box-shadow: 0 0 0 2px var(--panel), 0 0 0 4px var(--accent); }
</style>
