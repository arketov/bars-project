<script lang="ts">
  import { call } from "./api";
  import { BarsError, type ModuleInfo, type SearchHit, type SimilarOutput } from "./types";
  import { decimal, moduleLabel } from "./format";
  import { renderChunk } from "./markdown";

  let { project, sourceId, modules = [], onopen, onrefresh }: {
    project: string; sourceId: string; modules?: ModuleInfo[];
    onopen: (hit: SearchHit) => void; onrefresh: () => void;
  } = $props();
  let neighbor = $state<string | null>(null), chosen = $state<string[]>([]);
  let limit = $state<number | undefined>(undefined), threshold = $state<number | undefined>(0);
  let reply = $state<SimilarOutput | null>(null), busy = $state(false), error = $state("");
  let stale = $state(false), retry = $state(0);
  const validLimit = $derived(limit === undefined || (Number.isInteger(limit) && limit > 0 && limit <= 4294967295));
  const validThreshold = $derived(threshold !== undefined && Number.isFinite(threshold) && threshold >= 0 && threshold <= 1);
  const visible = $derived(reply?.results.filter(hit => validThreshold && hit.score >= threshold!) ?? []);

  $effect(() => {
    const root = project, id = neighbor ?? sourceId, scope = [...chosen], count = limit;
    void retry;
    if (!validLimit) return;
    let cancelled = false;
    reply = null; error = ""; stale = false; busy = true;
    void call<SimilarOutput>("similar", { cwd: root, project: root, chunk_id: id, modules: scope, ...(count === undefined ? {} : { limit: count }) })
      .then(result => { if (!cancelled) reply = result; })
      .catch(e => {
        if (cancelled) return;
        stale = e instanceof BarsError && e.kind === "chunk_not_found";
        error = stale ? "Раздел изменился в индексе. Обновите поиск и выберите отрывок заново." : String(e);
      })
      .finally(() => { if (!cancelled) busy = false; });
    return () => { cancelled = true; };
  });
</script>

<section class="similar" aria-label="Похожие разделы">
  <p class="note">По проиндексированной версии. Соседи по смыслу, не доказанные дубли. Область соседей: {chosen.length ? chosen.map(moduleLabel).join(", ") : "весь проект"}.</p>
  <details><summary>Модули соседей</summary>
    {#each modules as module}<label><input type="checkbox" value={module.name} bind:group={chosen} />{moduleLabel(module.name)}</label>{/each}
    {#if chosen.length}<button onclick={() => chosen = []}>Весь проект</button>{/if}
  </details>
  <div class="controls">
    <label>Лимит <input aria-label="Лимит похожих разделов" type="number" min="1" step="1" placeholder="Из настроек" bind:value={limit} /></label>
    <label>Косинус от <input aria-label="Порог похожих разделов" type="number" min="0" max="1" step="0.01" bind:value={threshold} /></label>
    {#if neighbor}<button onclick={() => neighbor = null}>К исходному отрывку</button>{/if}
  </div>
  {#if !validLimit}<p role="alert">Лимит — целое число от 1 до 4294967295.</p>{/if}
  {#if !validThreshold}<p role="alert">Порог — число от 0 до 1.</p>{/if}
  {#if busy && validLimit}<p role="status">Поиск похожих разделов…</p>
  {:else if error}<p role="alert">{error}</p>
    {#if stale}<button onclick={onrefresh}>Обновить поиск</button>
    {:else}<button onclick={() => retry++}>Повторить</button>{/if}
  {:else if reply && validLimit}
    <div class="source" data-document-path={reply.source.path} data-document-line={reply.source.first_line} data-menu-kind="file" data-menu-place="card"><b>Исходный отрывок</b><button class="path" onclick={() => onopen(reply!.source)}>{reply.source.path}:{reply.source.first_line}–{reply.source.last_line} · {reply.source.heading_chain}</button>
      <details><summary>Текст исходного отрывка</summary><div class="text">{@html renderChunk(reply.source.text)}</div></details>
    </div>
    <p class="note" role="status">Показано {visible.length} из {reply.results.length}; лимит {reply.limit}{reply.clamped ? " (ограничен настройками)" : ""}. Порог применяется к полученным соседям.</p>
    {#if reply.indexing}<p role="status">Идёт индексация: результат может быть неполным. В очереди: {reply.pending}.</p>{/if}
    {#if !reply.results.length}<p>Других разделов в выбранной области нет.</p>
    {:else if validThreshold && !visible.length}<p>Нет соседей выше выбранного порога. Уменьшите порог.</p>{/if}
    {#each visible as hit}
      <article data-document-path={hit.path} data-document-line={hit.first_line} data-menu-kind="file" data-menu-place="card">
        <b>{hit.heading_chain || hit.title}</b>
        <small>{hit.path}:{hit.first_line}–{hit.last_line} · {moduleLabel(hit.module)} · косинус {decimal(hit.score)}</small>
        <div class="text">{@html renderChunk(hit.text)}</div>
        <button onclick={() => onopen(hit)}>Открыть документ</button>
        {#if hit.chunk_id}<button onclick={() => neighbor = hit.chunk_id!}>Похожие</button>{/if}
      </article>
    {/each}
  {/if}
</section>

<style>
  .similar { border-top: 1px solid var(--line); margin-top: 8px; padding-top: 8px; font-size: 12px; overflow-wrap: anywhere; }
  .note, small { color: var(--muted); } p { margin: 6px 0; }
  .controls { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; margin: 6px 0; }
  label { display: inline-flex; gap: 4px; align-items: center; margin-right: 8px; }
  input[type=number] { width: 100px; min-width: 0; background: var(--panel); color: var(--text); border: 1px solid var(--line); padding: 3px; }
  button { color: var(--accent-text); background: transparent; border: 1px solid var(--line); border-radius: 4px; padding: 3px 6px; margin: 3px 4px 3px 0; }
  article, .source { padding: 8px 0; border-top: 1px solid var(--line); }
  small, .path { display: block; } .path { text-align: left; }
  .text { max-height: 220px; overflow: auto; } .text :global(pre) { overflow-x: auto; }
  .text :global(p) { margin: 5px 0; }
</style>
