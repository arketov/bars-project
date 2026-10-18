<script lang="ts">
  import type { ModelRuntime, ModelSourceName } from "./types";
  let { value = $bindable(), source, cpuOnly = false, runtime, role }: {
    value: "auto" | "cpu"; source: ModelSourceName; cpuOnly?: boolean;
    runtime?: ModelRuntime; role: string;
  } = $props();
</script>

<section aria-label={`${role} device`}>
  {#if source !== "builtin"}
    <p>Device managed by the provider · server device unknown</p>
  {:else if cpuOnly}
    <p>CPU only · this model backend does not support GPU</p>
  {:else}
    <div class="choices">
      <button class:on={value === "auto"} aria-pressed={value === "auto"} onclick={() => value = "auto"}>GPU if available</button>
      <button class:on={value === "cpu"} aria-pressed={value === "cpu"} onclick={() => value = "cpu"}>CPU only</button>
    </div>
    <small>Changing the device does not require reindexing. Applies after Save.</small>
  {/if}
  {#if runtime}
    <p class="runtime">Current configuration: {runtime.model} · {runtime.state}
      {#if runtime.actual} · <b>{runtime.actual === "directml" ? "DirectML" : "CPU"}</b>{/if}
    </p>
    {#if runtime.fallback_reason}<p class="fallback">GPU unavailable; using CPU. {runtime.fallback_reason}</p>{/if}
  {/if}
</section>

<style>
  section { margin-top:16px; padding-top:12px; border-top:1px solid var(--line); }
  .choices { display:flex; gap:8px; margin-bottom:8px; }
  button { padding:7px 12px; border:1px solid var(--line); border-radius:8px; color:var(--text); background:var(--panel); cursor:pointer; }
  button.on { color:var(--accent-text); background:var(--accent-soft); border-color:var(--accent); }
  small,.runtime { color:var(--muted); } p { margin:8px 0; }
  .fallback { color:var(--mid); overflow-wrap:anywhere; }
</style>
