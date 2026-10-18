<script lang="ts">
  import { app } from "./app.svelte";
  import { copyText } from "./api";
  import { decimal, fit, moduleLabel } from "./format";
  import Icon from "./Icon.svelte";
  import { plainText, renderChunk } from "./markdown";
  import type { SearchHit } from "./types";

  let { hit, rank, startOpen = false }: { hit: SearchHit; rank: number; startOpen?: boolean } = $props();

  // Раскрытость — выбор пользователя; пока он не трогал карточку, первая раскрыта (main-window.md).
  let toggled = $state<boolean | null>(null);
  const open = $derived(toggled ?? startOpen);
  const f = $derived(fit(hit.score, hit.score_kind));
  const shown = $derived(app.viewer?.hit.path === hit.path && app.viewer.hit.first_line === hit.first_line);
  const crumbs = $derived(hit.heading_chain.split("/").join(" / "));
  const title = $derived(hit.heading_chain.split("/").pop() || hit.title);

  async function copy() {
    await copyText(`${hit.path}:${hit.first_line}`);
    app.flash("Путь скопирован");
  }
</script>

<div class="card" class:open class:shown data-document-path={hit.path} data-document-line={hit.first_line}>
  <div
    class="chead"
    role="button"
    tabindex="0"
    onclick={() => (toggled = !open)}
    onkeydown={(event) => (event.key === "Enter" || event.key === " ") && (toggled = !open)}
  >
    <div class="rank" class:first={rank === 1}>{rank}</div>
    <div class="head">
      <div class="ctitle">{title}</div>
      <div class="crumbs"><span class="mono">{hit.path}:{hit.first_line}</span> · {crumbs}</div>
      <div class="badges"><span class="badge acc">{moduleLabel(hit.module)}</span><span class="badge">{hit.title}</span></div>
      {#if !open}<div class="snippet">{plainText(hit.text)}</div>{/if}
    </div>
    <div class="fit {f.cls}" title={f.explanation}>
      <div class="pct">{f.percent}<small>%</small></div>
      <div class="flabel">{f.label}</div>
      <div class="sbar" role="meter" aria-label={f.label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={f.percent}><i style="width:{f.percent}%"></i></div>
    </div>
    <div class="chev"><Icon name="chevron-down" size={16} /></div>
  </div>
  {#if open}
    <div class="cin">
      <div class="text">{@html renderChunk(hit.text)}</div>
      <div class="cfoot">
        <button class="btn" onclick={() => app.openDocument(hit)}><Icon name="doc" size={14} />Открыть документ</button>
        <button class="ghost" onclick={copy}><Icon name="copy" size={14} />Копировать путь:строку</button>
        <span class="nums">строки {hit.first_line}–{hit.last_line} · {hit.score_kind} {decimal(hit.score)}</span>
      </div>
    </div>
  {/if}
</div>

<style>
  .card { background: var(--panel); border: 1px solid var(--line); border-radius: var(--radius); margin-bottom: 12px; box-shadow: var(--shadow); overflow: hidden; transition: border-color .15s; }
  .card:hover { border-color: var(--line-2); }
  .card.shown { border-color: var(--accent); }
  .chead { display: grid; grid-template-columns: 30px 1fr auto 22px; gap: 14px; align-items: center; padding: 14px 16px; cursor: pointer; }
  .rank { width: 30px; height: 30px; border-radius: 9px; background: var(--panel-2); border: 1px solid var(--line); display: grid; place-items: center; font-weight: 700; color: var(--muted); font-size: 13px; }
  .rank.first { background: var(--accent-soft); border-color: transparent; color: var(--accent-text); }
  .head { min-width: 0; }
  .ctitle { font-weight: 600; font-size: 15px; line-height: 1.3; }
  .crumbs { color: var(--muted); font-size: 12.5px; margin-top: 3px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .crumbs .mono { color: var(--accent-text); font-size: 12px; }
  .badges { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 7px; }
  .snippet { color: var(--muted); margin-top: 8px; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; font-size: 13.5px; }
  .fit { text-align: right; min-width: 118px; }
  .pct { font-size: 24px; font-weight: 700; line-height: 1; font-variant-numeric: tabular-nums; }
  .pct small { font-size: 13px; font-weight: 600; margin-left: 1px; }
  .flabel { font-size: 11.5px; font-weight: 600; margin-top: 4px; }
  .sbar { height: 5px; background: var(--low-soft); border-radius: 5px; margin-top: 7px; overflow: hidden; }
  .sbar i { display: block; height: 100%; border-radius: 5px; }
  .f-good .pct, .f-good .flabel { color: var(--good); } .f-good .sbar i { background: var(--good); }
  .f-mid .pct, .f-mid .flabel { color: var(--mid); } .f-mid .sbar i { background: var(--mid); }
  .f-low .pct, .f-low .flabel { color: var(--low); } .f-low .sbar i { background: var(--low); }
  .f-cosine .pct, .f-cosine .flabel { color: var(--accent-text); } .f-cosine .sbar i { background: var(--accent); }
  .chev { color: var(--faint); transition: transform .2s; display: grid; place-items: center; }
  .open .chev { transform: rotate(180deg); }
  .cin { padding: 4px 20px 18px 60px; }
  .text { font-size: 14.5px; line-height: 1.65; }
  .text :global(p) { margin: 0 0 .75em; } .text :global(ul) { margin: 0 0 .75em; padding-left: 1.3em; } .text :global(li) { margin: .2em 0; }
  .text :global(code) { font: 12.5px var(--mono); background: var(--panel-2); border: 1px solid var(--line); padding: 1px 5px; border-radius: 5px; }
  .cfoot { display: flex; flex-wrap: wrap; gap: 10px 12px; align-items: center; margin-top: 14px; padding-top: 12px; border-top: 1px dashed var(--line); }
  .nums { color: var(--faint); font-size: 12px; margin-left: auto; font-variant-numeric: tabular-nums; }
</style>
