<script lang="ts">
  import { DEFAULT_KEY_NAMES, settings } from "./settings.svelte";
  import type { EmbedDoc, RerankDoc } from "./types";

  // Поля внешнего провайдера и локального адреса (settings-window.md, «Вкладка Модели»).
  // Ключ уходит в хранилище ОС; в поле виден только запрос на ввод и имя записи.
  let { cfg = $bindable(), kind }: { cfg: EmbedDoc | RerankDoc; kind: "embed" | "rerank" } = $props();

  let editing = $state(false);
  let secret = $state("");
  const name = $derived(cfg.api_key_name ?? "");
  const present = $derived(!!name && settings.secrets[name] === true);
  const errors = $derived(settings.errors);

  async function saveKey() {
    const target = name || DEFAULT_KEY_NAMES[kind];
    if (!secret) return;
    if (await settings.setSecret(target, secret)) {
      cfg.api_key_name = target;
      secret = "";
      editing = false;
    }
  }
</script>

<div class="field">
  <div class="lbl"><b>Адрес API</b><small>{cfg.source === "ollama" ? "Корень сервера Ollama, без /api и /v1" : cfg.source === "remote" ? "OpenAI-совместимый endpoint" : "Локальный OpenAI-совместимый сервер"}</small></div>
  <div>
    <input class="inp mono" placeholder={cfg.source === "ollama" ? "http://127.0.0.1:11434" : "https://api.example.com/v1"} bind:value={cfg.base_url} />
    {#if errors[`${kind}.base_url`]}<small class="err">{errors[`${kind}.base_url`]}</small>{/if}
  </div>
</div>
{#if cfg.source === "ollama"}
  <p>Модель должна быть установлена в Ollama. Видеокартой и выгрузкой управляет Ollama; rerank выберите отдельно. После замены весов под прежним именем нужна полная переиндексация.</p>
{/if}
<div class="field">
  <div class="lbl"><b>Модель</b></div>
  <div>
    <input class="inp mono" bind:value={cfg.model} />
    {#if errors[`${kind}.model`]}<small class="err">{errors[`${kind}.model`]}</small>{/if}
  </div>
</div>
{#if cfg.source === "remote"}
  <div class="field">
    <div class="lbl">
      <b>Ключ</b>
      <small>
        {#if name}Хранилище ОС, запись <code>bars/{name}</code> — {present ? "ключ задан" : "ключа в хранилище нет"}{:else}Хранится в хранилище ОС, в файле настроек остаётся только имя записи{/if}
      </small>
    </div>
    <button class="ghost" onclick={() => (editing = !editing)}>{present ? "Заменить ключ…" : "Задать ключ…"}</button>
  </div>
  {#if editing}
    <div class="keyrow">
      <input class="inp mono" type="password" autocomplete="off" placeholder="Вставьте ключ" bind:value={secret} />
      <button class="btn" disabled={!secret} onclick={saveKey}>В хранилище</button>
      <button class="ghost" onclick={() => { editing = false; secret = ""; }}>Отмена</button>
    </div>
  {/if}
  <div class="field">
    <div class="lbl"><b>Переменная окружения</b><small>Если задана — используется вместо ключа</small></div>
    <div>
      <input class="inp mono" placeholder="OPENROUTER_API_KEY" bind:value={cfg.api_key_env} />
      {#if errors[`${kind}.key`]}<small class="err">{errors[`${kind}.key`]}</small>{/if}
    </div>
  </div>
{/if}

<style>
  .field { display: grid; grid-template-columns: 1fr 260px; gap: 16px; align-items: center; padding: 9px 0; }
  .field + .field, .keyrow + .field { border-top: 1px dashed var(--line); }
  .lbl b { display: block; font-weight: 600; font-size: 13.5px; }
  .lbl small { display: block; color: var(--muted); font-size: 12px; }
  .lbl code { font: 11.5px var(--mono); color: var(--faint); }
  .keyrow { display: flex; gap: 8px; padding: 4px 0 10px; }
  .err { display: block; color: var(--bad); font-size: 12px; margin-top: 3px; }
</style>
