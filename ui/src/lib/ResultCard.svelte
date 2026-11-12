<script lang="ts">
  import { app } from "./app.svelte";
  import { commandAdapter } from './command-adapters';
  import { copyText } from "./api";
  import { decimal, moduleLabel } from "./format";
  import Icon from "./Icon.svelte";
  import { appearance } from "./appearance.svelte";
  import { colorVar, documentColor, keyOf } from "./appearance";
  import { relativeToDocs } from "./tree";
  import { renderChunk, plainText } from "./markdown";
  import { highlight, markTerms, sectionChain, sectionsWord, VISIBLE_SECTIONS, type DocumentCard, type Marker } from "./search-results";
  import type { SearchHit } from "./types";
  import SimilarSections from "./SimilarSections.svelte";

  // Карточка документа: название, папка и разделы под ним; лучший раздел раскрыт, второй виден строкой,
  // остальные — под «Ещё N разделов» (global-search-design.md, «Проект выдачи: один список»).
  let { card, marker }: { card: DocumentCard; marker: Marker } = $props();

  // Раскрытость раздела — выбор пользователя; пока он не трогал строку, раскрыт только лучший.
  let toggled = $state<Record<number, boolean>>({});
  let allShown = $state(false);
  let similarFor = $state<number | null>(null);
  const project = $derived(app.displayed?.project ?? app.current);
  const best = $derived(card.sections[0]);
  const visible = $derived(allShown ? card.sections : card.sections.slice(0, VISIBLE_SECTIONS));
  const hidden = $derived(card.sections.length - VISIBLE_SECTIONS);
  const dot = $derived(colorVar(documentColor(appearance.data, keyOf(relativeToDocs(card.path, app.docsRoot || "")))));
  const showScores = $derived(app.showScores);
  const results = $derived(app.results);
  /** Выдача по worktree: плашка с именем, граф и «похожие» есть только у основного checkout (worktrees.md, «Чего в worktree нет»). */
  const wtName = $derived(results?.applied.scope?.worktree?.name);
  const wtOnly = "только у основного checkout";
  const hasScore = (hit: SearchHit) => hit.score_kind !== "none";
  const isOpen = (index: number) => toggled[index] ?? index === 0;
  const flip = (index: number) => { toggled = { ...toggled, [index]: !isOpen(index) }; };

  const SIGNALS: { key: "rerank" | "cosine" | "fulltext" | "exact" | "links"; name: string }[] = [
    { key: "rerank", name: "оценка модели (rerank)" },
    { key: "cosine", name: "близость по смыслу" },
    { key: "fulltext", name: "слова запроса" },
    { key: "exact", name: "точное совпадение" },
    { key: "links", name: "связи документов" },
  ];
  /** Строка разбора: число или причина, почему его нет. */
  function signalText(hit: SearchHit, key: (typeof SIGNALS)[number]["key"]): { text: string; na: boolean } {
    const value = hit.signals?.[key];
    if (value !== undefined) return { text: decimal(value), na: false };
    if (key === "links") return { text: "не используется", na: true };
    const used = results?.applied.signals?.includes(key);
    if (used) return { text: "нет сигнала", na: true };
    return { text: results?.semantic_error || results?.rerank_error ? "модель недоступна" : "не использовался", na: true };
  }

  /** Подсветка слов запроса внутри отрисованного раздела. */
  function marks(node: HTMLElement) {
    markTerms(node, marker);
  }

  function openSection(hit: SearchHit) {
    void app.openDocument({ path: hit.path, title: hit.title, first_line: hit.first_line, last_line: hit.last_line, fromSearch: true, terms: { words: marker.words, phrases: marker.phrases }, ...(wtName ? { wt: wtName } : {}) });
  }
  function openDocument() {
    void app.openDocument({ path: card.path, title: card.title, first_line: 0, last_line: 0, fromSearch: true, ...(wtName ? { wt: wtName } : {}) });
  }
  function showInGraph() {
    if (wtName) return;
    app.graphFocus = card.path;
    app.graphShowQuery = true;
    app.view = "graph";
  }
  async function copy(hit: SearchHit) {
    await copyText(`${hit.path}:${hit.first_line}`);
    app.flash("Путь скопирован");
  }
