<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";

  // Цвет не единственный носитель: у каждого вида свой значок и текст (palette.md, error-states.md).
  let {
    kind,
    title,
    text,
    children,
  }: { kind: "warn" | "bad" | "info" | "acc"; title: string; text?: string; children?: Snippet } = $props();

  const icon = $derived(kind === "bad" ? "error" : kind === "warn" ? "warn" : "info");
</script>

<div class="banner {kind}" role={kind === "bad" ? "alert" : "status"}>
  <span class="ic"><Icon name={icon} size={18} width={2} /></span>
  <div class="bt"><b>{title}</b>{#if text}<span>{text}</span>{/if}</div>
  {#if children}<div class="acts">{@render children()}</div>{/if}
</div>
