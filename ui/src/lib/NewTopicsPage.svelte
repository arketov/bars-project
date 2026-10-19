<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "./app.svelte";
  import Banner from "./Banner.svelte";
  import { decimal, plural } from "./format";
  import Icon from "./Icon.svelte";
  import {
    ago,
    appliedParams,
    checkIsStale,
    dateTime,
    FILTER_LABEL,
    isActive,
    linesLabel,
    observationsLabel,
    outcomeLabel,
    reasonsLabel,
    scopeLabel,
    sourcesLabel,
    STATUS_LABEL,
    summarizeCheck,
    type TopicCheck,
    type TopicFilter,
    type TopicSummary,
  } from "./new-topics";
  import TopicConfirm from "./TopicConfirm.svelte";

  // Экран «Новые темы» (docs/features/new-topics/design.md): список вопросов, для которых не нашлись материалы,
  // и карточка темы с проверкой. Все данные и операции — у воркера; решение о достаточности знаний принимает человек.
  const store = app.topics;
  const project = $derived(app.project);
  const detail = $derived(store.detail);
  const FILTERS: TopicFilter[] = ["active", "materials_found", "resolved"];

  /** Узкое окно: список и карточка сменяют друг друга внутри одного экрана («К списку»). */
  let cardOpen = $state(store.selectedId !== null);
  let confirm = $state<"delete" | "clear" | null>(null);
  let editing = $state(false);
  let draft = $state("");
  let listEl: HTMLDivElement | undefined = $state();

  const activeTopics = $derived(store.topics.filter(isActive));
  const batchRunning = $derived(store.batch !== null);
  const loadingFirst = $derived(store.loading && store.list === null);

  onMount(() => {
    void store.load(store.list !== null);
    if (store.selectedId !== null && !store.detail) void store.loadDetail(store.selectedId);
    // Возврат из документа сохраняет прокрутку списка.
    if (listEl) listEl.scrollTop = store.scrollTop;
  });

  function pick(topic: TopicSummary) {
    cardOpen = true;
    editing = false;
    if (store.selectedId !== topic.id) void store.select(topic.id);
  }

  function startRename() {
    if (!detail) return;
    draft = detail.title;
    editing = true;
  }

  async function saveRename() {
    if (!detail) return;
    const title = draft.trim();
    if (!title || title === detail.title) {
      editing = false;
      return;
    }
    if (await store.rename(detail.id, title)) editing = false;
  }

  function renameKey(event: KeyboardEvent) {
    if (event.key === "Enter" && !event.isComposing) {
      event.preventDefault();
      void saveRename();
    } else if (event.key === "Escape") editing = false;
  }

  async function confirmed() {
    const action = confirm;
    if (action === "delete" && detail) {
      if (await store.remove(detail.id)) cardOpen = false;
    } else if (action === "clear") {
      if (await store.clear()) cardOpen = false;
    }
    confirm = null;
  }

  const topicsWord = (n: number) => plural(n, ["тема", "темы", "тем"]);
  const variantText = (id: number) => detail?.variants_list.find((variant) => variant.id === id)?.text ?? "Формулировка удалена";
  const canExpand = $derived(!!detail?.variants_list.some((variant) => variant.scope !== "project"));
  const latest = $derived(detail?.checks[0] ?? null);
  const previous = $derived(detail?.checks[1] ?? null);
  const decisionNeeded = $derived(detail?.status === "materials_found");

  function emptyTitle(): string {
    if (store.topics.length === 0) return "Новых тем пока нет";
    return "Нет тем по выбранному фильтру";
  }
</script>

