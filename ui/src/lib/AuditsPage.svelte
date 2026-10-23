<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "./app.svelte";
  import Banner from "./Banner.svelte";
  import Icon from "./Icon.svelte";
  import {
    DUPLICATES_DEFAULT,
    KIND_HINT,
    KIND_LABEL,
    SCHEDULE_LABEL,
    STATE_LABEL,
    countsLabel,
    emptyResultLabel,
    isBusy,
    linesLabel,
    roleLabel,
    stamp,
    stateNote,
    tagLabel,
    type AuditFinding,
    type AuditKind,
    type AuditSubject,
    type ScheduleMode,
  } from "./audits";
  import { decimal } from "./format";

  // Экран «Аудиты» (docs/features/knowledge-base/audits.md): состояние видов, запуск, расписание и находки.
  // Аудит читает документацию и ничего в ней не меняет; находка — повод посмотреть документ.
  const store = app.audits;
  const KINDS: AuditKind[] = ["broken_links", "duplicates", "contradictions"];
  const list = $derived(store.list);

  onMount(() => {
    void store.load(store.list !== null);
  });

  // Пока вид идёт или ждёт, состояние обновляется чаще.
  $effect(() => {
    if (!store.anyBusy) return;
    const timer = setInterval(() => void store.load(true), 1500);
    return () => clearInterval(timer);
  });

  function openSubject(subject: AuditSubject) {
    const name = subject.path.split("/").pop() ?? subject.path;
    void app.openDocument({ path: subject.path, title: subject.heading.split("/").pop() || name, first_line: subject.first_line, last_line: subject.last_line });
  }

  const scheduleOf = (event: Event): ScheduleMode => (event.currentTarget as HTMLSelectElement).value as ScheduleMode;
  const standingLabel = (finding: AuditFinding) => (finding.standing === "new" ? "новая" : finding.standing === "gone" ? "исчезла" : "");
</script>

