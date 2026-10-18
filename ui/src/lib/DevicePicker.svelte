<script lang="ts">
  import type { ModelRuntime, ModelSourceName } from "./types";
  let { value = $bindable(), source, cpuOnly = false, gpuOnly = false, runtime, role }: {
    value: "auto" | "cpu"; source: ModelSourceName; cpuOnly?: boolean; gpuOnly?: boolean;
    runtime?: ModelRuntime; role: string;
  } = $props();
  const states: Record<NonNullable<ModelRuntime>["state"], string> = {
    unloaded: "не загружена", loading: "загружается", ready: "готова", failed: "ошибка загрузки",
  };
</script>

<section aria-label={`Устройство ${role}`}>
  {#if source !== "builtin"}
    <p>Устройством управляет провайдер · устройство сервера неизвестно</p>
  {:else if cpuOnly}
    <p>Только CPU · этот движок модели не поддерживает GPU</p>
  {:else if gpuOnly}
    <button class:on={value === "auto"} aria-pressed={value === "auto"} onclick={() => value = "auto"}>DirectML</button>
    <p>FP16 требует DirectML. Для CPU выберите обычную bge-m3; потребуется переиндексация.</p>
  {:else}
    <div class="choices">
      <button class:on={value === "auto"} aria-pressed={value === "auto"} onclick={() => value = "auto"}>GPU, если доступен</button>
      <button class:on={value === "cpu"} aria-pressed={value === "cpu"} onclick={() => value = "cpu"}>Только CPU</button>
    </div>
    <small>Смена устройства не требует переиндексации. Применяется после «Сохранить».</small>
  {/if}
  {#if runtime}
    <p class="runtime">Текущая конфигурация: {runtime.model} · {states[runtime.state]}
      {#if runtime.actual} · <b>{({directml:"DirectML",vulkan:"Vulkan",metal:"Metal",cpu:"CPU"} as Record<string,string>)[runtime.actual] ?? runtime.actual}</b>{/if}
      {#if runtime.precision} · {runtime.precision.toUpperCase()}{/if}
    </p>
    {#if runtime.fallback_reason}<p class="fallback">GPU недоступен; используется CPU. {runtime.fallback_reason}</p>{/if}
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
