<script lang="ts">
  import { app } from "./app.svelte";
  import Banner from "./Banner.svelte";
  import { decimal, documents, moduleLabel, plural } from "./format";
  import Icon from "./Icon.svelte";
  import ResultCard from "./ResultCard.svelte";
  import SearchError from "./SearchError.svelte";
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
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      void app.search();
    }
  }

  const resultsWord = (n: number) => plural(n, ["результат", "результата", "результатов"]);
</script>

<main>
  <div class="top">
    <div class="ask" class:dis={blocked}>
      <textarea
        rows="1"
        placeholder="Что найти в документации — например, «как считается порог релевантности»"
        disabled={blocked}
        bind:value={app.query}
        onkeydown={onKey}
      ></textarea>
      <div class="askbar">
        <div class="scope">
          {#if blocked}
            Поиск недоступен: нужна переиндексация
          {:else if touched.length}
            Искать в: <b>{scopeLabel()}</b>
          {:else}
            Искать во <b>всём проекте</b>
          {/if}
        </div>
        <div class="count" title="Сколько результатов вернуть. По умолчанию — из настроек">
          Результатов
          <span class="stepper">
            <button onclick={() => step(-1)} aria-label="Меньше">−</button><b>{app.shownCount}</b><button onclick={() => step(1)} aria-label="Больше">+</button>
          </span>
          {#if app.count !== null}
            <button class="link reset" onclick={() => (app.count = null)}>по умолчанию</button>
          {/if}
        </div>
        {#if !app.viewer}
          <span class="sep"></span>
          <span class="kbdhint"><kbd>Ctrl</kbd> + <kbd>Enter</kbd></span>
        {/if}
        <button class="btn go" disabled={blocked || app.searching || !app.query.trim()} onclick={() => app.search()}>
          Найти <Icon name="arrow-right" size={15} width={2.4} />
        </button>
      </div>
    </div>
  </div>

  <div class="results">
    <div class="wrap">
      {#if blocked && project}
        <Banner
          kind="warn"
          title="Проект {project.name} нужно переиндексировать"
          text="Индекс построен моделью {app.status?.model.index ?? '—'}, а в настройках — {app.status?.model.settings ?? '—'}. Поиск по проекту не работает, пока вы не согласитесь на переиндексацию. Агенты получают ошибку «нужна переиндексация» и не могут запустить её сами."
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
              <span>{results.applied.score_kind === "rerank" ? "реранк" : "без реранка"}</span>
              <span>порог <b>{decimal(results.applied.min_score)}</b></span>
              <span>{app.elapsedMs} мс</span>
            </div>
            {#each results.results as hit, index (hit.path + ":" + hit.first_line)}
              <ResultCard {hit} rank={index + 1} startOpen={index === 0} />
            {/each}
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
                  <li>Понизьте порог для этого запроса — слабые результаты появятся с пометкой «слабо».</li>
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
  main { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  .top { padding: 22px 32px 14px; }
  .ask { max-width: 920px; margin: 0 auto; background: var(--panel); border: 1px solid var(--line); border-radius: 16px; box-shadow: var(--shadow); padding: 12px 12px 10px 16px; transition: border-color .15s, box-shadow .15s; }
  .ask:focus-within { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft), var(--shadow); }
  .ask.dis { opacity: .6; }
  textarea { width: 100%; border: 0; outline: none; background: transparent; resize: none; font-size: 16px; line-height: 1.5; min-height: 26px; display: block; }
  textarea::placeholder { color: var(--faint); }
  .askbar { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 10px; margin-top: 6px; }
  .scope { flex: 1 1 180px; color: var(--muted); font-size: 12.5px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .scope b { color: var(--text); font-weight: 600; }
  .kbdhint { color: var(--faint); font-size: 12px; white-space: nowrap; }
  .go { padding: 9px 18px; border-radius: 10px; }
  .results { flex: 1; overflow-y: auto; padding: 4px 32px 80px; min-height: 0; }
  .wrap { max-width: 920px; margin: 0 auto; }
  .meta { color: var(--muted); font-size: 12.5px; margin: 6px 2px 14px; display: flex; flex-wrap: wrap; gap: 4px 14px; }
  .meta b { color: var(--text); font-weight: 600; }
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
</style>