{#snippet checkBlock(check: TopicCheck, heading: string, old: boolean)}
  {@const summary = summarizeCheck(check)}
  <section class="check" class:old aria-label={heading}>
    <h3>{heading}</h3>
    <p class="muted small">
      {dateTime(check.at)} · {check.expanded ? "весь проект (явное расширение)" : "исходная область"} · модель {check.model || "—"}
      {#if appliedParams(check)}· {appliedParams(check)}{/if}
    </p>
    {#if detail && checkIsStale(check, detail)}
      <p class="notice warn">Тема изменилась после этой проверки; повторите её, если нужен актуальный итог.</p>
    {/if}
    {#if summary.headline === "incomplete"}
      <Banner kind="warn" title="Проверка неполная" text="Ни положительного, ни отрицательного окончательного вывода сделать нельзя. Тема не закрыта и область не расширялась." />
    {/if}
    {#each check.results as result (result.variant_id)}
      <div class="vresult" class:found={result.outcome === "found"} class:incomplete={result.outcome === "incomplete"}>
        <p class="vtext">{variantText(result.variant_id)}</p>
        <p class="vscope muted small">{scopeLabel(result.scope)} · {outcomeLabel(result)}</p>
        {#each result.hits as hit (hit.path + ":" + hit.first_line)}
          <div class="source">
            <strong>{hit.title}</strong>
            <code>{hit.path}:{linesLabel(hit)}</code>
            {#if hit.heading_chain}<span class="muted small">{hit.heading_chain.split("/").join(" / ")}</span>{/if}
            <span class="muted small">оценка {decimal(hit.score)} · {hit.score_kind}</span>
            <button class="ghost sm" onclick={() => app.openDocument({ path: hit.path, title: hit.title, first_line: hit.first_line, last_line: hit.last_line })}>
              <Icon name="doc" size={13} />Открыть документ
            </button>
          </div>
        {/each}
      </div>
    {/each}
    {#if summary.found > 0 && summary.total > 0}
      <p class="notice">
        Найдено по {summary.found} из {summary.total} {plural(summary.total, ["формулировки", "формулировок", "формулировок"])}. Совпадения сами по себе не закрывают тему: достаточность определяете вы.
      </p>
    {:else if summary.headline === "empty"}
      <p class="notice">Материалы не найдены при этой проверке. Тема остаётся в списке: дополните документацию и проверьте снова.</p>
    {/if}
  </section>
{/snippet}

<section class="viewer" aria-label="Новые темы">
<div class="vhead">
  <div class="vt"><b>Новые темы</b><small>Вопросы, для которых не нашлись материалы · из поиска окна и MCP · видите только вы</small></div>
  <button class="iconbtn" title="Закрыть" aria-label="Закрыть «Новые темы»" onclick={() => app.closeTopics()}><Icon name="close" size={16} /></button>
</div>
<div class="frame">
<div class="topics" class:card-open={cardOpen}>
  <div class="pane list">
    <header>
      <div class="bar">
        {#if store.batch}
          <span class="progress" role="status" aria-live="polite">Проверяется {Math.min(store.batch.done + 1, store.batch.total)} из {store.batch.total}</span>
          <button class="ghost sm" disabled={store.batch.cancelled} onclick={() => store.cancelBatch()}>{store.batch.cancelled ? "Останавливаем…" : "Остановить"}</button>
        {:else}
          <button class="btn sm" disabled={!activeTopics.length || store.busy || !!store.error} onclick={() => store.checkAll()}>Проверить все</button>
        {/if}
      </div>
    </header>

    {#if store.list && !store.list.enabled}
      <Banner kind="warn" title="Сбор новых тем выключен" text="Сохранённые темы доступны, новые не добавляются.">
        <button class="ghost sm" disabled={store.busy} onclick={() => store.setEnabled(true)}>Включить сбор</button>
      </Banner>
    {/if}
    {#if store.list && store.list.losses > 0}
      <Banner kind="warn" title="Часть обращений не записана: {store.list.losses}" text={`С запуска bars сбор потерял записи${store.list.loss_reason ? `: ${store.list.loss_reason}` : ""}. Поиск при этом работал. Тексты вопросов в журнал не попадают.`} />
    {/if}

    <div class="tabs" role="group" aria-label="Статус темы">
      {#each FILTERS as filter (filter)}
        <button aria-pressed={store.filter === filter} onclick={() => (store.filter = filter)}>{FILTER_LABEL[filter]}</button>
      {/each}
    </div>
    <div class="finder">
      <input type="search" aria-label="Найти тему" placeholder="Найти тему" autocomplete="off" bind:value={store.query} />
      <select aria-label="Сортировка" bind:value={store.sort}>
        <option value="recent">По последнему обращению</option>
        <option value="count">По числу обращений</option>
      </select>
    </div>

    <div class="rows" bind:this={listEl} onscroll={(event) => (store.scrollTop = event.currentTarget.scrollTop)}>
      {#if store.error}
        <div class="empty-block" role="alert">
          <strong>Не удалось прочитать список</strong>
          <p class="muted">{store.error}</p>
          <button class="ghost sm" onclick={() => store.load()}>Повторить</button>
        </div>
      {:else if loadingFirst}
        <div class="skeleton" role="status" aria-label="Загрузка списка"></div>
        <div class="skeleton"></div>
        <div class="skeleton"></div>
      {:else if store.visible.length === 0}
        <div class="empty-block">
          <strong>{emptyTitle()}</strong>
          {#if store.topics.length === 0}
            <p class="muted">Здесь появятся вопросы, для которых не нашлись материалы: пустые поиски окна и MCP собираются сами, а непустую выдачу можно отметить кнопкой «Не хватило знаний». Содержимое внешних чатов не читается.</p>
          {/if}
        </div>
      {:else}
        {#each store.visible as topic (topic.id)}
          <button class="topic" aria-pressed={store.selectedId === topic.id} onclick={() => pick(topic)}>
            <span class="ttitle">{topic.title}</span>
            <span class="meta">
              <span class="tag {topic.status}">{store.checking.has(topic.id) ? "Проверяем…" : STATUS_LABEL[topic.status]}</span>
              <span>{observationsLabel(topic.observations)}</span>
              <span>{sourcesLabel(topic.sources)}</span>
              <span>{ago(topic.last_seen)}</span>
            </span>
          </button>
        {/each}
      {/if}
    </div>

    <details class="manage">
      <summary>Управление списком</summary>
      <label class="collect">
        <input type="checkbox" checked={store.list?.enabled ?? false} disabled={!store.list || store.busy} onchange={(event) => store.setEnabled(event.currentTarget.checked)} />
        Собирать новые темы этого проекта
      </label>
      {#if store.list?.store_path}
        <p class="muted small">Хранение: <span class="mono">{store.list.store_path}</span>. Вопросы могут содержать чувствительные сведения; bars не меняет .gitignore.</p>
      {/if}
      <button class="ghost sm danger" disabled={!store.topics.length || store.busy} onclick={() => (confirm = "clear")}>Очистить список…</button>
    </details>
  </div>

  <div class="pane card" aria-label="Карточка темы">
    <button class="back ghost sm" onclick={() => (cardOpen = false)}><Icon name="back" size={13} />К списку</button>
    {#if store.selectedId === null}
      <p class="muted placeholder">Выберите тему в списке.</p>
    {:else if store.detailError}
      <div class="empty-block" role="alert">
        <strong>Не удалось открыть тему</strong>
        <p class="muted">{store.detailError}</p>
        <button class="ghost sm" onclick={() => store.selectedId !== null && store.loadDetail(store.selectedId)}>Повторить</button>
      </div>
    {:else if !detail || detail.id !== store.selectedId}
      <div class="skeleton" role="status" aria-label="Загрузка темы"></div>
    {:else}
      {@const checking = store.checking.has(detail.id)}
      <p class="eyebrow">Вопрос к документации</p>
      {#if editing}
        <div class="rename">
          <input aria-label="Название темы" maxlength="300" bind:value={draft} onkeydown={renameKey} />
          <button class="btn sm" disabled={store.busy || !draft.trim()} onclick={saveRename}>Сохранить</button>
          <button class="ghost sm" onclick={() => (editing = false)}>Отмена</button>
        </div>
      {:else}
        <h2>{detail.title} <button class="link small" onclick={startRename}>Переименовать</button></h2>
      {/if}
      <p class="status-line">
        <span class="tag {detail.status}">{checking ? "Проверяем…" : STATUS_LABEL[detail.status]}</span>
        <span class="muted small">{observationsLabel(detail.observations)} · {sourcesLabel(detail.sources)}</span>
      </p>
      <p class="reason muted">{reasonsLabel(detail.reasons)}. Первое обращение: {dateTime(detail.first_seen)}, последнее: {dateTime(detail.last_seen)}. Это факт поиска, а не доказательство отсутствия знания.</p>
      {#if detail.status === "resolved" && detail.resolved_at}
        <p class="notice">Закрыта вами {dateTime(detail.resolved_at)}. Новое самостоятельное обращение с тем же вопросом снова откроет тему.</p>
      {/if}

      <div class="actions">
        <button class="btn" disabled={checking} onclick={() => store.check(detail.id)}>Проверить знания</button>
        {#if canExpand}
          <button class="ghost" disabled={checking} title="Явно искать по всему проекту, а не в исходной области" onclick={() => store.check(detail.id, true)}>Проверить по всему проекту</button>
        {/if}
        <button class="ghost danger" disabled={store.busy} onclick={() => (confirm = "delete")}>Удалить…</button>
      </div>

      <h3>Формулировки · {detail.variants_list.length}</h3>
      {#each detail.variants_list as variant (variant.id)}
        <div class="variant"><p>{variant.text}</p><span class="muted small">{scopeLabel(variant.scope)}</span></div>
      {/each}

      {#if checking}
        <section class="check" aria-label="Проверка знаний">
          <h3>Проверка знаний</h3>
          <p class="muted" role="status">Ищем материалы по сохранённым формулировкам…</p>
          <div class="skeleton"></div>
        </section>
      {:else if latest}
        {@render checkBlock(latest, "Проверка знаний", false)}
        {#if decisionNeeded}
          <div class="actions decision">
            <button class="btn" disabled={store.busy} onclick={() => store.setStatus(detail.id, "resolved")}>Знаний достаточно</button>
            <button class="ghost" disabled={store.busy} onclick={() => store.setStatus(detail.id, "open")}>Всё ещё не хватает</button>
          </div>
        {/if}
        {#if previous}
          <details class="previous">
            <summary>Предыдущая проверка · {dateTime(previous.at)}</summary>
            {@render checkBlock(previous, "Прежняя проверка", true)}
          </details>
        {/if}
      {:else}
        <section class="check" aria-label="Проверка знаний">
          <h3>Проверка знаний</h3>
          <p class="muted small">Ещё не проверяли. Добавили документацию — нажмите «Проверить знания».</p>
        </section>
      {/if}
      {#if detail.status === "resolved"}
        <div class="actions decision">
          <button class="ghost" disabled={store.busy} onclick={() => store.setStatus(detail.id, "open")}>Всё ещё не хватает</button>
        </div>
      {/if}
    {/if}
  </div>
</div>
</div>
</section>

{#if confirm}
  <TopicConfirm
    title={confirm === "clear" ? "Очистить список?" : "Удалить тему?"}
    text={confirm === "clear"
      ? `Будут удалены все темы (${store.topics.length} ${topicsWord(store.topics.length)}) проекта ${project?.name ?? ""}. Сбор новых тем продолжится, если он включён.`
      : "Удалить тему и сохранённые варианты? Новый запрос может добавить её снова."}
    confirmLabel={confirm === "clear" ? "Очистить" : "Удалить"}
    busy={store.busy}
    onconfirm={confirmed}
    oncancel={() => (confirm = null)}
  />
{/if}

<style>
  .viewer { display: flex; flex-direction: column; min-width: 0; min-height: 0; background: var(--panel); border-left: 1px solid var(--line); animation: slide .25s both; }
  @keyframes slide { from { opacity: 0; transform: translateX(16px); } to { opacity: 1; transform: none; } }
  .vhead { display: flex; align-items: center; gap: 10px; padding: 14px 16px 12px 22px; border-bottom: 1px solid var(--line); }
  .vt { flex: 1; min-width: 0; }
  .vt b { display: block; font-size: 15px; }
  .vt small { display: block; color: var(--muted); font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .iconbtn { border: 1px solid transparent; background: none; color: var(--muted); padding: 5px; border-radius: 8px; display: grid; place-items: center; }
  .iconbtn:hover { background: var(--panel-2); border-color: var(--line); color: var(--text); }
  .frame { flex: 1; display: grid; min-width: 0; min-height: 0; container-type: inline-size; }
  .topics { display: grid; grid-template-columns: minmax(300px, 400px) minmax(0, 1fr); min-width: 0; min-height: 0; }
  .pane { min-width: 0; min-height: 0; display: flex; flex-direction: column; }
  .list { padding: 16px 16px 12px 22px; border-right: 1px solid var(--line); }
  .card { padding: 20px 24px 60px; overflow-y: auto; }
  h2 { font-size: 19px; line-height: 1.4; margin: 6px 0 4px; overflow-wrap: anywhere; }
  h3 { font-size: 13px; margin: 22px 0 8px; }
  .small { font-size: 12px; }
  .bar { display: flex; align-items: center; gap: 10px; margin-bottom: 12px; min-height: 30px; }
  .progress { font-size: 12.5px; color: var(--accent-text); font-weight: 600; font-variant-numeric: tabular-nums; }
  .tabs { display: flex; gap: 4px; margin: 6px 0 10px; flex-wrap: wrap; }
  .tabs button { font-size: 12px; padding: 4px 9px; border: 1px solid var(--line); border-radius: 8px; background: transparent; color: var(--muted); }
  .tabs button[aria-pressed="true"] { background: var(--accent-soft); color: var(--accent-text); border-color: transparent; font-weight: 600; }
  .finder { display: flex; gap: 8px; margin-bottom: 8px; }
  .finder input, .finder select, .rename input { border: 1px solid var(--line); background: var(--panel-2); border-radius: 9px; padding: 7px 10px; outline: none; min-width: 0; }
  .finder input { flex: 1; }
  .finder select { max-width: 48%; font-size: 12px; }
  .finder input:focus, .finder select:focus, .rename input:focus { border-color: var(--accent); background: var(--panel); }
  .rows { flex: 1; min-height: 0; overflow-y: auto; }
  .topic { display: block; width: 100%; text-align: left; border: 1px solid transparent; background: transparent; padding: 11px 12px; margin: 2px 0; border-radius: 10px; }
  .topic:hover { background: var(--panel-2); }
  .topic[aria-pressed="true"] { border-color: var(--accent); background: var(--accent-soft); }
  .topic:focus-visible, .tabs button:focus-visible, .link:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .ttitle { display: block; font-size: 13.5px; font-weight: 600; margin-bottom: 6px; overflow-wrap: anywhere; }
  .meta { display: flex; flex-wrap: wrap; gap: 4px 10px; align-items: center; color: var(--muted); font-size: 12px; }
  .tag { font-size: 11px; font-weight: 600; border: 1px solid var(--line); padding: 1px 8px; border-radius: 20px; white-space: nowrap; }
  .tag.open { color: var(--mid); border-color: var(--mid); }
  .tag.materials_found { color: var(--good); border-color: var(--good); }
  .tag.resolved { color: var(--muted); }
  .empty-block { padding: 34px 10px; text-align: center; color: var(--muted); }
  .empty-block strong { display: block; color: var(--text); margin-bottom: 8px; }
  .empty-block p { margin: 0 0 12px; font-size: 12.5px; line-height: 1.55; }
  .skeleton { height: 56px; background: var(--panel-2); border-radius: 10px; margin: 10px 0; animation: pulse 1s infinite alternate; }
  @keyframes pulse { to { opacity: .45; } }
  @media (prefers-reduced-motion: reduce) { .skeleton { animation: none; } }
  .manage { border-top: 1px solid var(--line); margin-top: 12px; padding-top: 10px; }
  .manage summary { cursor: pointer; color: var(--muted); font-size: 12px; }
  .collect { display: flex; align-items: center; gap: 8px; margin: 12px 0 6px; font-size: 12.5px; }
  .manage p { margin: 6px 0 10px; line-height: 1.5; overflow-wrap: anywhere; }
  .back { display: none; align-self: flex-start; margin-bottom: 12px; }
  .placeholder { margin-top: 30px; }
  .eyebrow { text-transform: uppercase; letter-spacing: .1em; color: var(--muted); font-size: 10px; margin: 0; }
  .status-line { display: flex; align-items: center; gap: 10px; margin: 4px 0; flex-wrap: wrap; }
  .reason { font-size: 13px; margin: 10px 0 14px; line-height: 1.55; }
  .rename { display: flex; gap: 8px; margin: 6px 0; align-items: center; flex-wrap: wrap; }
  .rename input { flex: 1; min-width: 200px; font-size: 15px; }
  .actions { display: flex; gap: 8px; flex-wrap: wrap; align-items: center; margin-top: 4px; }
  .actions.decision { margin-top: 16px; }
  .variant { padding: 10px 14px; background: var(--panel); border: 1px solid var(--line); border-radius: 10px; margin: 6px 0; }
  .variant p { margin: 0 0 4px; font-size: 13.5px; overflow-wrap: anywhere; }
  .check { border-top: 1px solid var(--line); margin-top: 22px; padding-top: 16px; }
  .check h3 { margin-top: 0; }
  .check.old { border-top: 0; margin-top: 10px; padding-top: 4px; opacity: .85; }
  .vresult { margin: 10px 0; padding: 10px 12px; border: 1px solid var(--line); border-radius: 10px; }
  .vresult.found { border-color: var(--good); }
  .vresult.incomplete { border-color: var(--mid); }
  .vtext { margin: 0 0 2px; font-weight: 600; font-size: 13.5px; overflow-wrap: anywhere; }
  .vscope { margin: 0 0 6px; }
  .source { display: flex; flex-direction: column; align-items: flex-start; gap: 3px; padding: 10px 12px; margin-top: 8px; border-radius: 9px; background: var(--good-soft); }
  .source strong { font-size: 13px; }
  .source code { font: 11.5px var(--mono); color: var(--good); overflow-wrap: anywhere; }
  .source .ghost { margin-top: 6px; }
  .notice { color: var(--muted); font-size: 12.5px; margin: 10px 0 0; line-height: 1.5; }
  .notice.warn { color: var(--mid); }
  .previous { margin-top: 16px; }
  .previous summary { cursor: pointer; color: var(--muted); font-size: 12.5px; }
  .link.small { font-size: 12px; margin-left: 8px; }
  .ghost.sm, .btn.sm { padding: 5px 11px; font-size: 12.5px; }
  @container (max-width: 760px) {
    .topics { grid-template-columns: minmax(0, 1fr); }
    .list { padding: 14px 14px 12px 18px; border-right: 0; }
    .card { padding: 14px 16px 48px 18px; }
    .back { display: inline-flex; }
    .topics.card-open .list, .topics:not(.card-open) .card { display: none; }
  }
</style>
