<script lang="ts">
  import { app } from './app.svelte';
  import { type TextHit, type TextDocument } from './text-search.svelte';
  const store = app.textSearch;
  function open(doc: TextDocument, hit: TextHit) {
    void app.openDocument({path:doc.path, title:doc.title, first_line:hit.line, last_line:hit.line, column:hit.column, length:hit.length, fromSearch:true});
  }
  // Строка выводится один раз: между отметками — обычный текст, вхождения — <mark> (ISS-35).
  function pieces(text: string, ranges: [number, number][]): { text: string; mark: boolean }[] {
    const out: { text: string; mark: boolean }[] = [];
    let at = 0;
    for (const [a, b] of [...ranges].sort((x, y) => x[0] - y[0])) {
      if (a < at) continue;
      if (a > at) out.push({ text: text.slice(at, a), mark: false });
      out.push({ text: text.slice(a, b) || '▏', mark: true });
      at = b;
    }
    if (at < text.length) out.push({ text: text.slice(at), mark: false });
    return out;
  }
</script>

{#if store.loading || store.output || store.error}
  <section aria-label="В тексте" class="text-results">
    <h2>В тексте {#if store.output}<small>{store.output.docs_total} док. · {store.output.hits_total}{store.output.truncated ? '+' : ''} вх. · {store.output.elapsed_ms} мс</small>{/if}</h2>
    {#if store.loading}<p role="status">Ищем в файлах…</p>{/if}
    {#if store.error}<p role="alert">{store.error}</p>{/if}
    {#if store.output}
      {#each store.output.documents as doc (doc.path)}
        <article>
          <h3>{doc.title}</h3><div class="path">{doc.path}</div>
          {#each doc.hits as hit, index (`${hit.line}:${hit.column}:${index}`)}
            <button class="occurrence" onclick={() => open(doc, hit)} title={`Открыть строку ${hit.line}, столбец ${hit.column}`}>
              <span class="line">{hit.line}</span><span class="snippet">{#each pieces(hit.text, hit.ranges) as piece}{#if piece.mark}<mark>{piece.text}</mark>{:else}{piece.text}{/if}{/each}</span>
            </button>
          {/each}
          {#if doc.has_more}<button class="link more" disabled={store.more.length > 0} onclick={() => store.loadMore(doc.path)}>{store.more.includes(doc.path) ? 'Загружаем…' : 'Показать ещё'}</button>{/if}
        </article>
      {/each}
      {#if !store.output.documents.length}<p class="muted">Вхождений в файлах не найдено.</p>{/if}
      {#if store.output.truncated && store.output.truncated !== 'page'}<p class="muted">Показана часть вхождений. Уточните запрос или область.</p>{/if}
      {#if store.output.skipped.length}<details><summary>Не прочитано: {store.output.skipped.length}</summary>{#each store.output.skipped as skipped}<p>{skipped.path}: {skipped.reason}</p>{/each}</details>{/if}
    {/if}
  </section>
{/if}

<style>
  .text-results { border-bottom:1px solid var(--line); margin-bottom:12px; padding-bottom:8px; }
  h2 {font-size:13px; margin:8px 0; display:flex; flex-wrap:wrap; gap:8px; align-items:baseline;}
  small,.path,.line {color:var(--muted); font-size:11px; font-weight:400;}
  h3 {font-size:12px; margin:8px 0 2px;}
  .path {overflow-wrap:anywhere; margin-bottom:3px;}
  .occurrence {display:flex; gap:8px; width:100%; border:0; border-radius:3px; background:transparent; text-align:left; padding:3px 0; font-size:12px;}
  .occurrence:hover {background:var(--panel-2);}
  .line {min-width:28px; text-align:right; flex-shrink:0;}
  .snippet {white-space:pre-wrap; overflow-wrap:anywhere; min-width:0;}
  mark {background:var(--accent-soft); color:var(--accent-text);}
  p,details,.more {font-size:12px; margin:6px 0;}
</style>
