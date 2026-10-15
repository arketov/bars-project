<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "./app.svelte";
  import { copyText } from "./api";
  import Icon from "./Icon.svelte";
  import ProviderFields from "./ProviderFields.svelte";
  import ScoreInput from "./ScoreInput.svelte";
  import { settings } from "./settings.svelte";
  import { clone } from "./settings-logic";
  import { theme, type ThemePref } from "./theme.svelte";
  import type { EmbedDoc, InstallClientName, ModelInfo, ModelSourceName, RerankDoc } from "./types";

  onMount(() => void settings.load());

  const TABS = [
    ["models", "Модели"],
    ["search", "Поиск"],
    ["chunks", "Нарезка"],
    ["clients", "Клиенты"],
    ["system", "Запуск"],
    ["look", "Оформление"],
  ] as const;
  const SOURCES: [ModelSourceName, string][] = [
    ["builtin", "Встроенная"],
    ["remote", "Удалённый провайдер"],
    ["local-openai", "Локальный адрес"],
  ];
  const THEMES: [ThemePref, string][] = [
    ["system", "Как в системе"],
    ["light", "Светлая"],
    ["dark", "Тёмная"],
  ];
  const CLIENT_NAMES: Record<InstallClientName, [string, string]> = {
    "claude-code": ["CC", "Claude Code"],
    codex: ["Cx", "Codex"],
  };

  const draft = $derived(settings.draft);
  const errors = $derived(settings.errors);
  const hasErrors = $derived(Object.keys(errors).length > 0);
  const tab = $derived(app.settingsTab);

  function setSource(kind: "embed" | "rerank", source: ModelSourceName) {
    if (!draft || !settings.saved) return;
    const cfg: EmbedDoc | RerankDoc = draft[kind];
    if (cfg.source === source) return;
    const saved = settings.saved[kind];
    if (saved.source === source) {
      // Вернулись к сохранённому способу: берём его поля, а не пустые.
      Object.assign(cfg, clone({ source: saved.source, model: saved.model, base_url: saved.base_url, api_key_name: saved.api_key_name, api_key_env: saved.api_key_env }));
      return;
    }
    cfg.source = source;
    if (source === "builtin") {
      cfg.model = (kind === "embed" ? settings.models?.embed[0]?.id : settings.models?.rerank[0]?.id) ?? cfg.model;
    } else {
      cfg.model = "";
      if (source === "local-openai") cfg.api_key_name = null;
    }
  }

  function modelNote(model: ModelInfo): string {
    const parts = [];
    if (model.dim) parts.push(`${model.dim} измерений`);
    parts.push(model.backend === "onnx" ? "ONNX" : "candle");
    return parts.join(" · ");
  }

  const savedModel = $derived(settings.saved?.embed.model);

  async function copy(text: string, what: string) {
    await copyText(text);
    app.flash(`${what} скопировано`);
  }

  function clientBadge(status: { installed: boolean; stale: boolean; needs_hook_approval: boolean }) {
    if (status.stale) return { cls: "warn", text: "пути устарели" };
    if (status.needs_hook_approval) return { cls: "warn", text: "хук ждёт одобрения" };
    if (status.installed) return { cls: "ok", text: "установлено" };
    return { cls: "off", text: "не установлено" };
  }
</script>