<section class="viewer" aria-label="Аудиты">
  <div class="vhead">
    <div class="vt"><b>Аудиты</b><small>Аудит ничего не меняет в файлах и не доказывает отсутствия проблем</small></div>
    <button class="iconbtn" title="Закрыть" aria-label="Закрыть «Аудиты»" onclick={() => app.closeAudits()}><Icon name="close" size={16} /></button>
  </div>
  <div class="body">
    {#if store.error}
      <div class="empty-block" role="alert">
        <strong>Не удалось прочитать аудиты</strong>
        <p class="muted">{store.error}</p>
        <button class="ghost sm" onclick={() => store.load()}>Повторить</button>
      </div>
    {:else if store.loading && !list}
      <div class="skeleton" role="status" aria-label="Загрузка аудитов"></div>
      <div class="skeleton"></div>
    {:else if list}
      <label class="overlap small"><input type="checkbox" checked={store.showIgnored} onchange={(event) => store.setShowIgnored(event.currentTarget.checked)} />Игнорируемые</label>
      {#if list.reindex_required}
        <Banner kind="warn" title="Индекс требует переиндексации" text="Повторы не запустить, пока индекс не обновлён; показан прежний результат." />
      {:else if list.indexing}
        <Banner kind="warn" title="Идёт индексация" text="Результат «Дублей» может быть неполным: такой запуск не заменяет прежний полный." />
      {/if}

      {#each KINDS as kind (kind)}
        {@const audit = store.summary(kind)}
        {#if audit}
          {@const note = stateNote(audit, list)}
          {@const page = store.findings[kind]}
          <article class="kind" aria-label={KIND_LABEL[kind]}>
            <header>
              <div class="title">
                <h2>{KIND_LABEL[kind]}</h2>
                <span class="state {audit.state}" role="status">{STATE_LABEL[audit.state]}</span>
              </div>
              <p class="muted small">{KIND_HINT[kind]}</p>
              <p class="meta small">
                {#if audit.state === "running" && audit.running_since}
                  <span>идёт с {stamp(audit.running_since)}</span>
                {:else if audit.last_run_at}
                  <span>запуск {stamp(audit.last_run_at)}</span>
                {/if}
                {#if countsLabel(audit)}<span class="counts" class:fresh={audit.new_count > 0}>{countsLabel(audit)}</span>{/if}
              </p>
              {#if note}<p class="note small" class:bad={audit.state === "failed"}>{note}</p>{/if}
              <div class="controls">
                <label class="schedule small">Расписание
                  <select
                    aria-label="Расписание: {KIND_LABEL[kind]}"
                    value={audit.schedule}
                    disabled={store.busy}
                    onchange={(event) => store.setSchedule(kind, scheduleOf(event))}
                  >
                    {#each audit.schedules as mode (mode)}<option value={mode}>{SCHEDULE_LABEL[mode]}</option>{/each}
                  </select>
                </label>
                {#if isBusy(audit)}
                  <button class="ghost sm" onclick={() => store.cancel(kind)}>Остановить</button>
                {:else}
                  <button class="btn sm" disabled={store.busy} onclick={() => store.run(kind)}>Запустить</button>
                {/if}
                <button class="ghost sm" aria-expanded={!!store.open[kind]} disabled={audit.result_run_id === undefined} onclick={() => store.toggle(kind)}>
                  {store.open[kind] ? "Скрыть находки" : "Показать находки"}
                </button>
              </div>
              {#if kind === "duplicates"}
                <div class="shown small">
                  <span class="muted">Показаны: {DUPLICATES_DEFAULT}</span>
                  <label class="overlap">
                    <input type="checkbox" checked={store.showOverlap} onchange={(event) => store.setOverlap(event.currentTarget.checked)} />
                    Перекрытие содержания{audit.overlap_total > 0 ? ` (${audit.overlap_total})` : ""}
                  </label>
                </div>
              {/if}
            </header>

            {#if store.open[kind]}
              <div class="findings" aria-label="Находки: {KIND_LABEL[kind]}">
                {#if page?.error}
                  <p class="note bad small" role="alert">{page.error}</p>
                  <button class="ghost sm" onclick={() => store.loadFindings(kind, true)}>Повторить</button>
                {:else if page?.loading && !page.items.length}
                  <div class="skeleton" role="status" aria-label="Загрузка находок"></div>
                {:else if page && page.items.length === 0}
                  <p class="muted small">{emptyResultLabel(audit) || "Находок нет."}</p>
                {:else if page}
                  {#each page.items as finding, index (index)}
                    <div class="finding" class:gone={finding.standing === "gone"} data-standing={finding.standing}>
                      <p class="reason">
                        <span class="tagname">{tagLabel(finding.tag)}</span>
                        {#if finding.score !== undefined && kind === "duplicates"}<span class="score">{decimal(finding.score)}</span>{/if}
                        <span>{finding.reason}</span>
                        {#if standingLabel(finding)}<span class="stand {finding.standing}">{standingLabel(finding)}</span>{/if}
                      </p>
                      {#each finding.subjects as subject, i (i)}
                        <button class="subject" onclick={() => openSubject(subject)} title="Открыть документ на строке">
                          <code>{subject.path}:{linesLabel(subject)}</code>
                          {#if kind === "duplicates"}<span class="role">{roleLabel(subject.role)}</span>{/if}
                          {#if subject.heading}<span class="muted small">{subject.heading.split("/").join(" / ")}</span>{/if}
                        </button>
                      {/each}
                      {#if finding.detail}<p class="detail small">{finding.detail}</p>{/if}
                      <button class="ghost sm" disabled={store.busy} onclick={() => store.ignore(kind, finding)}>{finding.ignored ? "Вернуть" : "Игнорировать"}</button>
                    </div>
                  {/each}
                  {#if page.next !== null}
                    <button class="ghost sm more" disabled={page.loading} onclick={() => store.loadFindings(kind, false)}>
                      Ещё ({page.total - page.items.length})
                    </button>
                  {/if}
                {/if}
              </div>
            {/if}
          </article>
        {/if}
      {/each}

      <article class="kind" aria-label="Новые темы">
        <header>
          <div class="title"><h2>Новые темы</h2></div>
          <p class="muted small">Вопросы, для которых не нашлись материалы: своя страница, сбор идёт сам.</p>
          <div class="controls">
            <button class="ghost sm" onclick={() => app.openTopics()}>
              Открыть{app.topics.count === null ? "" : ` · активных ${app.topics.count}`}
            </button>
          </div>
        </header>
      </article>
    {/if}
  </div>
</section>

<style>
  .viewer { display: flex; flex-direction: column; min-width: 0; min-height: 0; background: var(--panel); border-left: 1px solid var(--line); animation: slide .25s both; }
  @keyframes slide { from { opacity: 0; transform: translateX(16px); } to { opacity: 1; transform: none; } }
  .vhead { display: flex; align-items: center; gap: 10px; padding: 14px 16px 12px 22px; border-bottom: 1px solid var(--line); }
  .vt { flex: 1; min-width: 0; }
  .vt b { display: block; font-size: 15px; }
  .vt small { display: block; color: var(--muted); font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .iconbtn { border: 1px solid transparent; background: none; color: var(--muted); padding: 5px; border-radius: 8px; display: grid; place-items: center; }
  .iconbtn:hover { background: var(--panel-2); border-color: var(--line); color: var(--text); }
  .body { flex: 1; min-height: 0; overflow-y: auto; padding: 16px 22px 48px; display: flex; flex-direction: column; gap: 14px; max-width: 880px; }
  .kind { border: 1px solid var(--line); border-radius: 12px; padding: 14px 16px; background: var(--panel); }
  .title { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  h2 { font-size: 15px; margin: 0; }
  .small { font-size: 12px; }
  p { margin: 6px 0 0; line-height: 1.5; }
  .state { font-size: 11px; font-weight: 600; border: 1px solid var(--line); padding: 1px 9px; border-radius: 20px; color: var(--muted); }
  .state.running, .state.queued { color: var(--accent-text); border-color: var(--accent); }
  .state.complete { color: var(--text); }
  .state.partial { color: var(--mid); border-color: var(--mid); }
  .state.failed { color: var(--bad, #c0392b); border-color: var(--bad, #c0392b); }
  .meta { display: flex; gap: 12px; flex-wrap: wrap; color: var(--muted); font-variant-numeric: tabular-nums; }
  .counts.fresh { color: var(--accent-text); font-weight: 600; }
  .note { color: var(--mid); }
  .note.bad { color: var(--bad, #c0392b); }
  .controls { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-top: 10px; }
  .schedule { display: inline-flex; align-items: center; gap: 6px; color: var(--muted); margin-right: 4px; }
  .schedule select { border: 1px solid var(--line); background: var(--panel-2); border-radius: 8px; padding: 4px 8px; font-size: 12px; color: inherit; }
  .shown { display: flex; gap: 14px; align-items: center; flex-wrap: wrap; margin-top: 10px; }
  .overlap { display: inline-flex; align-items: center; gap: 6px; }
  .findings { margin-top: 14px; border-top: 1px solid var(--line); padding-top: 10px; display: flex; flex-direction: column; gap: 8px; }
  .finding { border: 1px solid var(--line); border-radius: 10px; padding: 9px 11px; }
  .finding.gone { opacity: .55; }
  .reason { display: flex; gap: 8px; flex-wrap: wrap; align-items: baseline; margin: 0 0 4px; font-size: 13px; }
  .tagname { font-weight: 600; }
  .score { color: var(--muted); font-variant-numeric: tabular-nums; }
  .stand { font-size: 11px; font-weight: 600; padding: 0 7px; border-radius: 20px; border: 1px solid var(--line); }
  .stand.new { color: var(--accent-text); border-color: var(--accent); }
  .stand.gone { color: var(--muted); }
  .subject { display: flex; gap: 8px; flex-wrap: wrap; align-items: baseline; width: 100%; text-align: left; border: 0; background: transparent; padding: 3px 4px; border-radius: 6px; color: inherit; }
  .subject:hover { background: var(--panel-2); }
  .subject:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .subject code { font: 11.5px var(--mono); overflow-wrap: anywhere; }
  .role { font-size: 11px; color: var(--muted); border: 1px solid var(--line); border-radius: 20px; padding: 0 7px; white-space: nowrap; }
  .detail { color: var(--muted); white-space: pre-wrap; overflow-wrap: anywhere; margin-top: 6px; border-left: 2px solid var(--line); padding-left: 8px; }
  .more { align-self: flex-start; }
  .empty-block { padding: 34px 10px; text-align: center; color: var(--muted); }
  .empty-block strong { display: block; color: var(--text); margin-bottom: 8px; }
  .skeleton { height: 56px; background: var(--panel-2); border-radius: 10px; margin: 6px 0; animation: pulse 1s infinite alternate; }
  @keyframes pulse { to { opacity: .45; } }
  @media (prefers-reduced-motion: reduce) { .skeleton { animation: none; } }
  .ghost.sm, .btn.sm { padding: 5px 11px; font-size: 12.5px; }
</style>
