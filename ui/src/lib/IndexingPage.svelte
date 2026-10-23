<script lang="ts">
  import { app } from "./app.svelte";
  import { commandAdapter } from './command-adapters';
  let root: HTMLElement;
  import Banner from "./Banner.svelte";
  import { decimal, documents, duration, jobReason, percentOf, plural, remainingSeconds } from "./format";
  import Icon from "./Icon.svelte";
  import { operationText } from "./notifications";
  import type { DocumentSource, DocumentState } from "./types";

  const project = $derived(app.project);
  /** Очередь документов проекта (document-queue.md): приходит в ответе `activity`, который окно опрашивает само. */
  const docQueue = $derived(app.activity?.documents?.find((entry) => entry.project === project?.path) ?? null);
  const DOC_STATE: Record<DocumentState, string> = { waiting: "ждёт паузы", queued: "в очереди", indexing: "идёт embed", error: "ошибка" };
  const DOC_SOURCE: Record<DocumentSource, string> = { explicit: "по команде", edit: "правка в bars", watch: "правка на диске", periodic: "проверка" };

  const waitingDocs = $derived(docQueue?.waiting ?? 0);
  const activity = $derived(app.activity);
  const running = $derived(activity?.running && activity.running.project === project?.path ? activity.running : null);
  const queued = $derived(activity?.queue.find((job) => job.project === project?.path) ?? null);
  const problems = $derived(activity?.problems.find((entry) => entry.project === project?.path) ?? null);
  const paused = $derived(activity?.paused ?? false);
  const rebuilding = $derived(running?.kind === "rebuild" || queued?.kind === "rebuild");
  const percent = $derived(running ? percentOf(running.docs_done, running.docs_total) : 0);
  const left = $derived(running ? remainingSeconds(running.docs_done, running.docs_total, running.elapsed_seconds) : null);
  const rows = $derived([
    ...(activity?.running ? [{ ...activity.running, running: true, files: 0 }] : []),
    ...(activity?.queue ?? []).map((job) => ({ ...job, running: false })),
  ]);
  const files = $derived(problems?.files ?? []);
  const last = $derived(project ? app.latestIndexingResult(project.path) : null);
  const resultState: Record<string, string> = { success: "Готово", partial: "Частичный итог", error: "Ошибка", cancelled: "Отменено" };

  /** Что сделать с ошибкой файла: подсказки из indexing-progress.md. */
  function advice(reason: string): string {
    const text = reason.toLowerCase();
    if (text.includes("utf")) return "Файл не в кодировке UTF-8: пересохраните его в UTF-8 — bars индексирует только такие файлы.";
    if (text.includes("доступ") || text.includes("denied") || text.includes("прав")) {
      return "Закройте программу, которая держит файл, или проверьте права. Повтор — автоматически при следующей проверке.";
    }
    if (text.includes("длин") || text.includes("too long")) {
      return "Уменьшите «Максимальный размер чанка» в настройках на вкладке «Нарезка» или выберите другую модель.";
    }
    return "Повторите: bars попробует снова при следующей проверке.";
  }
</script>

