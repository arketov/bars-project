<script lang="ts">
  import { onDestroy } from 'svelte';
  import { keybindings } from './keybindings.svelte';
  import { catalog, bindingFor, conflicts, defaultBinding, fromEvent, invalid, label, platform, signature, validate, type Binding, type Overrides } from './keybindings';
  let draft = $state<Overrides>(structuredClone($state.snapshot(keybindings.snapshot.profile.platforms[platform] ?? {})));
  let revision = $state(keybindings.snapshot.profile.revision), recovery = $state<string | null>(null);
  let query = $state(''), category = $state(''), changed = $state(false), editing = $state(''), candidate = $state<Binding | null>(null), error = $state(''), saving = $state(false), resetOpen = $state(false);
  const categories = [...new Set(catalog.map(c => c.category))];
  const problems = $derived(validate(draft, platform));
  const dirty = $derived(JSON.stringify(draft) !== JSON.stringify(keybindings.snapshot.profile.platforms[platform] ?? {}) || recovery !== null);
  const rows = $derived(catalog.filter(c => (!category || c.category === category) && (!changed || Object.hasOwn(draft, c.id)) && `${c.title} ${c.id} ${c.category} ${label(bindingFor(c, draft, platform))}`.toLowerCase().includes(query.toLowerCase())));
  const unknown = $derived(Object.keys(draft).filter(id => !catalog.some(c => c.id === id)));
  const stale = $derived(revision !== keybindings.snapshot.profile.revision);
  const candidateError = $derived(candidate ? invalid(candidate, platform) : 'Нажмите сочетание');
  const collision = $derived(candidate ? conflicts({ ...draft, [editing]: candidate }, platform).filter(pair => pair.some(c => c.id === editing)) : []);
  function stop() { editing = ''; candidate = null; keybindings.recording = false; }
  function resetDraft() { stop(); draft = structuredClone($state.snapshot(keybindings.snapshot.profile.platforms[platform] ?? {})); revision = keybindings.snapshot.profile.revision; recovery = null; error = ''; }
  function record(event: KeyboardEvent) {
    if (event.isComposing || event.keyCode === 229 || event.getModifierState('AltGraph') || ['Dead','Process'].includes(event.key)) return;
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); stop(); return; }
    if (event.key === 'Tab') return;
    if (event.target instanceof HTMLButtonElement && ['Enter',' '].includes(event.key)) return;
    const binding = fromEvent(event);
    if (!binding) { error = 'Клавиша не распознана или идёт ввод IME/AltGr'; return; }
    event.preventDefault(); event.stopPropagation(); candidate = binding; error = '';
  }
  function accept(replace = false) {
    if (!candidate || candidateError || collision.length && !replace) return;
    const next = { ...draft };
    if (replace) for (const pair of collision) for (const c of pair) if (c.id !== editing) next[c.id] = null;
    const command = catalog.find(c => c.id === editing)!;
    if (signature(candidate) === signature(defaultBinding(command, platform))) delete next[editing]; else next[editing] = candidate;
    draft = next; stop();
  }
  async function save() { saving = true; error = ''; try { await keybindings.save($state.snapshot(draft), revision, recovery); resetDraft(); } catch (e) { error = String(e); } finally { saving = false; } }
  onDestroy(() => { keybindings.recording = false; });
</script>

