<script lang="ts">
  import { app } from "./app.svelte";
  import Banner from "./Banner.svelte";
  import { decimal, documents, moduleLabel, plural, singleLine } from "./format";
  import Icon from "./Icon.svelte";
  import ResultCard from "./ResultCard.svelte";
  import SearchError from "./SearchError.svelte";
  import TextResults from './TextResults.svelte';
  import GraphSearchBundle from './GraphSearchBundle.svelte';
  import { touchedModules } from "./tree";

  const project = $derived(app.project);
  const blocked = $derived(project?.state === "reindex_required");
  const touched = $derived(touchedModules(app.tree, app.selected));
  const running = $derived(app.activity?.running);
  const updating = $derived(
    !!project && project.state !== "reindex_required" && app.waitingOf(project.path) > 0 && app.documentsOf(project) > 0,
  );
  const percent = $derived(project ? app.progressOf(project.path) : null);
  const results = $derived(app.results);

  function scopeLabel(): string {
    if (!touched.length) return "";
    return touched.map((module) => moduleLabel(module.path)).join(", ");
  }

  function step(delta: number) {
    app.count = Math.min(20, Math.max(1, app.shownCount + delta));
  }

  function onKey(event: KeyboardEvent) {
    if (event.key !== "Enter" || event.isComposing || event.keyCode === 229) return;
    event.preventDefault();
    if (!event.isComposing && app.query.trim()) void app.search();
  }

  function onInput(event: Event) {
    const field = event.currentTarget as HTMLTextAreaElement;
    const clean = singleLine(field.value);
    if (clean !== field.value) {
      field.value = clean;
      app.query = clean;
    }
  }

  const resultsWord = (n: number) => plural(n, ["результат", "результата", "результатов"]);
</script>