</script>

<article class="doc" data-document-path={card.path} data-document-line={best.first_line} use:commandAdapter={{ 'result.graph': { run: showInGraph }, 'result.open': { run: () => openSection(best) }, 'result.expand': { run: () => flip(0) } }}>
  <div class="dh">
    {#if dot}<span class="dot" style:background={dot} title="Цвет документа" aria-hidden="true"></span>{/if}
    <h3><button class="link title" onclick={openDocument} title="Открыть документ">{card.title}</button></h3>
    {#if wtName}<span class="wt-plate" data-testid="worktree-plate" title="Результат worktree">worktree · {wtName}</span>{/if}
    <span class="sp"></span>
    {#if showScores && hasScore(best)}<span class="score" title="Оценка лучшего раздела">{decimal(best.score)}<span class="bar"><i style="width:{Math.round(Math.min(1, best.score) * 100)}%"></i></span></span>{/if}
  </div>
  <div class="dpath" title={card.path}>{card.folder || moduleLabel(card.module)}</div>
  {#each visible as hit, index (hit.first_line + ":" + hit.chunk_id)}
    {@const open = isOpen(index)}
    {@const chain = sectionChain(hit)}
    <div class="sec" class:open class:minor={index >= VISIBLE_SECTIONS} data-document-line={hit.first_line}>
      <button class="srow" aria-expanded={open} onclick={() => flip(index)}>
        <span class="caret"><Icon name="chevron-right" size={12} width={2.4} /></span>
        <span class="stxt">
          <span class="shead">{#each chain as part, i (i)}{#if i}<i>›</i>{/if}{part}{/each}</span>
          {#if !open}<span class="snip">{#each highlight(plainText(hit.text), marker) as piece}{#if piece.mark}<mark>{piece.text}</mark>{:else}{piece.text}{/if}{/each}</span>{/if}
        </span>
        {#if showScores}
          {#if hasScore(hit)}<span class="score">{decimal(hit.score)}<span class="bar"><i style="width:{Math.round(Math.min(1, hit.score) * 100)}%"></i></span></span>
          {:else}<span class="score" title="Оценки нет: нет опорного сигнала">—</span>{/if}
        {/if}
      </button>
      {#if open}
        <div class="sbody">
          <div class="text" use:marks>{@html renderChunk(hit.text)}</div>
          {#if showScores && hit.signals}
            <div class="sig"><table><tbody>
              {#each SIGNALS as signal (signal.key)}
                {@const value = signalText(hit, signal.key)}
                <tr class:na={value.na}><td>{signal.name}</td><td>{value.text}</td></tr>
              {/each}
            </tbody></table></div>
          {/if}
          <div class="acts">
            <button class="btn sm" onclick={() => openSection(hit)}><Icon name="doc" size={14} />Открыть раздел</button>
            <button class="ghost sm" title="Скопировать путь и строку раздела" onclick={() => copy(hit)}><Icon name="copy" size={14} />Копировать ссылку</button>
            <button class="ghost sm" disabled={!!wtName} title={wtName ? wtOnly : undefined} onclick={showInGraph}>В графе</button>
            <button class="ghost sm" disabled={!hit.chunk_id || !project || !!wtName} title={wtName ? wtOnly : hit.chunk_id ? "Найти соседей по смыслу" : "ID раздела отсутствует. Обновите воркер и повторите поиск."} aria-expanded={similarFor === index} onclick={() => similarFor = similarFor === index ? null : index}>Похожие разделы</button>
          </div>
          {#if similarFor === index && project && hit.chunk_id}
            {#key project + ":" + hit.chunk_id}<SimilarSections {project} sourceId={hit.chunk_id} modules={app.project?.modules ?? []} onopen={(neighbor) => { if (app.current === project) void app.openDocument(neighbor); }} onrefresh={() => { similarFor = null; void app.search(); }} />{/key}
          {/if}
        </div>
      {/if}
    </div>
  {/each}
  {#if hidden > 0}
    <button class="more" aria-expanded={allShown} onclick={() => allShown = !allShown}>{allShown ? "Свернуть ▴" : `Ещё ${hidden} ${sectionsWord(hidden)} в этом документе ▾`}</button>
  {/if}
</article>

<style>
  .wt-plate { margin-left: 8px; padding: 0 6px; border: 1px solid var(--accent); border-radius: 999px; background: var(--accent-soft); color: var(--accent-text); font-size: 11px; line-height: 18px; white-space: nowrap; }
  .doc { background: var(--panel); border: 1px solid var(--line); border-radius: var(--radius); margin-bottom: 10px; overflow: hidden; }
  .doc:hover { border-color: var(--line-2); }
  .dh { display: flex; gap: 10px; align-items: baseline; padding: 12px 16px 0; }
  .dh h3 { margin: 0; font-size: 15.5px; font-weight: 650; letter-spacing: -.1px; min-width: 0; }
  .title { font: inherit; text-align: left; color: var(--text); }
  .title:hover { color: var(--accent-text); text-decoration: underline; }
  .sp { flex: 1; }
  .dot { width: 8px; height: 8px; border-radius: 50%; align-self: center; flex: none; }
  .dpath { padding: 0 16px; color: var(--muted); font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .sec { border-top: 1px solid var(--line); margin-top: 10px; }
  .sec:first-of-type { border-top: 0; margin-top: 4px; }
  .srow { display: flex; gap: 8px; align-items: flex-start; width: 100%; padding: 9px 16px; border: 0; background: none; text-align: left; color: var(--text); }
  .srow:hover { background: var(--panel-2); }
  .caret { flex: none; margin-top: 3px; color: var(--faint); transition: transform .12s; display: grid; }
  .sec.open .caret { transform: rotate(90deg); }
  .stxt { flex: 1; min-width: 0; }
  .shead { font-size: 12.5px; font-weight: 600; color: var(--accent-text); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; display: block; }
  .shead i { font-style: normal; color: var(--faint); font-weight: 400; margin: 0 4px; }
  .sec.minor:not(.open) .snip { display: none; }
  .sec.minor .shead { color: var(--text); font-weight: 500; }
  .snip { display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; color: var(--text); font-size: 13.5px; line-height: 1.5; margin-top: 2px; }
  mark, .text :global(mark) { background: var(--accent-soft); color: inherit; border-radius: 3px; padding: 0 1px; box-shadow: inset 0 -2px 0 var(--accent); }
  .sbody { padding: 0 16px 12px 38px; }
  .text { font-size: 13px; line-height: 1.6; }
  .text :global(p) { margin: 0 0 .75em; } .text :global(ul) { margin: 0 0 .75em; padding-left: 1.3em; } .text :global(li) { margin: .2em 0; }
  .text :global(code) { font: 12.5px var(--mono); background: var(--panel-2); border: 1px solid var(--line); padding: 1px 5px; border-radius: 5px; }
  .acts { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 8px; }
  .score { flex: none; display: inline-flex; align-items: center; gap: 6px; font: 600 12px var(--mono); color: var(--muted); }
  .bar { width: 44px; height: 6px; background: var(--panel-2); border: 1px solid var(--line); border-radius: 6px; overflow: hidden; }
  .bar i { display: block; height: 100%; background: var(--accent); }
  .sig { margin-top: 8px; border: 1px solid var(--line); border-radius: 9px; background: var(--panel-2); padding: 6px 10px; font-size: 12px; }
  .sig table { width: 100%; border-collapse: collapse; }
  .sig td { padding: 2px 4px; } .sig td:last-child { text-align: right; font: 12px var(--mono); }
  .sig tr.na td { color: var(--faint); }
  .more { display: block; width: 100%; border: 0; border-top: 1px solid var(--line); background: var(--panel-2); color: var(--muted); font-size: 12.5px; padding: 7px 16px; text-align: left; font-weight: 600; }
  .more:hover { color: var(--text); }
</style>