<section aria-label="Сочетания клавиш" class="keybindings">
  <h1>Сочетания клавиш</h1>
  <p>Физическая позиция клавиши одинакова в EN и RU; русская буква — в скобках. На Mac для F6 может требоваться Fn.</p>
  {#if keybindings.error}<p role="alert">{keybindings.error}</p>{/if}
  {#if !keybindings.available}<button onclick={() => keybindings.load()}>Повторить загрузку профиля</button>{/if}
  {#if stale}<p role="alert">Карта изменена другим окном. Черновик сохранён. Отмените правки, чтобы загрузить новую ревизию.</p>{/if}
  <div class="filters"><input aria-label="Найти действие или сочетание" placeholder="Действие или сочетание" bind:value={query} /><select aria-label="Категория действий" bind:value={category}><option value="">Все категории</option>{#each categories as item}<option>{item}</option>{/each}</select><label><input type="checkbox" bind:checked={changed} /> Изменённые</label></div>
  <div class="rows">{#each rows as command (command.id)}
    <div class="binding-row"><span><b>{command.title}</b><small>{command.category} · {command.context}{command.future ? ` · ${command.future}` : ''}</small></span><span>{label(bindingFor(command, draft, platform))}</span><div class="buttons"><button onclick={() => { editing = command.id; candidate = null; keybindings.recording = true; }}>Назначить</button><button onclick={() => draft = { ...draft, [command.id]: null }}>Убрать</button><button onclick={() => { const next = { ...draft }; delete next[command.id]; draft = next; }}>По умолчанию</button></div></div>
  {/each}</div>
  {#each unknown as id}<p>Неизвестное действие: {id} — неактивное назначение сохранится.</p>{/each}
  {#each problems as problem}<p role="alert">{problem}</p>{/each}
  {#if error}<p role="alert">{error}</p>{/if}
  <div class="actions"><button onclick={() => resetOpen = true}>Сбросить все</button><button onclick={resetDraft}>Отменить / загрузить актуальные</button><button disabled={!dirty || saving || !!problems.length || stale || !keybindings.available || !!keybindings.snapshot.error && recovery === null} onclick={save}>{saving ? 'Сохраняю…' : 'Сохранить сочетания'}</button><span>Ревизия {revision}</span></div>
</section>
{#if editing}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <dialog open use:modal aria-label="Запись сочетания" onkeydown={record} oncancel={(e) => { e.preventDefault(); stop(); }}>
    <h2>{catalog.find(c => c.id === editing)?.title}</h2><p>Нажмите сочетание; Escape — отмена.</p><p>{candidate ? label(candidate) : 'Ожидание клавиш'}</p>
    {#if candidateError}<p role="alert">{candidateError}</p>{/if}
    {#each collision as [a, b]}<p role="alert">{a.title} ({a.context}) ↔ {b.title} ({b.context})</p>{/each}
    <button disabled={!candidate || !!candidateError || !!collision.length} onclick={() => accept()}>Принять</button>{#if collision.length}<button disabled={!!candidateError} onclick={() => accept(true)}>Переназначить сюда</button>{/if}<button onclick={stop}>Отмена</button>
  </dialog>
{/if}
{#if resetOpen}<dialog open use:modal aria-label="Сброс сочетаний" oncancel={() => resetOpen = false}><p>Сбросить все назначения? Изменится только черновик. Повреждённый файл заменится после сохранения.</p><button onclick={() => { draft = {}; recovery = keybindings.snapshot.recovery; revision = keybindings.snapshot.profile.revision; resetOpen = false; }}>Сбросить</button><button onclick={() => resetOpen = false}>Отмена</button></dialog>{/if}

<script lang="ts" module>
  function modal(node: HTMLDialogElement) { const previous = document.activeElement as HTMLElement | null; node.removeAttribute('open'); node.showModal(); node.querySelector<HTMLElement>('button')?.focus(); return { destroy() { node.close(); if (previous?.isConnected) previous.focus(); } }; }
</script>
<style>
  .keybindings { font-size:12px; container-type:inline-size; min-width:0; } p { color:var(--muted); } [role=alert] { color:var(--bad); } .filters,.actions,.buttons { display:flex; gap:6px; flex-wrap:wrap; align-items:center; } .filters { margin-bottom:12px; } input,select,button { font:inherit; max-width:100%; min-width:0; box-sizing:border-box; } .binding-row { display:grid; grid-template-columns:minmax(0,1fr) minmax(0,.7fr) minmax(0,1fr); gap:8px; align-items:center; padding:8px 0; border-bottom:1px solid var(--line); } small { display:block; color:var(--muted); margin-top:4px; } .actions { margin:14px 0; } dialog { position:fixed; margin:auto; max-width:min(500px,90vw); background:var(--panel); color:var(--text); border:1px solid var(--line); border-radius:8px; padding:18px; } dialog::backdrop { background:#0005; } @container(max-width:600px) { .binding-row { grid-template-columns:1fr; } }
</style>
