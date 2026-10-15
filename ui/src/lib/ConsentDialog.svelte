<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "./app.svelte";
  import { call } from "./api";
  import { isRemote, rowTime, totals, totalsText, type ConsentRow } from "./consent";
  import Icon from "./Icon.svelte";
  import { settings } from "./settings.svelte";
  import type { SettingsOutput, StatusOutput, TreeOutput } from "./types";

  // Согласие на переиндексацию после смены embed-модели (Т-25, reindex-consent.md). Состояние хранит воркер:
  // закрытие диалога согласия не снимает, проекты остаются в `reindex_required`.
  let rows = $state<ConsentRow[]>([]);
  let selected = $state<ReadonlySet<string>>(new Set());
  let loading = $state(true);
  let sending = $state(false);
  let remoteHost = $state("");

  const summary = $derived(totals(rows, selected));
  const indexModel = $derived(rows[0]?.indexModel ?? null);
  const settingsModel = $derived(rows[0]?.settingsModel ?? "");
  const remote = $derived(isRemote(settingsModel));
  const modelName = (fingerprint: string | null) => fingerprint?.split(":").slice(1).join(":") ?? "—";
  const sourceName = (fingerprint: string | null) => {
    switch (fingerprint?.split(":")[0]) {
      case "builtin": return "встроенная";
      case "remote": return "удалённый провайдер";
      case "local-openai": return "локальный адрес";
      default: return "—";
    }
  };

  onMount(async () => {
    const loaded: ConsentRow[] = [];
    for (const project of app.needReindex) {
      try {
        const [status, tree] = await Promise.all([
          call<StatusOutput>("status", { cwd: project.path, project: project.path }),
          call<TreeOutput>("tree", { cwd: project.path, project: project.path }),
        ]);
        loaded.push({
          path: project.path,
          name: project.name,
          documents: tree.documents.length,
          chunks: tree.documents.reduce((sum, document) => sum + document.chunks, 0),
          indexModel: status.model.index,
          settingsModel: status.model.settings,
        });
      } catch {
        // Проект пропал между проверками — в таблицу не попадает.
      }
    }
    rows = loaded;
    selected = new Set(loaded.map((row) => row.path));
    try {
      const current = settings.saved ?? (await call<SettingsOutput>("settings_get")).settings;
      remoteHost = hostOf(current.embed.base_url ?? "");
    } catch {
      remoteHost = "";
    }
    loading = false;
  });

  function hostOf(url: string): string {
    try {
      return new URL(url).host;
    } catch {
      return url;
    }
  }

  function flip(path: string, on: boolean) {
    const next = new Set(selected);
    if (on) next.add(path);
    else next.delete(path);
    selected = next;
  }

  /** Согласие даётся по проектам: каждый отмеченный получает `reindex` с `accept_model_change` (OQ-56). */
  async function go() {
    sending = true;
    try {
      for (const row of rows.filter((r) => selected.has(r.path))) {
        await call("reindex", { cwd: row.path, project: row.path, full: true, accept_model_change: true });
      }
      app.consentOpen = false;
      const first = rows.find((r) => selected.has(r.path));
      await app.refresh(true);
      if (first) {
        await app.select(first.path);
        app.openIndexing();
      }
    } catch (error) {
      app.flash(error instanceof Error ? error.message : String(error), true);
    } finally {
      sending = false;
    }
  }

  function later() {
    app.consentOpen = false;
    app.view = "main";
  }
</script>

<div class="overlay">
  <div class="dialog" role="dialog" aria-modal="true" aria-label="Согласие на переиндексацию">
    <div class="dh">
      <div class="art"><Icon name="warn" size={18} width={2} /></div>
      <div>
        <h2>Сменилась embed-модель — нужна переиндексация</h2>
        <p>Векторы старой модели нельзя сравнивать с векторами новой, поэтому индекс проектов придётся построить заново.</p>
      </div>
    </div>
    <div class="db">
      <div class="swap">
        <div class="m"><small>Индекс построен</small><b>{modelName(indexModel)}</b><small>{sourceName(indexModel)}</small></div>
        <Icon name="arrow-right" size={18} width={2} />
        <div class="m new">
          <small>В настройках сейчас</small>
          <b>{modelName(settingsModel)}{remote && remoteHost ? ` · ${remoteHost}` : ""}</b>
          <small>{sourceName(settingsModel)}</small>
        </div>
      </div>

      {#if loading}
        <p class="muted">Читаю проекты…</p>
      {:else if rows.length === 0}
        <p class="muted">Проектов, которым нужна переиндексация, нет: возможно, прежняя модель уже возвращена.</p>
      {:else}
        <table class="tbl">
          <thead><tr><th></th><th>Проект</th><th class="n">Документов</th><th class="n">Чанков</th><th class="n">Время</th></tr></thead>
          <tbody>
            {#each rows as row (row.path)}
              <tr>
                <td><input type="checkbox" checked={selected.has(row.path)} aria-label={`Переиндексировать ${row.name}`} onchange={(event) => flip(row.path, event.currentTarget.checked)} /></td>
                <td><b>{row.name}</b></td>
                <td class="n">{row.documents.toLocaleString("ru-RU")}</td>
                <td class="n">{row.chunks.toLocaleString("ru-RU")}</td>
                <td class="n">{rowTime(row)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
        <p class="total muted">{totalsText(summary)}</p>
      {/if}

      <div class="note info">
        <Icon name="info" size={16} />
        <div>
          <b>Что произойдёт.</b> Документы проекта эмбеддятся заново новой моделью в отдельный файл; старый индекс заменяется в самом конце. Пока идёт переиндексация, поиск по проекту не работает. Временно понадобится место под второй индекс. Переиндексацию можно остановить — тогда старый индекс останется, а проект так и будет ждать согласия.
        </div>
      </div>
      {#if remote}
        <div class="note bad">
          <Icon name="error" size={16} />
          <div><b>Текст документов уйдёт внешнему провайдеру.</b> Все чанки отправятся на <span class="mono">{remoteHost || "адрес из настроек"}</span>; время и стоимость определяет провайдер. Ключ берётся из хранилища ОС.</div>
        </div>
      {:else}
        <div class="note info">
          <Icon name="info" size={16} />
          <div>Время — оценка по скорости модели; она уточняется по ходу индексации (bge-m3 считает около 0,5 с на чанк). Не скачанная модель скачается перед индексацией: это входит в её ход.</div>
        </div>
      {/if}
    </div>
    <div class="df">
      <button class="ghost" title="Настройки сохранены, проекты ждут согласия" onclick={later}>Позже</button>
      <button class="link" title="Вернуть в настройках прежнюю модель — индексы снова подходят, переиндексация не нужна" onclick={() => settings.revertModel(indexModel)}>Вернуть прежнюю модель</button>
      <span class="sp"></span>
      <button class="btn" disabled={summary.projects === 0 || sending} onclick={go}>
        {summary.projects ? `Переиндексировать (${summary.projects})` : "Переиндексировать"}
      </button>
    </div>
  </div>
</div>

<style>
  .total { font-size: 12.5px; margin: 8px 2px 0; }
</style>
