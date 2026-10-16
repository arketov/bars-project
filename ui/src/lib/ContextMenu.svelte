<script lang="ts">
  import { onMount, tick } from "svelte";
  import { app } from "./app.svelte";
  import { itemsFor, place, run, type MenuItem } from "./contextmenu";

  let items = $state<MenuItem[]>([]);
  let target: EventTarget | null = null;
  let at = $state({ left: 0, top: 0 });
  let box = $state<HTMLDivElement>();

  function close() {
    items = [];
    target = null;
  }

  async function open(event: MouseEvent) {
    // Стандартное меню webview выключено всегда: своё — только там, где есть что показать.
    event.preventDefault();
    const found = itemsFor(event.target);
    if (!found.length) return close();
    target = event.target;
    at = { left: event.clientX, top: event.clientY };
    items = found;
    await tick();
    if (box) at = place(event.clientX, event.clientY, box.offsetWidth, box.offsetHeight, innerWidth, innerHeight);
  }

  async function pick(item: MenuItem) {
    const where = target;
    close();
    const problem = await run(item.action, where);
    if (problem) app.flash(problem, true);
  }

  onMount(() => {
    const onKey = (event: KeyboardEvent) => event.key === "Escape" && close();
    addEventListener("contextmenu", open);
    addEventListener("keydown", onKey);
    addEventListener("blur", close);
    addEventListener("resize", close);
    addEventListener("scroll", close, true);
    return () => {
      removeEventListener("contextmenu", open);
      removeEventListener("keydown", onKey);
      removeEventListener("blur", close);
      removeEventListener("resize", close);
      removeEventListener("scroll", close, true);
    };
  });
</script>

<svelte:window onmousedown={(event) => box && !box.contains(event.target as Node) && close()} />

{#if items.length}
  <div
    class="cmenu"
    bind:this={box}
    role="menu"
    tabindex="-1"
    style="left: {at.left}px; top: {at.top}px"
    onmousedown={(event) => event.preventDefault()}
  >
    {#each items as item (item.action)}
      <button role="menuitem" onclick={() => pick(item)}>{item.label}</button>
    {/each}
  </div>
{/if}

<style>
  .cmenu { position: fixed; z-index: 90; min-width: 170px; background: var(--panel); border: 1px solid var(--line-2); border-radius: 10px; box-shadow: 0 10px 30px rgba(0,0,0,.2); padding: 5px; }
  .cmenu button { display: block; width: 100%; text-align: left; border: 0; background: none; padding: 7px 10px; border-radius: 7px; font-size: 13px; color: var(--text); }
  .cmenu button:hover { background: var(--accent-soft); color: var(--accent-text); }
</style>