<main>
  <div class="top">
    <div class="ask">
      <textarea
        aria-label="Поисковый запрос"
        rows="1"
        wrap="off"
        placeholder="Что найти в документации — например, «как считается порог релевантности»"
        bind:value={app.query}
        onkeydown={onKey}
        oninput={onInput}
      ></textarea>
      <div class="askbar">
        <div class="scope">
          {#if blocked}
            По смыслу: нужна переиндексация
          {:else if touched.length}
            Искать в: <b>{scopeLabel()}</b>
          {:else}
            Искать во <b>всём проекте</b>
          {/if}
        </div>
        <div class="search-options">
        <label class="retrieval-mode">
          <span>Поиск</span>
          <span class="mode-control"><select aria-label="Режим поиска" disabled={blocked || app.searching} bind:value={app.retrievalMode}><option value="hybrid">Обычный</option><option value="mix">Со связями</option><option value="graph">Отношения и пути</option></select><Icon name="chevron-down" size={12} /></span>
        </label>
        <div class="count" title="Сколько результатов вернуть. По умолчанию — из настроек">
          Результатов
          <span class="stepper">
            <button onclick={() => step(-1)} aria-label="Меньше">−</button><b>{app.shownCount}</b><button onclick={() => step(1)} aria-label="Больше">+</button>
          </span>
          {#if app.count !== null}
            <button class="link reset" onclick={() => (app.count = null)}>по умолчанию</button>
          {/if}
        </div>
        </div>
        {#if !app.viewer}
          <span class="sep"></span>
          <span class="kbdhint"><kbd>Enter</kbd></span>
        {/if}
        <button class="btn go" disabled={!app.query.trim()} onclick={() => app.search()}>
          Найти <Icon name="arrow-right" size={15} width={2.4} />
        </button>
      </div>
      {#if app.retrievalMode === 'mix'}<label class="related-option"><input type="checkbox" bind:checked={app.includeRelated} />Добавлять связанные источники</label>{/if}
      <div class="text-options">
        <button class="ghost sm" aria-label="Учитывать регистр" aria-pressed={app.textSearch.caseSensitive} onclick={() => app.textSearch.caseSensitive = !app.textSearch.caseSensitive}>Aa</button>
        <button class="ghost sm" aria-label="Регулярное выражение" aria-pressed={app.textSearch.regex} onclick={() => app.textSearch.regex = !app.textSearch.regex}>.*</button>
        <input aria-label="Фильтр пути текстового поиска" placeholder="Путь: features/ или features/*.md" bind:value={app.textSearch.pathFilter} onkeydown={onKey} />
      </div>
    </div>
  </div>

  <div class="results">
    <div class="wrap">
      <TextResults />
      {#if app.searchedQuery}<h2 class="semantic-title">По смыслу</h2>{/if}
      {#if app.searching}<p role="status">Ищем…</p>{/if}
      {#if blocked && project}
        <Banner
          kind="warn"
          title="Проект {project.name} нужно переиндексировать"
          text="Индекс построен моделью {app.status?.model.index ?? '—'}, а в настройках — {app.status?.model.settings ?? '—'}. Поиск по смыслу ждёт переиндексации; «В тексте» читает файлы и доступен."
        >
          <button class="btn sm" onclick={() => (app.consentOpen = true)}>Переиндексировать…</button>
        </Banner>
        <p class="muted hint">Вместо переиндексации можно вернуть прежнюю модель в настройках — индекс снова подойдёт.</p>
      {:else}
        {#if updating && project}
          <Banner
            kind="acc"
            title="Идёт обновление индекса · ждут {app.waitingOf(project.path)} {plural(app.waitingOf(project.path), ['документ', 'документа', 'документов'])}"
            text="Поиск работает по тому, что уже проиндексировано; правки последних минут могут в выдаче не появиться."
          >
            {#if percent !== null}<span class="muted mini">{percent} %</span>{/if}
            <button class="ghost sm" onclick={() => app.openIndexing()}>Подробнее</button>
          </Banner>
        {/if}
        {#if app.searchError}
          <SearchError error={app.searchError} />
        {:else if results}
          {#if app.graphSearch}
            <div class="graph-search-info" role="status">Запрошен {app.graphSearch.requested_mode}; применён {app.graphSearch.applied_mode}.{#if app.graphSearch.fallback_reason}<p>Причина: {app.graphSearch.fallback_reason}</p>{/if}</div>
            {#each [app.graphSearch.related,app.graphSearch.graph_results].filter(Boolean) as graph}
              {#if graph}<GraphSearchBundle {graph} related={graph === app.graphSearch.related} />{/if}
            {/each}
          {/if}
          {#if results.rerank_error}
            <Banner
              kind="warn"
              title="Rerank не отработал — выдача без него"
              text="{results.rerank_error}. Оценка теперь по косинусу, порог для неё другой ({decimal(results.applied.min_score)})."
            >
              <button class="ghost sm" onclick={() => app.search()}>Повторить с rerank</button>
            </Banner>
          {/if}
          {#if results.results.length}
            <div class="meta">
              <span><b>{results.results.length}</b> {resultsWord(results.results.length)}</span>
              <span>{results.applied.score_kind === "rerank" ? "Порядок: по оценке rerank" : "Порядок: гибридный"}</span>
              <span>порог <b>{decimal(results.applied.min_score)}</b></span>
              <span>{app.elapsedMs} мс</span>
              {#if app.displayed}
                <span class="missing">
                  {#if app.markedRequest === app.displayed.requestId}
                    <span class="marked" role="status"><Icon name="check" size={12} width={2.4} />Добавлено в «Новые темы»</span>
                  {:else}
                    <button class="link" title="Ответа в этих фрагментах нет: запомнить вопрос в «Новых темах»" onclick={() => app.markMissing()}>Не хватило знаний</button>
                  {/if}
                </span>
              {/if}
            </div>
            <p class="score-help muted">
              {#if results.applied.score_kind === "cosine"}
                Полоски показывают косинусную близость, а не вероятность правильного ответа. Гибридный порядок учитывает также совпадения слов и точные идентификаторы; близость может возрастать вниз по списку.
              {:else}
                Полоски показывают оценки модели rerank, а не вероятность правильного ответа.
              {/if}
            </p>
            {#each results.results as hit, index (hit.path + ":" + hit.first_line)}
              <ResultCard {hit} rank={index + 1} startOpen={index === 0} />
            {/each}
          {:else if app.graphSearch?.applied_mode === 'graph'}
            {#if app.graphSearch.graph_results?.resolution === 'ambiguous'}
              <p>Имя неоднозначно. Выберите объект и источник в графе.</p>
            {:else if !app.graphSearch.graph_results?.items.length}
              <p>Связи в выбранной области и пределах не найдены. Смысловой слой частичный; отсутствие связи не доказано.</p>
            {/if}
          {:else if results.indexing && project && app.documentsOf(project) === 0}
            <div class="empty">
              <div class="art"><Icon name="search" size={30} width={2} /></div>
              <h1>{project.name} индексируется впервые</h1>
              <p>Пока в индексе ничего нет, поиск вернёт пустой результат. Дерево документов уже видно.</p>
              <div class="acts"><button class="ghost" onclick={() => app.openIndexing()}>Ход индексации</button></div>
            </div>
          {:else}
            <div class="empty none">
              <div class="art low"><Icon name="search" size={30} width={2} /></div>
              <h1>Ничего не найдено</h1>
              <p>
                {#if app.emptyInfo?.best != null}
                  Подходящие чанки есть, но их оценка ниже порога — <b>{decimal(results.applied.min_score)}</b>.
                  Лучший кандидат получил {decimal(app.emptyInfo.best)}.
                {:else}
                  Ни один чанк не набрал порог — <b>{decimal(results.applied.min_score)}</b>.
                {/if}
              </p>
              <ul>
                {#if touched.length}
                  <li><b>Снимите галочки с модулей</b> — сейчас поиск идёт только по {touched.length} из {app.tree.length}.</li>
                {/if}
                <li>Переформулируйте запрос: назовите термин, как он записан в документах.</li>
                {#if app.emptyInfo?.lowest != null}
                  <li>Понизьте порог для этого запроса — появятся результаты с меньшей оценкой.</li>
                {/if}
              </ul>
              <div class="acts">
                {#if touched.length}<button class="btn" onclick={() => app.searchWhole()}>Искать по всему проекту</button>{/if}
                {#if app.emptyInfo?.lowest != null}
                  <button class="ghost" onclick={() => app.searchWithThreshold(app.emptyInfo!.lowest!)}>
                    Порог {decimal(app.emptyInfo.lowest)} для этого запроса
                  </button>
                {/if}
              </div>
              <p class="faint applied">
                Применено: {results.applied.results} {resultsWord(results.applied.results)} · порог {decimal(results.applied.min_score)} · {results.applied.score_kind === "rerank" ? "реранк" : "без реранка"} · {app.elapsedMs} мс
              </p>
            </div>
          {/if}
        {:else if !app.searching}
          <div class="start muted">
            {#if project && app.documentsOf(project) > 0}
              В проекте {documents(app.documentsOf(project))}. Введите запрос и нажмите «Найти».
            {/if}
          </div>
        {/if}
      {/if}
    </div>
  </div>
</main>

<style>
  .text-options {display:flex; gap:4px; align-items:center; margin-top:6px;}
  .text-options button[aria-pressed="true"] {background:var(--accent-soft); color:var(--accent-text);}
  .text-options input {min-width:0; flex:1; font-size:12px; padding:3px 6px; border:1px solid var(--line); border-radius:4px; background:var(--panel-2); color:var(--text);}
  .semantic-title {font-size:13px; margin:8px 0;}
  main { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  .top { padding: 8px 12px; width: 100%; max-width: 664px; margin: 0 auto; }
  .ask { max-width: 920px; margin: 0 auto; background: var(--panel); border: 1px solid var(--line); border-radius: 16px; box-shadow: var(--shadow); padding: 12px 12px 10px 16px; transition: border-color .15s, box-shadow .15s; }
  .ask:focus-within { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft), var(--shadow); }
  textarea { width: 100%; border: 0; outline: none; background: transparent; resize: none; overflow: hidden; font-size: 16px; line-height: 1.5; min-height: 26px; display: block; }
  textarea::placeholder { color: var(--faint); }
  .askbar { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; margin-top: 6px; padding: 2px 0; }
  .askbar > :not(.scope) { flex-shrink: 0; }
  .scope { flex: 1 0 80px; color: var(--muted); font-size: 12.5px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .search-options { display: flex; flex-wrap: nowrap; align-items: center; gap: 12px; }
  .retrieval-mode { display: flex; align-items: center; gap: 6px; color: var(--muted); font-size: 12.5px; }
  .mode-control { position: relative; display: flex; align-items: center; }
  .mode-control :global(svg) { position: absolute; right: 8px; pointer-events: none; }
  .mode-control select { appearance: none; background: var(--panel-2); color: var(--text); border: 1px solid var(--line-2); border-radius: 8px; padding: 3px 26px 3px 9px; max-width: 100%; cursor: pointer; }
  .mode-control select:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .related-option { display: flex; align-items: center; gap: 7px; margin-top: 10px; color: var(--muted); font-size: 12.5px; }
  .scope b { color: var(--text); font-weight: 600; }
  .kbdhint { color: var(--faint); font-size: 12px; white-space: nowrap; }
  .go { padding: 9px 18px; border-radius: 10px; }
  .results { flex: 1; overflow-y: auto; padding: 4px 12px 60px; min-height: 0; }
  .wrap { max-width: 640px; margin: 0 auto; }
  .score-help { font-size: 12px; line-height: 1.5; margin: -6px 2px 14px; }
  .meta { color: var(--muted); font-size: 12.5px; margin: 6px 2px 14px; display: flex; flex-wrap: wrap; gap: 4px 14px; }
  .meta b { color: var(--text); font-weight: 600; }
  .missing { margin-left: auto; }
  .marked { display: inline-flex; align-items: center; gap: 5px; color: var(--good); }
  .missing .link { font-size: 12.5px; }
  .count { display: flex; align-items: center; gap: 6px; color: var(--muted); font-size: 12.5px; white-space: nowrap; }
  .stepper { display: flex; align-items: center; border: 1px solid var(--line-2); border-radius: 8px; background: var(--panel-2); }
  .stepper button { border: 0; background: none; width: 24px; height: 26px; color: var(--muted); font-size: 15px; line-height: 1; padding: 0; }
  .stepper button:hover { color: var(--accent-text); }
  .stepper b { min-width: 18px; text-align: center; color: var(--text); font-variant-numeric: tabular-nums; }
  .reset { color: var(--faint); font-size: 11.5px; }
  .sep { width: 1px; height: 20px; background: var(--line); }
  .hint { font-size: 12.5px; }
  .mini { font-size: 12.5px; }
  .start { padding: 40px 0; text-align: center; }
  .empty.none { padding-top: 44px; }
  .applied { margin-top: 14px; font-size: 12.5px; }
  @media (max-width: 760px) {
    .top { padding: 16px 12px 12px; }
    .results { padding: 4px 12px 64px; }
  }
</style>
