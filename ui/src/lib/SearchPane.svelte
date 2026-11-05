<script lang="ts">
  import { app } from "./app.svelte";
  import Banner from "./Banner.svelte";
  import { documents, moduleLabel, plural, singleLine } from "./format";
  import Icon from "./Icon.svelte";
  import ResultCard from "./ResultCard.svelte";
  import SearchError from "./SearchError.svelte";
  import { decimal } from "./format";
  import { touchedModules, searchScope } from "./tree";
  import { graphScope } from "./graph-scope.svelte";
  import { dropToken, groupHits, hasOperators, markerFor, parseQuery, sectionsWord, withoutOperators } from "./search-results";

  const project = $derived(app.project);
  const blocked = $derived(project?.state === "reindex_required");
  const touched = $derived(touchedModules(app.tree, app.selected));
  const updating = $derived(
    !!project && project.state !== "reindex_required" && app.waitingOf(project.path) > 0 && app.documentsOf(project) > 0,
  );
  const percent = $derived(project ? app.progressOf(project.path) : null);
  const results = $derived(app.results);
  const cards = $derived(groupHits(results?.results ?? []));
  const sectionCount = $derived(results?.results.length ?? 0);
  const marker = $derived(markerFor(app.searchedQuery, results?.understood));
  const tokens = $derived(parseQuery(app.query));
  const operators = $derived(tokens.filter((t) => t.kind !== "word"));
  let field = $state<HTMLTextAreaElement | null>(null);
  let mirror = $state<HTMLDivElement | null>(null);
  let helpOpen = $state(false);
  /** Область, пришедшая из графа («Искать в подграфе»): над поиском видна, пока галочки дерева не тронуты вручную. */
  const fromGraph = $derived(graphScope.active);
  /** Выдача на экране считана в другой области, чем действует сейчас. */
  const staleScope = $derived.by(() => {
    const shown = app.displayed;
    if (!fromGraph || !shown) return false;
    const now = searchScope(app.tree, app.selected);
    return JSON.stringify({ modules: shown.modules, files: shown.files }) !== JSON.stringify({ modules: now.modules, files: now.files });
  });
  $effect(() => {
    if (graphScope.focusTick) queueMicrotask(() => field?.focus());
  });

  function scopeLabel(): string {
    if (!touched.length) return "";
    return touched.map((module) => moduleLabel(module.path)).join(", ");
  }

  function step(delta: number) {
    app.count = Math.min(app.maxCount, Math.max(1, app.shownCount + delta));
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === "Escape" && helpOpen) { helpOpen = false; return; }
    if (event.key !== "Enter" || event.isComposing || event.keyCode === 229) return;
    event.preventDefault();
    if (!event.isComposing && app.query.trim()) void app.search();
  }

  function onInput(event: Event) {
    const input = event.currentTarget as HTMLTextAreaElement;
    const clean = singleLine(input.value);
    if (clean !== input.value) {
      input.value = clean;
      app.query = clean;
    }
  }

  /** Поле рисует операторы поверх прозрачного текста: фраза, исключение, папка — цветом и жирностью. */
  const painted = $derived.by(() => {
    const parts: { text: string; kind: string }[] = [];
    let pos = 0;
    for (const token of tokens) {
      if (token.start > pos) parts.push({ text: app.query.slice(pos, token.start), kind: "word" });
      parts.push({ text: token.text, kind: token.kind });
      pos = token.end;
    }
    if (pos < app.query.length) parts.push({ text: app.query.slice(pos), kind: "word" });
    return parts;
  });
  const syncScroll = () => { if (mirror && field) mirror.scrollLeft = field.scrollLeft; };
  const CHIP_LABEL: Record<string, string> = { phrase: "фраза", exclude: "исключить", path: "в папке" };

  const parameterSummary = $derived([
    app.count !== null ? app.shownCount + " рез." : "",
    app.showScores ? "баллы" : "",
  ].filter(Boolean).join(" · "));
  const docsWord = (n: number) => plural(n, ["документ", "документа", "документов"]);
  const ranBy = $derived(results ? (results.applied.score_kind === "rerank" ? "оценка модели" : results.applied.score_kind === "cosine" ? "близость по смыслу" : "только слова запроса") : "");
  const emptyHasPhrase = $derived(hasOperators(app.searchedQuery, "phrase"));
  const emptyHasExclude = $derived(hasOperators(app.searchedQuery, "exclude"));
  const emptyHasPath = $derived(hasOperators(app.searchedQuery, "path"));
  const emptyScope = $derived(touched.length ? `в области «${scopeLabel()}»` : emptyHasPath ? "в указанной папке" : "во всём проекте");

  function wholeProject() {
    if (emptyHasPath) app.query = withoutOperators(app.searchedQuery, ["path"]);
    void app.searchWhole();
  }