<div class="app has-savebar">
  <aside>
    <div class="brand">
      <div class="logo">b</div>
      <div><b>bars</b><small>настройки приложения</small></div>
    </div>
    <button class="back" onclick={() => (app.view = "main")}><Icon name="back" size={15} />К поиску</button>
    <div class="nav">
      {#each TABS as [id, title] (id)}
        <button class:on={tab === id} onclick={() => (app.settingsTab = id)}>
          {title}
          {#if id === "clients" && settings.clientsNeedAction > 0}<span class="badge warn">{settings.clientsNeedAction}</span>{/if}
        </button>
      {/each}
    </div>
    <div class="sfoot"><span class="mono file">{settings.file || "~/.bars/settings.toml"}</span></div>
  </aside>

  <main>
    <div class="page">
      {#if settings.loadError}
        <div class="banner bad"><span class="ic"><Icon name="error" size={18} width={2} /></span><div class="bt"><b>Настройки не прочитаны</b><span>{settings.loadError}</span></div></div>
      {/if}
      {#each settings.warnings as warning (warning)}
        <div class="note warn"><Icon name="warn" size={16} /><span>{warning}</span></div>
      {/each}

      {#if draft && tab === "models"}
        <h1>Модели</h1>
        <p class="lead">Чем bars превращает документы и запросы в векторы и как переоценивает кандидатов.</p>

        <div class="group">
          <div class="ghead"><div class="grow"><h2>Embed-модель</h2><p>Одна на все проекты. Индекс проекта строится этой моделью.</p></div></div>
          <div class="gbody">
            <div class="seg">
              {#each SOURCES as [id, title] (id)}
                <button class:on={draft.embed.source === id} onclick={() => setSource("embed", id)}>{title}</button>
              {/each}
            </div>
            {#if draft.embed.source === "builtin"}
              {#each settings.models?.embed ?? [] as model (model.id)}
                <div
                  class="model"
                  class:on={draft.embed.model === model.id}
                  role="radio"
                  aria-checked={draft.embed.model === model.id}
                  tabindex="0"
                  onclick={() => (draft.embed.model = model.id)}
                  onkeydown={(event) => (event.key === "Enter" || event.key === " ") && (draft.embed.model = model.id)}
                >
                  <span class="radio"></span>
                  <span class="mn">
                    <b>{model.id}</b>
                    <small>{modelNote(model)}{model.id === savedModel ? " · в индексах сейчас" : ""}</small>
                    {#each model.warnings as warning (warning)}<small class="warnline">{warning}</small>{/each}
                  </span>
                  {#if model.downloaded}
                    <span class="badge ok">скачана</span>
                  {:else if model.downloading}
                    <span class="badge acc">скачивается…</span>
                  {:else}
                    <button class="ghost" onclick={(event) => { event.stopPropagation(); void settings.download(model.id); }}>Скачать</button>
                  {/if}
                </div>
              {/each}
              <div class="note info"><Icon name="info" size={16} /><span>Модели скачиваются в <span class="mono">{settings.models?.models_dir ?? "~/.bars/models"}</span>. Не скачанная модель скачается и сама при первом использовании.</span></div>
            {:else}
              <ProviderFields bind:cfg={draft.embed} kind="embed" />
            {/if}
            {#if settings.willReindex}
              <div class="note warn">
                <Icon name="warn" size={16} />
                <span>После сохранения проекты, построенные другой моделью, потребуют переиндексации. Она начнётся только после вашего согласия; до него поиск по такому проекту не работает (Т-25).</span>
              </div>
            {/if}
          </div>
        </div>

        <div class="group">
          <div class="ghead">
            <div class="grow"><h2>Rerank</h2><p>Переоценивает кандидатов после слияния. С ним результатов по умолчанию {draft.search.results_reranked}, без него — {draft.search.results}.</p></div>
            <button class="switch" class:on={draft.rerank.enabled} role="switch" aria-checked={draft.rerank.enabled} aria-label="Rerank" onclick={() => (draft.rerank.enabled = !draft.rerank.enabled)}></button>
          </div>
          <div class="gbody" class:dim={!draft.rerank.enabled}>
            <div class="seg">
              {#each SOURCES as [id, title] (id)}
                <button class:on={draft.rerank.source === id} onclick={() => setSource("rerank", id)}>{title}</button>
              {/each}
            </div>
            {#if draft.rerank.source === "builtin"}
              {#each settings.models?.rerank ?? [] as model (model.id)}
                <div class="model on">
                  <span class="radio"></span>
                  <span class="mn"><b>{model.id}</b><small>{modelNote(model)} · по умолчанию</small></span>
                  {#if model.downloaded}<span class="badge ok">скачана</span>
                  {:else if model.downloading}<span class="badge acc">скачивается…</span>
                  {:else}<button class="ghost" onclick={() => settings.download(model.id)}>Скачать</button>{/if}
                </div>
              {/each}
              <div class="note info"><Icon name="info" size={16} /><span>Встроенный rerank тратит секунды на запрос; быстрый rerank даёт внешний по API.</span></div>
            {:else}
              <ProviderFields bind:cfg={draft.rerank} kind="rerank" />
            {/if}
          </div>
        </div>

        <div class="group">
          <div class="ghead"><div class="grow"><h2>Устройство</h2><p>Где считают встроенные модели. Смена устройства переиндексации не требует.</p></div></div>
          <div class="gbody">
            <div class="seg">
              <button class:on={draft.device.accelerator === "auto"} onclick={() => (draft.device.accelerator = "auto")}>Видеокарта, если есть</button>
              <button class:on={draft.device.accelerator === "cpu"} onclick={() => (draft.device.accelerator = "cpu")}>Только процессор</button>
            </div>
          </div>
        </div>
      {:else if draft && tab === "search"}
        <h1>Поиск</h1>
        <p class="lead">Значения по умолчанию для всех проектов. Проект может переопределить их в <span class="mono">.bars/config.toml</span>, агент — в вызове.</p>

        <div class="group">
          <div class="ghead"><h2>Число результатов</h2></div>
          <div class="gbody">
            <div class="field"><div class="lbl"><b>Результатов с реранком</b></div><div><input class="inp" type="number" min="1" bind:value={draft.search.results_reranked} />{#if errors["search.results_reranked"]}<small class="err">{errors["search.results_reranked"]}</small>{/if}</div></div>
            <div class="field"><div class="lbl"><b>Результатов без реранка</b></div><div><input class="inp" type="number" min="1" bind:value={draft.search.results} />{#if errors["search.results"]}<small class="err">{errors["search.results"]}</small>{/if}</div></div>
            <div class="field"><div class="lbl"><b>Кандидатов от каждого поиска</b><small>Полнотекстового и векторного <code>candidates</code></small></div><div><input class="inp" type="number" min="1" bind:value={draft.search.candidates} />{#if errors["search.candidates"]}<small class="err">{errors["search.candidates"]}</small>{/if}</div></div>
            <div class="field"><div class="lbl"><b>Кандидатов на реранк</b><small>Лучшие после слияния <code>rerank_candidates</code></small></div><div><input class="inp" type="number" min="1" bind:value={draft.search.rerank_candidates} />{#if errors["search.rerank_candidates"]}<small class="err">{errors["search.rerank_candidates"]}</small>{/if}</div></div>
            <div class="field">
              <div class="lbl"><b>Пределы для агента</b><small>Больше агент запросить не может <code>max_results · max_candidates · max_rerank_candidates</code></small></div>
              <div class="pair">
                <input class="inp" type="number" min="1" aria-label="max_results" bind:value={draft.search.max_results} /> ·
                <input class="inp" type="number" min="1" aria-label="max_candidates" bind:value={draft.search.max_candidates} /> ·
                <input class="inp" type="number" min="1" aria-label="max_rerank_candidates" bind:value={draft.search.max_rerank_candidates} />
              </div>
            </div>
          </div>
        </div>

        <div class="group">
          <div class="ghead"><div class="grow"><h2>Порог релевантности</h2><p>Результаты с оценкой ниже порога не попадают в выдачу — ни окну, ни агенту, ни CLI.</p></div></div>
          <div class="gbody">
            <div class="field"><div class="lbl"><b>С реранком</b><small>Оценка реранка <code>min_score_rerank</code></small></div><div><ScoreInput bind:value={draft.search.min_score_rerank} label="Порог с реранком" />{#if errors["search.min_score_rerank"]}<small class="err">{errors["search.min_score_rerank"]}</small>{/if}</div></div>
            <div class="field"><div class="lbl"><b>Без реранка</b><small>Косинусная близость <code>min_score_vector</code></small></div><div><ScoreInput bind:value={draft.search.min_score_vector} label="Порог без реранка" />{#if errors["search.min_score_vector"]}<small class="err">{errors["search.min_score_vector"]}</small>{/if}</div></div>
            <div class="field">
              <div class="lbl"><b>Пределы для агента</b><small>Порог агента вне пределов прижимается к границе <code>agent_min_score</code></small></div>
              <div>
                <div class="pair">от <ScoreInput bind:value={draft.search.agent_min_score[0]} slider={false} label="Нижний предел порога агента" /> до <ScoreInput bind:value={draft.search.agent_min_score[1]} slider={false} label="Верхний предел порога агента" /></div>
                {#if errors["search.agent_min_score"]}<small class="err">{errors["search.agent_min_score"]}</small>{/if}
              </div>
            </div>
          </div>
        </div>
      {:else if draft && tab === "chunks"}
        <h1>Нарезка</h1>
        <p class="lead">Документы режутся по заголовкам. Раздел больше предела делится на несколько чанков с той же цепочкой заголовков. Смена нарезки согласия не требует: проекты проверяются заново, векторы неизменившихся чанков берутся из индекса.</p>
        <div class="group">
          <div class="ghead"><h2>Размер чанка</h2></div>
          <div class="gbody">
            <div class="field"><div class="lbl"><b>Минимум, знаков</b><small>Раздел короче склеивается со следующим</small></div><div><input class="inp" type="number" min="1" bind:value={draft.chunking.min_chars} />{#if errors["chunking.min_chars"]}<small class="err">{errors["chunking.min_chars"]}</small>{/if}</div></div>
            <div class="field"><div class="lbl"><b>Максимум, знаков</b><small>Раздел длиннее режется на несколько чанков</small></div><div><input class="inp" type="number" min="1" bind:value={draft.chunking.max_chars} />{#if errors["chunking.max_chars"]}<small class="err">{errors["chunking.max_chars"]}</small>{/if}</div></div>
          </div>
        </div>
      {:else if tab === "clients"}
        <h1>Клиенты</h1>
        <p class="lead">Установка прописывает MCP-сервер <span class="mono">bars</span> и хук начала сессии. Чужие записи не трогаются, перед правкой файла рядом остаётся копия <span class="mono">.bars-backup</span>.</p>
        <div class="group">
          {#each settings.clients as client (client.client)}
            {@const badge = clientBadge(client)}
            {@const [icon, title] = CLIENT_NAMES[client.client]}
            <div class="client">
              <div class="cicon">{icon}</div>
              <div class="cn">
                <b>{title}</b><span class="badges"><span class="badge {badge.cls}">{badge.text}</span></span>
                <small class="mono">{client.files.join(" · ")}</small>
              </div>
              <div class="acts">
                <button class="ghost" disabled={settings.busy === client.client} onclick={() => settings.install(client.client)}>{client.installed ? "Переустановить" : "Установить"}</button>
                {#if client.installed}<button class="ghost danger" disabled={settings.busy === client.client} onclick={() => settings.uninstall(client.client)}>Удалить</button>{/if}
              </div>
              {#if client.stale}
                <div class="note warn"><Icon name="warn" size={16} /><span>Записи указывают на старое расположение bars (приложение переехало или обновилось). Нажмите «Переустановить».</span></div>
              {:else if client.needs_hook_approval}
                <div class="note warn"><Icon name="warn" size={16} /><span>Codex не запускает новый хук, пока его не одобрить: выполните <span class="mono">/hooks</span> в Codex. MCP уже работает. Одобрен ли хук, bars не видит, поэтому напоминание остаётся.</span></div>
              {/if}
            </div>
          {:else}
            <div class="client"><div class="cn"><small>Состояние клиентов не прочитано.</small></div></div>
          {/each}
          <div class="client">
            <div class="cicon">+</div>
            <div class="cn"><b>Другой MCP-клиент</b><small>Подключается вручную, без хука: проекты регистрируются в окне или через CLI.</small></div>
            <div class="acts"><button class="ghost" disabled={!settings.instruction} onclick={() => copy(settings.instruction?.mcp_config ?? "", "Конфиг")}>Копировать конфиг</button></div>
          </div>
        </div>

        <div class="group">
          <div class="ghead">
            <div class="grow"><h2>Инструкция для агента</h2><p>Передайте агенту — он подключит bars сам.</p></div>
            <button class="btn" disabled={!settings.instruction} onclick={() => copy(settings.instruction?.text ?? "", "Инструкция")}>Копировать</button>
          </div>
          <div class="gbody"><pre class="instr">{settings.instruction?.text ?? "—"}</pre></div>
        </div>
      {:else if tab === "system"}
        <h1>Запуск</h1>
        <p class="lead">bars живёт в трее и сам поднимается, когда его зовёт агент. Автозапуск нужен, чтобы индекс обновлялся и до первого обращения. Применяется сразу, без «Сохранить».</p>
        <div class="group">
          <div class="gbody">
            {#if settings.autostart?.supported}
              <label class="check">
                <input type="checkbox" checked={settings.autostart.enabled} disabled={settings.busy === "autostart"} onchange={(e) => settings.setAutostart(e.currentTarget.checked)} />
                <span><b>Запускать при входе в систему</b><small>bars стартует свёрнутым в трей, окно не открывается.</small></span>
              </label>
              {#if settings.autostart.stale}
                <div class="note warn"><Icon name="warn" size={16} /><span>Запись автозапуска указывает на другое расположение bars (приложение переехало). При следующем запуске bars исправит её сам.</span></div>
              {/if}
            {:else if settings.autostart}
              <small>Автозапуск на этой системе пока не поддерживается.</small>
            {:else}
              <small>Состояние автозапуска не прочитано.</small>
            {/if}
          </div>
        </div>
      {:else if tab === "look"}
        <h1>Оформление</h1>
        <p class="lead">Тема окна. Применяется сразу, без «Сохранить».</p>
        <div class="group">
          <div class="gbody">
            <div class="themes">
              {#each THEMES as [id, title] (id)}
                <button class="theme" class:on={theme.pref === id} onclick={() => theme.set(id)}>
                  <div class="pv {id === 'system' ? 's' : id === 'light' ? 'l' : 'd'}">
                    {#if id === "system"}<b class="sysbar"></b>{:else}<i></i><i><b></b></i>{/if}
                  </div>
                  {title}
                </button>
              {/each}
            </div>
          </div>
        </div>
      {/if}
    </div>
  </main>
</div>

{#if tab !== "look" && tab !== "clients" && tab !== "system"}
  <div class="savebar">
    <span class="grow">
      {#if hasErrors}Есть ошибки в значениях — исправьте их, чтобы сохранить{:else if settings.dirty}Изменения не сохранены{:else}Настройки сохранены в <span class="mono">{settings.file}</span>{/if}
    </span>
    <button class="ghost" disabled={!settings.dirty} onclick={() => settings.reset()}>Отменить</button>
    <button class="btn" disabled={!settings.dirty || hasErrors || settings.saving} onclick={() => settings.save()}>{settings.saving ? "Сохраняю…" : "Сохранить"}</button>
  </div>
{/if}

<style>
  .app { display: grid; grid-template-columns: 300px 1fr; height: 100vh; }
  aside { background: var(--panel); border-right: 1px solid var(--line); display: flex; flex-direction: column; min-height: 0; }
  .brand { padding: 18px 18px 12px; display: flex; align-items: center; gap: 10px; }
  .logo { width: 30px; height: 30px; border-radius: 9px; background: var(--accent); color: var(--on-accent); display: grid; place-items: center; font-weight: 800; font-size: 16px; letter-spacing: -.5px; }
  .brand b { font-size: 15px; }
  .brand small { display: block; color: var(--muted); font-size: 11.5px; margin-top: -2px; }
  .back { margin: 0 14px 12px; display: flex; align-items: center; gap: 8px; padding: 7px 10px; border-radius: 9px; border: 0; background: none; color: var(--muted); font-weight: 600; font-size: 13px; }
  .back:hover { background: var(--panel-2); color: var(--text); }
  .nav { flex: 1; padding: 0 8px; }
  .nav button { width: 100%; display: flex; align-items: center; gap: 10px; border: 0; background: none; text-align: left; padding: 9px 12px; border-radius: 9px; color: var(--muted); font-weight: 600; font-size: 13.5px; margin-bottom: 2px; }
  .nav button:hover { background: var(--panel-2); color: var(--text); }
  .nav button.on { background: var(--accent-soft); color: var(--accent-text); }
  .nav button .badge { margin-left: auto; }
  .file { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  main { overflow-y: auto; min-height: 0; }
  .page { max-width: 760px; margin: 0 auto; padding: 28px 32px 110px; }
  .page h1 { font-size: 22px; margin: 0 0 4px; }
  .lead { color: var(--muted); margin: 0 0 20px; }
  .group { background: var(--panel); border: 1px solid var(--line); border-radius: var(--radius); box-shadow: var(--shadow); margin-bottom: 16px; }
  .ghead { padding: 14px 18px 12px; border-bottom: 1px solid var(--line); display: flex; align-items: center; gap: 12px; }
  .ghead h2 { font-size: 15px; margin: 0; flex: 1; }
  .ghead p { margin: 2px 0 0; color: var(--muted); font-size: 12.5px; font-weight: 400; }
  .grow { flex: 1; }
  .gbody { padding: 14px 18px; }
  .check { display: flex; align-items: flex-start; gap: 12px; cursor: pointer; }
  .check input { margin-top: 3px; width: 16px; height: 16px; accent-color: var(--accent); }
  .check b { display: block; font-size: 13.5px; }
  .check small { display: block; color: var(--muted); font-size: 12px; margin-top: 2px; }
  .gbody.dim { opacity: .45; }
  .field { display: grid; grid-template-columns: 1fr 260px; gap: 16px; align-items: center; padding: 9px 0; }
  .field + .field { border-top: 1px dashed var(--line); }
  .lbl b { display: block; font-weight: 600; font-size: 13.5px; }
  .lbl small { display: block; color: var(--muted); font-size: 12px; }
  .lbl code { font: 11.5px var(--mono); color: var(--faint); }
  .err { display: block; color: var(--bad); font-size: 12px; margin-top: 3px; }
  .pair { display: flex; align-items: center; gap: 6px; color: var(--muted); font-size: 12px; }
  .pair .inp { width: 70px; }
  .seg { display: flex; padding: 3px; background: var(--panel-2); border: 1px solid var(--line); border-radius: 10px; gap: 2px; margin-bottom: 14px; }
  .seg button { flex: 1; border: 0; background: transparent; padding: 7px 8px; border-radius: 8px; color: var(--muted); font-weight: 600; font-size: 13px; }
  .seg button.on { background: var(--panel); color: var(--text); box-shadow: 0 1px 3px rgba(0,0,0,.12); }
  .model { display: flex; align-items: center; gap: 12px; padding: 10px 12px; border: 1px solid var(--line); border-radius: 10px; margin-bottom: 8px; cursor: pointer; transition: border-color .15s, background .15s; }
  .model:hover { border-color: var(--line-2); }
  .model.on { border-color: var(--accent); background: var(--accent-soft); }
  .radio { width: 16px; height: 16px; border-radius: 50%; border: 1.5px solid var(--line-2); flex: none; display: grid; place-items: center; background: var(--panel); }
  .model.on .radio { border-color: var(--accent); }
  .model.on .radio::after { content: ""; width: 8px; height: 8px; border-radius: 50%; background: var(--accent); }
  .mn { flex: 1; min-width: 0; }
  .mn b { display: block; font: 600 13px var(--mono); }
  .mn small { display: block; color: var(--muted); font-size: 12px; }
  .mn .warnline { color: var(--mid); }
  .switch { width: 36px; height: 20px; border-radius: 20px; background: var(--line-2); border: 0; position: relative; padding: 0; flex: none; transition: background .15s; }
  .switch::after { content: ""; position: absolute; left: 2px; top: 2px; width: 16px; height: 16px; border-radius: 50%; background: #fff; transition: transform .15s; box-shadow: 0 1px 2px rgba(0,0,0,.25); }
  .switch.on { background: var(--accent); }
  .switch.on::after { transform: translateX(16px); }
  .client { display: grid; grid-template-columns: 40px 1fr auto; gap: 14px; align-items: center; padding: 14px 18px; }
  .client + .client { border-top: 1px solid var(--line); }
  .cicon { width: 40px; height: 40px; border-radius: 10px; background: var(--panel-2); border: 1px solid var(--line); display: grid; place-items: center; font-weight: 700; color: var(--muted); }
  .cn b { font-size: 14px; }
  .badges { display: inline-flex; gap: 6px; margin-left: 8px; vertical-align: 1px; }
  .cn small { display: block; color: var(--muted); font-size: 12px; margin-top: 2px; word-break: break-all; }
  .acts { display: flex; gap: 8px; }
  .client .note { grid-column: 2 / 4; margin-top: 0; }
  pre.instr { margin: 0; padding: 12px 14px; border-radius: 10px; background: var(--panel-2); border: 1px solid var(--line); font: 12px/1.55 var(--mono); white-space: pre-wrap; color: var(--text); }
  .savebar { position: fixed; left: 300px; right: 0; bottom: 0; background: var(--panel); border-top: 1px solid var(--line); padding: 10px 32px; display: flex; align-items: center; gap: 12px; color: var(--muted); font-size: 12.5px; }
  .themes { display: grid; grid-template-columns: repeat(3, 1fr); gap: 12px; }
  .theme { border: 1.5px solid var(--line); border-radius: 12px; padding: 10px; background: none; text-align: left; font-weight: 600; font-size: 13px; color: var(--text); }
  .theme:hover { border-color: var(--line-2); }
  .theme.on { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .pv { height: 70px; border-radius: 8px; margin-bottom: 8px; display: grid; grid-template-columns: 30% 1fr; overflow: hidden; border: 1px solid rgba(0,0,0,.08); }
  .pv i { display: block; } .pv i:last-child { display: flex; align-items: flex-end; padding: 6px; }
  .pv b { width: 40%; height: 8px; border-radius: 4px; background: #d6336c; }
  .pv.l i:first-child { background: #fff; } .pv.l i:last-child { background: #f5f4f6; }
  .pv.d i:first-child { background: #1b181d; } .pv.d i:last-child { background: #131115; } .pv.d b { background: #f0568c; }
  .pv.s { background: linear-gradient(135deg, #f5f4f6 50%, #131115 50%); display: flex; align-items: flex-end; padding: 6px; }
  .sysbar { display: block; width: 40%; height: 8px; border-radius: 4px; background: #d6336c; }
</style>