<main bind:this={root} use:commandAdapter={{ 'indexing.details': { run: () => { const heading=root.querySelector<HTMLElement>('.panel h2'); if(heading) {heading.tabIndex=-1;heading.focus();heading.scrollIntoView?.({block:'nearest'});} }, reason: () => project ? undefined : 'Проект не выбран' } }}>
  {#if project}
    <div class="page">
      <h1>{project.name}</h1>
      {#if app.workerDown}<Banner kind="warn" title="Нет связи с воркером" text="{app.workerDown}. Сохранён последний снимок хода индексации; данные могут быть устаревшими." />{/if}
      <p class="lead">
        {#if rebuilding}
          Полная переиндексация по вашему согласию: индекс строится заново в отдельном файле.
        {:else if running}
          Идёт проверка проекта: bars сверяет документы с индексом.
        {:else if queued}
          Проект ждёт своей очереди.
        {:else if waitingDocs > 0}
          Правки документов ждут паузы перед индексацией: {waitingDocs}. Поиск по смыслу увидит их, когда пауза кончится.
        {:else if problems?.last_error}
          {problems.last_error}
        {:else if files.length}
          Индекс обновлён, кроме {files.length} {files.length === 1 ? "файла" : "файлов"}: они остались с ошибкой.
        {:else}
          Сейчас нет задания индексации. Итог последнего прохода показан отдельно.
        {/if}
      </p>

      {#if rebuilding}
        <Banner
          kind="warn"
          title="Поиск по {project.name} пока не работает"
          text="Старый индекс заменится новым, когда переиндексация закончится. Поиск по другим проектам работает как обычно."
        />
      {/if}

      {#if running}
        <div class="panel">
          <div class="ch">
            <h2>{paused ? "Приостановлено" : running.docs_total === 0 ? "Обход документов" : "Идёт индексация"}</h2>
            <span class="badge acc">{paused ? `пауза · ${percent} %` : `${percent} %`}</span>
          </div>
          <div class="cb">
            <div class="hrow base">
              <span class="big">{running.docs_done}</span>
              <span class="muted">из {documents(running.docs_total)}</span>
              <span class="sp"></span>
              <span class="muted">{left === null ? "оценка времени — после первых документов" : `осталось около ${duration(left)}`}</span>
            </div>
            <div class="pbar" class:stripe={!paused} class:paused style="margin-top:10px"><i style="width:{percent}%"></i></div>
            <div class="stats">
              <div class="stat"><small>Чанков обработано</small><b>{running.chunks_done}</b></div>
              <div class="stat"><small>Скорость</small><b>{decimal(running.chunks_per_second, 1)} чанка/с</b></div>
              <div class="stat"><small>Устройство embed · текущее задание</small><b>{running.embed_device === "directml" ? "DirectML" : running.embed_device === "cpu" ? "CPU" : "Пока неизвестно"}</b></div>
            </div>
            <dl class="kv">
              <dt>Сейчас</dt><dd class="mono">{running.current_file ?? "—"}</dd>
              {#if running.attempt > 0}<dt>Попытка векторизации</dt><dd>{running.attempt}/4</dd>{/if}
              <dt>Ошибок в файлах</dt>
              <dd>{#if files.length}<span class="badge bad">{files.length}</span>{:else}<span class="badge ok">нет</span>{/if}</dd>
            </dl>
            <div class="hrow controls">
              <button class="ghost sm" onclick={() => app.togglePause()}>
                <Icon name={paused ? "play" : "pause"} size={13} />{paused ? "Продолжить" : "Приостановить"}
              </button>
              <button class="ghost sm danger" onclick={() => app.cancel(project.path)}><Icon name="stop" size={13} />Остановить</button>
              <span class="sp"></span>
              <span class="faint tiny">
                Пауза замораживает очередь целиком.
                {rebuilding ? "Остановка оставляет старый индекс; проект продолжит ждать согласия." : "Остановка оставляет уже записанные документы."}
              </span>
            </div>
          </div>
        </div>
      {:else if queued}
        <div class="panel">
          <div class="ch"><h2>В очереди</h2><span class="badge">ждёт</span></div>
          <div class="cb">
            <p class="muted none">{jobReason(queued.kind, queued.priority, queued.files)}.</p>
            <div class="hrow controls">
              <button class="ghost sm danger" onclick={() => app.cancel(project.path)}><Icon name="stop" size={13} />Убрать из очереди</button>
              {#if paused}<span class="badge warn">очередь на паузе</span>
                <button class="ghost sm" onclick={() => app.togglePause()}><Icon name="play" size={13} />Продолжить</button>{/if}
            </div>
          </div>
        </div>
      {/if}

      {#if docQueue && docQueue.total > 0}
        <div class="panel docs" aria-label="Документы в очереди">
          <div class="ch">
            <h2>Документы</h2>
            <span class="badge">{docQueue.total} {plural(docQueue.total, ["документ", "документа", "документов"])}</span>
          </div>
          <p class="muted small sums">
            Ждут паузы: {docQueue.waiting} · в очереди: {docQueue.queued} · идёт embed: {docQueue.indexing} · с ошибкой: {docQueue.error}
          </p>
          <div>
            {#each docQueue.documents as doc (doc.path)}
              <div class="q doc" class:now={doc.state === "indexing"}>
                <div class="grow">
                  <b class="mono">{doc.path}</b>
                  <div class="why">
                    {DOC_SOURCE[doc.source]}{#if doc.state === "waiting" && doc.due_in !== null} · embed через {doc.due_in} с{/if}
                  </div>
                  {#if doc.state === "error" && doc.error}<div class="why err">{doc.error}</div>{/if}
                </div>
                <span class="badge" class:acc={doc.state === "indexing"} class:bad={doc.state === "error"}>{DOC_STATE[doc.state]}</span>
                {#if doc.state === "error"}<button class="ghost sm" onclick={() => app.retryDocument(project.path, doc.path)}>Повторить</button>{/if}
              </div>
            {/each}
          </div>
          {#if docQueue.total > docQueue.documents.length}
            <p class="muted small sums">Показано {docQueue.documents.length} из {docQueue.total}.</p>
          {/if}
        </div>
        <p class="muted small">
          Документ после правки ждёт паузу и индексируется один раз, когда правки стихли. «Проверить сейчас» не ждёт паузы.
        </p>
      {/if}

      {#if !running && !queued}
        <div class="panel">
          <div class="ch"><h2>Последний проход</h2>{#if last}<span class="badge" class:bad={last.state === "error"} class:warn={last.state === "partial" || last.state === "cancelled"}>{resultState[last.state] ?? "Исход неизвестен"}</span>{/if}</div>
          <div class="cb">
            {#if app.indexingHistoryError}
              <p role="alert">Не удалось обновить итог: {app.indexingHistoryError}. Сохранён последний снимок.</p>
              <button class="ghost sm" onclick={() => app.openIndexing()}>Повторить чтение</button>
            {:else if app.indexingHistoryLoading}
              <p role="status">Получаем итог последнего прохода…</p>
            {/if}
            {#if last}
              <p>{operationText(last)}</p>
              <dl class="kv">
                <dt>Начало</dt><dd>{new Date(last.started_at).toLocaleString("ru-RU")}</dd>
                <dt>Окончание</dt><dd>{last.finished_at ? new Date(last.finished_at).toLocaleString("ru-RU") : "Подтверждённого итога нет"}</dd>
                {#if last.report}
                  <dt>Документы</dt><dd>Добавлено {last.report.added} · изменено {last.report.changed} · удалено {last.report.removed} · без изменений {last.report.unchanged} · ошибок {last.report.problems}</dd>
                  <dt>Индекс</dt><dd>{last.kind === "scan" ? "Проверка файлов не изменяет индекс" : last.report.index_applied ? "Результат применён" : "Сохранён прежний индекс"}</dd>
                {/if}
              </dl>
              {#if last.error}<p>{last.error}</p>{/if}
              {#if last.report?.problem_paths?.length}<ul>{#each last.report.problem_paths as path}<li class="mono">{path}</li>{/each}</ul>{/if}
            {:else if !app.indexingHistoryLoading}
              <p>Подтверждённый итог недоступен: проход ещё не выполнялся, worker перезапущен или история вытеснена. Актуальность индекса по отсутствию задания не определяется.</p>
            {/if}
          </div>
        </div>
      {/if}

      {#if files.length}
        <Banner kind="warn" title="Ошибок в документах: {files.length}" text={problems?.rebuild_not_applied ? "Полная переиндексация не применена. Старый индекс сохранён. Устраните причину и повторите переиндексацию." : problems?.last_error ? "Проверка остановлена. Предыдущие обновления документов сохранены; оставшиеся документы ещё нужно проверить." : "Обновление завершено с ошибками в документах. Предыдущие версии документов с ошибками сохранены, если они были в индексе."}>
          <button class="btn sm" onclick={() => problems?.rebuild_not_applied ? app.retryRebuild(project.path) : app.reindexCheck(project.path)}>Повторить</button>
        </Banner>
        <div class="panel">
          <div class="ch"><h2>Файлы с ошибкой</h2><span class="badge bad">{files.length}</span></div>
          <div>
            {#each files as file (file.path)}
              <div class="errfile">
                <div class="t"><Icon name="error" size={15} width={2} /><span class="mono">{file.path}</span></div>
                <div class="m">{file.reason}. {advice(file.reason)}</div>
                {#if file.stage === "embedding"}<div class="m">Векторизация · попыток: {file.attempts} · {file.previous_version ? "предыдущая версия сохранена" : "не проиндексирован"}</div>{/if}
              </div>
            {/each}
          </div>
        </div>
        <div class="panel">
          <div class="ch"><h2>Что делает bars сам</h2></div>
          <div class="cb muted small">
            При сетевой ошибке и ответе 5xx провайдера — до трёх повторов с паузой. Если не помогло, файл остаётся с этой ошибкой; следующая проверка (по правке файла или часовая) попробует снова. Текст последней ошибки без ключей виден здесь и в <span class="mono">bars-cli status</span> (<span class="mono">last_error</span>).
          </div>
        </div>
      {/if}

      {#if problems?.last_error}
        <Banner kind="bad" title="Последняя ошибка индексации" text={problems.last_error}>
          <button class="btn sm" onclick={() => problems?.rebuild_not_applied ? app.retryRebuild(project.path) : app.reindexCheck(project.path)}>Повторить</button>
        </Banner>
      {/if}

      {#if rows.length}
        <div class="panel">
          <div class="ch"><h2>Очередь индексации</h2><span class="badge">{rows.length} {plural(rows.length, ["задание", "задания", "заданий"])}</span></div>
          <div>
            {#each rows as row, index (`${row.running ? "running" : "queued"}:${row.project}`)}
              <div class="q" class:now={row.running}>
                <span class="n">{index + 1}</span>
                <div class="grow"><b>{row.name}</b><div class="why">{jobReason(row.kind, row.priority, row.files)}</div></div>
                {#if row.running}<span class="badge acc">{paused ? "пауза" : "идёт"}</span>{:else}<span class="badge">ждёт</span>{/if}
              </div>
            {/each}
          </div>
        </div>
        <p class="muted small">
          Очередь обрабатывает один проект за раз. Явная переиндексация и хук начала сессии идут раньше правок файлов и проверок при старте.
        </p>
      {/if}

      {#if !running && !queued && !files.length && !problems?.last_error}
        <div class="panel">
          <div class="cb">
            <div class="hrow">
              <span class="muted">Сейчас ничего не индексируется. {documents(app.documentsOf(project))} в индексе.</span>
              <span class="sp"></span>
              <button class="btn sm" onclick={() => app.reindexCheck(project.path)}>Проверить сейчас</button>
            </div>
            <p class="muted small none">bars сверяет документы с индексом сам: по правке файла и каждые 15 минут. Кнопка запускает проверку этого проекта немедленно.</p>
          </div>
        </div>
      {/if}

      <button class="link back" onclick={() => (app.view = "main")}>← К поиску</button>
    </div>
  {/if}
</main>

<style>
  main { overflow-y: auto; min-height: 0; }
  .page { max-width: 820px; margin: 0 auto; padding: 28px 32px 60px; }
  .page h1 { font-size: 22px; margin: 0 0 4px; }
  .lead { color: var(--muted); margin: 0 0 18px; }
  .base { align-items: baseline; }
  .big { font-size: 30px; font-weight: 700; letter-spacing: -.5px; font-variant-numeric: tabular-nums; }
  .stats { display: grid; grid-template-columns: repeat(3, 1fr); gap: 12px; margin: 14px 0; }
  .stat { background: var(--panel-2); border: 1px solid var(--line); border-radius: 10px; padding: 9px 12px; }
  .stat small { display: block; color: var(--muted); font-size: 11.5px; }
  .stat b { font-size: 15px; font-variant-numeric: tabular-nums; }
  .controls { margin-top: 14px; flex-wrap: wrap; }
  .tiny { font-size: 12px; }
  .small { font-size: 12.5px; }
  .none { margin: 0 0 12px; }
  .q { display: flex; gap: 12px; align-items: center; padding: 10px 14px; border-bottom: 1px solid var(--line); font-size: 13px; }
  .q:last-child { border-bottom: 0; }
  .n { width: 22px; height: 22px; border-radius: 50%; background: var(--panel-2); border: 1px solid var(--line); display: grid; place-items: center; font-size: 11.5px; font-weight: 700; color: var(--muted); flex: none; }
  .q.now .n { background: var(--accent); border-color: transparent; color: var(--on-accent); }
  .why { color: var(--muted); font-size: 12px; }
  .grow { flex: 1; }
  .errfile { padding: 10px 14px; border-bottom: 1px solid var(--line); }
  .errfile:last-child { border-bottom: 0; }
  .t { display: flex; align-items: center; gap: 10px; }
  .t .mono { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .t :global(svg) { color: var(--bad); flex: none; }
  .m { color: var(--muted); font-size: 12.5px; margin: 3px 0 0 25px; }
  .back { margin-top: 8px; }
  .sums { padding: 0 14px; margin: 8px 0; }
  .doc .mono { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; display: block; }
  .why.err { color: var(--bad); }
</style>
