<script lang="ts">
  import { app } from "./app.svelte";
  import Banner from "./Banner.svelte";
  import { decimal, documents, duration, jobReason, percentOf, remainingSeconds } from "./format";
  import Icon from "./Icon.svelte";

  const project = $derived(app.project);
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

<main>
  {#if project}
    <div class="page">
      <h1>{project.name}</h1>
      <p class="lead">
        {#if rebuilding}
          Полная переиндексация по вашему согласию: индекс строится заново в отдельном файле.
        {:else if running}
          Идёт проверка проекта: bars сверяет документы с индексом.
        {:else if queued}
          Проект ждёт своей очереди.
        {:else if files.length}
          Индекс обновлён, кроме {files.length} {files.length === 1 ? "файла" : "файлов"}: они остались с ошибкой.
        {:else}
          Индексация не идёт: индекс актуален.
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
              <div class="stat"><small>Устройство</small><b>—</b></div>
            </div>
            <dl class="kv">
              <dt>Сейчас</dt><dd class="mono">{running.current_file ?? "—"}</dd>
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

      {#if files.length}
        <Banner kind="warn" title="{files.length} {files.length === 1 ? 'документ не проиндексирован' : 'документов не проиндексированы'}" text="Всё остальное в индексе актуально и ищется. Уже проиндексированная версия этих файлов (если была) не тронута.">
          <button class="btn sm" onclick={() => app.reindexCheck(project.path)}>Повторить</button>
        </Banner>
        <div class="panel">
          <div class="ch"><h2>Файлы с ошибкой</h2><span class="badge bad">{files.length}</span></div>
          <div>
            {#each files as file (file.path)}
              <div class="errfile">
                <div class="t"><Icon name="error" size={15} width={2} /><span class="mono">{file.path}</span></div>
                <div class="m">{file.reason}. {advice(file.reason)}</div>
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
          <button class="btn sm" onclick={() => app.reindexCheck(project.path)}>Повторить</button>
        </Banner>
      {/if}

      {#if rows.length}
        <div class="panel">
          <div class="ch"><h2>Очередь индексации</h2><span class="badge">{rows.length} {rows.length === 1 ? "проект" : "проекта"}</span></div>
          <div>
            {#each rows as row, index (row.project)}
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
</style>