</script>

<svelte:window onclick={(event) => { if (helpOpen && !(event.target as Element).closest?.(".help")) helpOpen = false; }} />

<main>
  <div class="top">
    {#if fromGraph}
      <div class="graph-scope" role="status" aria-label="Область из графа">
        <span>Область из графа: <b>{fromGraph.documents} {plural(fromGraph.documents, ["документ", "документа", "документов"])}</b>{#if fromGraph.skipped} · {fromGraph.skipped} исключено: удалены или вне проекта{/if}</span>
        <button class="link" onclick={() => graphScope.restore()}>Вернуть прежнюю область</button>
      </div>
    {/if}
    <div class="ask">
      <div class="qrow">
        <div class="qfield" class:empty-q={!app.query}>
          <div class="mirror" bind:this={mirror} aria-hidden="true">{#each painted as part, i (i)}<span class={"tok-" + part.kind}>{part.text}</span>{/each}</div>
          <textarea
            bind:this={field}
            aria-label="Поисковый запрос"
            rows="1"
            wrap="off"
            spellcheck="false"
            placeholder={'Спросите словами или уточните: "точная фраза", -слово, папка:инструкции'}
            bind:value={app.query}
            onkeydown={onKey}
            oninput={onInput}
            onscroll={syncScroll}
          ></textarea>
        </div>
        <span class="help">
          <button class="ghost sm iconbtn" aria-label="Как уточнить запрос" title="Как уточнить запрос" aria-expanded={helpOpen} onclick={() => (helpOpen = !helpOpen)}>?</button>
          {#if helpOpen}
            <div class="pop" role="dialog" aria-label="Как уточнить запрос">
              <b>Уточнить запрос</b>
              <table><tbody>
                <tr><td><code>"возврат средств"</code></td><td>слова именно в таком порядке; в выдаче только разделы с этой фразой</td></tr>
                <tr><td><code>-скидка</code></td><td>исключить разделы со словом (или <code>-"фраза"</code>)</td></tr>
                <tr><td><code>папка:инструкции</code></td><td>искать только в папке; то же <code>path:</code>. С пробелом — в кавычках</td></tr>
              </tbody></table>
              <p>Всё остальное — обычные слова: можно писать вопрос как есть. Дефис в слове («по-русски», «274-ФЗ») и «-5» операторами не считаются. <code>папка:</code> сужает то, что отмечено в дереве, а не расширяет.</p>
            </div>
          {/if}
        </span>
        <button class="btn go" disabled={!app.query.trim()} onclick={() => app.search()}>
          Найти <Icon name="arrow-right" size={15} width={2.4} />
        </button>
      </div>
      <div class="qmeta">
        {#if operators.length}
          <span>Понято как:</span>
          {#each operators as token (token.start)}
            <span class="chip {token.kind}">{CHIP_LABEL[token.kind]}: {token.value} <button aria-label="Убрать из запроса" title="Убрать из запроса" onclick={() => void app.searchAs(dropToken(app.query, token))}>×</button></span>
          {/each}
          <span class="sp"></span><span>Остальное — слова вопроса</span>
        {:else}
          <span class="scope">
            {#if fromGraph}Искать в: <b>подграфе из графа</b>
            {:else if touched.length}Искать в: <b>{scopeLabel()}</b>
            {:else}Искать во <b>всём проекте</b>{/if}
          </span>
          <span class="sp"></span>
          <span class="muted">Подсказка: <span class="mono">"фраза"</span> · <span class="mono">-слово</span> · <span class="mono">папка:имя</span></span>
        {/if}
      </div>
      <details class="query-parameters">
        <summary>Параметры{#if parameterSummary}{' · '}{parameterSummary}{/if}</summary>
        <div class="search-options">
          <div class="count" title="Сколько результатов вернуть. По умолчанию — из настроек">
            Результатов
            <span class="stepper">
              <button onclick={() => step(-1)} aria-label="Меньше">−</button><b>{app.shownCount}</b><button onclick={() => step(1)} aria-label="Больше">+</button>
            </span>
            {#if app.count !== null}
              <button class="link reset" onclick={() => (app.count = null)}>по умолчанию</button>
            {/if}
          </div>
          <label class="toggle"><input type="checkbox" checked={app.showScores} onchange={(event) => app.setShowScores(event.currentTarget.checked)} /> показать баллы</label>
        </div>
      </details>
    </div>
  </div>

  <div class="results" bind:this={graphScope.scrollHost}>
    <div class="wrap">
      {#if staleScope}<p class="muted hint" role="status">Выдача ниже относится к прежней области. Нажмите «Найти», чтобы искать в области из графа.</p>{/if}
      {#if app.searchUnsaved.length}
        <div role="alert" class="save-warning">
          {#each app.searchUnsaved as item}<p>Не записано: <button class="link" onclick={() => app.openSearchUnsaved(item.path)}>{item.path}</button> — {item.reason}{item.reason.endsWith(".") ? "" : "."} Результаты по записанной версии.</p>{/each}
        </div>
      {/if}
      {#if blocked && project}
        <Banner
          kind="warn"
          title="Проект {project.name} нужно переиндексировать"
          text="Индекс построен моделью {app.status?.model.index ?? '—'}, а в настройках — {app.status?.model.settings ?? '—'}. Поиск ждёт переиндексации."
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
        {#if app.searching}
          {#if app.waitingModel}
            <Banner
              kind="acc"
              title="Загружается модель поиска"
              text="Поиск подождёт её — обычно несколько секунд, самое долгое — 20. Если не дождётся, найдёт по словам и скажет об этом."
            >
              <button class="ghost sm" onclick={() => app.searchWords()}>Искать по словам сейчас</button>
            </Banner>
            <div class="skeleton"></div><div class="skeleton"></div><div class="skeleton"></div>
          {:else}
            <p role="status" class="muted">Ищем…</p>
          {/if}
        {:else if app.searchError}
          <SearchError error={app.searchError} />
        {:else if results}
          {#if results.semantic_error}
            <Banner
              kind="warn"
              title="Результаты только по словам запроса"
              text="Модель поиска не успела загрузиться, поэтому смысл вопроса не учтён: порядок мог бы быть точнее. Операторы работают."
            >
              <button class="ghost sm" onclick={() => app.search()}>Повторить с моделью</button>
            </Banner>
          {/if}
          {#if results.rerank_error}
            <Banner
              kind="warn"
              title="Оценка моделью не выполнена — порядок по близости"
              text="{results.rerank_error}."
            >
              <button class="ghost sm" onclick={() => app.search()}>Повторить оценку моделью</button>
            </Banner>
          {/if}
          {#if cards.length}
            <div class="summary">
              <b>{cards.length} {docsWord(cards.length)}, {sectionCount} {sectionsWord(sectionCount)}</b>
              {#if app.showScores}
                <span class="tech">{results.applied.score_kind === "none" ? "без опоры · порога нет" : `порог ${decimal(results.applied.min_score)} · опора: ${ranBy}`} · {app.elapsedMs} мс</span>
              {/if}
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
            {#each cards as card (card.path)}
              <ResultCard {card} {marker} />
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
              <h1>Ничего не нашлось</h1>
              <p>По запросу <b>«{app.searchedQuery}»</b> {emptyScope} совпадений нет.</p>
              <ul>
                {#if emptyHasPhrase}<li>Уберите кавычки: без них слова ищутся не обязательно подряд.</li>{/if}
                {#if emptyHasExclude}<li>Уберите исключение: оно могло вычеркнуть нужный раздел.</li>{/if}
                {#if touched.length || emptyHasPath}<li>Расширьте область: сейчас поиск идёт не по всему проекту.</li>{/if}
                <li>Переформулируйте запрос: назовите термин, как он записан в документах.</li>
              </ul>
              <div class="acts">
                {#if emptyHasPhrase}<button class="ghost" onclick={() => app.searchAs(withoutOperators(app.searchedQuery, ["phrase"]))}>Без кавычек</button>{/if}
                {#if emptyHasExclude}<button class="ghost" onclick={() => app.searchAs(withoutOperators(app.searchedQuery, ["exclude"]))}>Без исключения</button>{/if}
                {#if touched.length || emptyHasPath}<button class="ghost" onclick={wholeProject}>Весь проект</button>{/if}
              </div>
              {#if app.showScores}
                <p class="faint applied">Применено: порог {decimal(results.applied.min_score)} · {ranBy} · {app.elapsedMs} мс</p>
              {/if}
            </div>
          {/if}
        {:else}
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
  .save-warning { border: 1px solid var(--mid); border-radius: 6px; padding: 8px 10px; margin-bottom: 10px; font-size: 12px; overflow-wrap: anywhere; }
  .save-warning p { margin: 0; }
  main { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  .top { padding: 8px 12px; width: 100%; max-width: 664px; margin: 0 auto; }
  .ask { max-width: 920px; margin: 0 auto; background: var(--panel); border: 1px solid var(--line); border-radius: 16px; box-shadow: var(--shadow); padding: 12px 12px 10px 16px; transition: border-color .15s, box-shadow .15s; }
  .ask:focus-within { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft), var(--shadow); }
  .qrow { display: flex; gap: 10px; align-items: center; }
  .qfield { position: relative; flex: 1; min-width: 0; height: 26px; }
  .mirror, textarea { position: absolute; inset: 0; width: 100%; height: 100%; margin: 0; padding: 0; border: 0; font: 16px/26px var(--font, inherit); white-space: pre; overflow: hidden; letter-spacing: normal; }
  .mirror { color: var(--text); pointer-events: none; }
  textarea { background: transparent; outline: none; resize: none; color: transparent; caret-color: var(--text); display: block; }
  textarea::placeholder { color: var(--faint); }
  .qfield.empty-q .mirror { display: none; }
  .tok-exclude { color: var(--bad); font-weight: 600; }
  .tok-phrase { color: var(--accent-text); font-weight: 600; }
  .tok-path { color: var(--good); font-weight: 600; }
  .qmeta { display: flex; flex-wrap: wrap; gap: 6px 8px; align-items: center; margin-top: 8px; padding-top: 8px; border-top: 1px dashed var(--line); font-size: 12.5px; color: var(--muted); }
  .sp { flex: 1; }
  .scope { min-width: 0; overflow-wrap: anywhere; }
  .scope b { color: var(--text); font-weight: 600; }
  .chip { display: inline-flex; align-items: center; gap: 6px; padding: 2px 9px; border-radius: 20px; border: 1px solid var(--line); background: var(--panel-2); font-size: 12px; font-weight: 600; color: var(--muted); }
  .chip.exclude { background: var(--bad-soft); color: var(--bad); border-color: transparent; }
  .chip.phrase { background: var(--accent-soft); color: var(--accent-text); border-color: transparent; }
  .chip.path { background: var(--good-soft); color: var(--good); border-color: transparent; }
  .chip button { border: 0; background: none; padding: 0; color: inherit; line-height: 1; font-size: 14px; }
  .help { position: relative; }
  .iconbtn { width: 28px; height: 28px; border-radius: 50%; padding: 0; font-weight: 700; }
  .pop { position: absolute; right: 0; top: calc(100% + 8px); width: 420px; max-width: calc(100vw - 40px); background: var(--panel); border: 1px solid var(--line-2); border-radius: 12px; box-shadow: 0 10px 30px rgba(0,0,0,.2); padding: 12px 14px; z-index: 30; font-size: 13px; color: var(--text); text-align: left; }
  .pop table { width: 100%; border-collapse: collapse; }
  .pop td { padding: 5px 6px; border-bottom: 1px dashed var(--line); vertical-align: top; }
  .pop tr:last-child td { border: 0; }
  .pop code { font: 12px var(--mono); background: var(--panel-2); border: 1px solid var(--line); border-radius: 5px; padding: 1px 5px; white-space: nowrap; }
  .pop p { margin: 8px 0 0; color: var(--muted); font-size: 12px; }
  .go { padding: 9px 18px; border-radius: 10px; flex-shrink: 0; }
  .search-options { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; margin-top: 10px; }
  .toggle { display: inline-flex; align-items: center; gap: 7px; font-size: 12.5px; color: var(--muted); cursor: pointer; user-select: none; }
  .graph-scope { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 12px; margin: 0 0 6px; padding: 5px 10px; border: 1px solid var(--line-2); border-radius: 8px; background: var(--accent-soft); font-size: 12px; }
  .graph-scope .link { margin-left: auto; }
  .results { flex: 1; overflow-y: auto; padding: 4px 12px 60px; min-height: 0; }
  .wrap { max-width: 640px; margin: 0 auto; }
  .summary { display: flex; flex-wrap: wrap; align-items: baseline; gap: 4px 10px; margin: 16px 2px 8px; color: var(--muted); font-size: 12.5px; }
  .summary b { color: var(--text); font-size: 13px; }
  .tech { font-family: var(--mono); font-size: 11.5px; color: var(--faint); }
  .missing { margin-left: auto; }
  .marked { display: inline-flex; align-items: center; gap: 5px; color: var(--good); }
  .missing .link { font-size: 12.5px; }
  .count { display: flex; align-items: center; gap: 6px; color: var(--muted); font-size: 12.5px; white-space: nowrap; }
  .stepper { display: flex; align-items: center; border: 1px solid var(--line-2); border-radius: 8px; background: var(--panel-2); }
  .stepper button { border: 0; background: none; width: 24px; height: 26px; color: var(--muted); font-size: 15px; line-height: 1; padding: 0; }
  .stepper button:hover { color: var(--accent-text); }
  .stepper b { min-width: 18px; text-align: center; color: var(--text); font-variant-numeric: tabular-nums; }
  .reset { color: var(--faint); font-size: 11.5px; }
  .hint { font-size: 12.5px; }
  .mini { font-size: 12.5px; }
  .start { padding: 40px 0; text-align: center; }
  .empty.none { padding-top: 44px; }
  .applied { margin-top: 14px; font-size: 12.5px; }
  .skeleton { height: 78px; border-radius: var(--radius); background: linear-gradient(90deg, var(--panel) 25%, var(--panel-2) 50%, var(--panel) 75%); background-size: 200% 100%; border: 1px solid var(--line); margin-bottom: 10px; animation: shimmer 1.4s linear infinite; }
  @keyframes shimmer { to { background-position: -200% 0; } }
  @media (max-width: 760px) {
    .top { padding: 16px 12px 12px; }
    .results { padding: 4px 12px 64px; }
  }
  .query-parameters { border-top: 1px solid var(--line); margin-top: 8px; padding-top: 7px; font-size: 12px; }
  .query-parameters summary { cursor: pointer; color: var(--muted); overflow-wrap: anywhere; }
  .query-parameters summary:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
</style>
