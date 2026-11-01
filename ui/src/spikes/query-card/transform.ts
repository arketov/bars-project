// Изолированный макет на актуальных компонентах: ошибки и действия продукта не копируются.
function replaceOnce(source: string, before: string, after: string): string {
  if (source.split(before).length !== 2) throw new Error(`Макет карточки: изменился исходный компонент (${before.slice(0, 70)})`);
  return source.replace(before, after);
}

export function queryCardDesign(input: string, kind: "search" | "editor"): string {
  let source = input.replaceAll("\r\n", "\n");
  const swap = (before: string, after: string) => { source = replaceOnce(source, before, after); };
  if (kind === "search") {
    const start = source.indexOf('        <div class="search-options">');
    const end = source.indexOf('        {#if !app.viewer}', start);
    if (start < 0 || end < start) throw new Error("Макет карточки: не найдены параметры поиска");
    const options = source.slice(start, end);
    swap(options, "");
    const relatedStart = source.indexOf("      {#if app.retrievalMode === 'mix'}");
    const relatedEnd = source.indexOf('\n    </div>\n  </div>', relatedStart);
    if (relatedStart < 0 || relatedEnd < relatedStart) throw new Error("Макет карточки: не найдены текстовые параметры");
    const textOptions = source.slice(relatedStart, relatedEnd);
    const controlsStart = textOptions.indexOf('      <div class="text-options">');
    if (controlsStart < 0) throw new Error("Макет карточки: не найдены переключатели текстового поиска");
    const related = textOptions.slice(0, controlsStart);
    const controls = textOptions.slice(controlsStart);
    swap(textOptions, `<details class="query-parameters">
        <summary>Параметры{#if parameterSummary}{' · '}{parameterSummary}{/if}</summary>
        ${options}
        ${related}
        {#if app.retrievalMode === 'mix'}<p class="parameter-caption">Эксперимент: улучшение порядка результатов не подтверждено.</p>{/if}
        <p class="parameter-caption">Только в тексте: регистр, выражение и путь</p>
        ${controls}
      </details>`);
    swap('  const resultsWord =', `  const parameterSummary = $derived([
    app.retrievalMode !== 'hybrid' ? uiLabel('searchMode', app.retrievalMode).text : '',
    app.retrievalMode === 'mix' && !app.includeRelated ? 'без связанных источников' : '',
    app.count !== null ? app.shownCount + ' рез.' : '',
    app.textSearch.caseSensitive ? 'регистр' : '',
    app.textSearch.regex ? 'выражение' : '',
    app.textSearch.pathFilter.trim() ? 'путь' : '',
  ].filter(Boolean).join(' · '));
  const emptyText = $derived(!!app.textSearch.output && !app.textSearch.loading && !app.textSearch.error
    && !app.textSearch.output.documents.length && !app.textSearch.output.skipped.length && !app.textSearch.output.truncated);
  const resultsWord =`);
    swap('      <TextResults />', '      {#if !emptyText}<TextResults />{/if}');
    swap('    </div>\n  </div>\n</main>', '      {#if emptyText}<p class="text-empty" role="status">В тексте: вхождений не найдено</p>{/if}\n    </div>\n  </div>\n</main>');
    swap('          {#if blocked}\n            По смыслу: нужна переиндексация\n          {:else if touched.length}', '          {#if touched.length}');
    swap('  .scope { flex: 1 0 80px; color: var(--muted); font-size: 12.5px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }',
      '  .scope { flex: 1 1 180px; color: var(--muted); font-size: 12.5px; min-width: 0; overflow-wrap: anywhere; }');
    swap('flex-wrap: nowrap; align-items: center; gap: 12px;', 'flex-wrap: wrap; align-items: center; gap: 12px;');
    swap('</style>', `  .query-parameters { border-top: 1px solid var(--line); margin-top: 8px; padding-top: 7px; font-size: 12px; }
  .query-parameters summary { cursor: pointer; color: var(--muted); overflow-wrap: anywhere; }
  .query-parameters summary:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .query-parameters .search-options { margin-top: 10px; }
  .parameter-caption { font-size: 11px; color: var(--muted); margin: 10px 0 4px; }
  .text-empty { font-size: 12px; color: var(--muted); border-top: 1px solid var(--line); padding-top: 10px; }
</style>`);
  } else {
    swap('Decoration, EditorView, keymap, type DecorationSet', 'Decoration, EditorView, keymap, WidgetType, type DecorationSet');
    swap('  const hitField =', `  class FoundLabel extends WidgetType {
    readonly first: number;
    readonly last: number;
    constructor(first: number, last: number) { super(); this.first = first; this.last = last; }
    eq(other: FoundLabel) { return this.first === other.first && this.last === other.last; }
    toDOM() {
      const node = document.createElement('div');
      node.className = 'cm-found-label';
      node.textContent = this.first === this.last ? 'строка ' + this.first + ' · найдено' : 'строки ' + this.first + '–' + this.last + ' · найдено';
      return node;
    }
  }
  const hitField =`);
    swap('        const marks = [];', `        const first = tr.state.doc.lineAt(effect.value.from);
        const last = tr.state.doc.lineAt(effect.value.to);
        const marks = [Decoration.widget({ widget: new FoundLabel(first.number, last.number), block: true, side: -1 }).range(first.from)];`);
    swap("        if (effect.value.exact && effect.value.to > effect.value.from) return Decoration.set([Decoration.mark({class:'cm-hit'}).range(effect.value.from,effect.value.to)]);", `        if (effect.value.exact) {
          marks.push(Decoration.line({ class: effect.value.to === effect.value.from ? 'cm-hit cm-found-line' : 'cm-found-line' }).range(first.from));
          if (effect.value.to > effect.value.from) marks.push(Decoration.mark({ class: 'cm-hit' }).range(effect.value.from, effect.value.to));
          return Decoration.set(marks, true);
        }`);
    swap('Decoration.line({ class: "cm-hit" })', 'Decoration.line({ class: "cm-hit cm-found-line" })');
    swap('return Decoration.set(marks);', 'return Decoration.set(marks, true);');
    swap('    ".cm-hit": { background: "var(--accent-soft)" },', `    ".cm-hit": { background: "var(--accent-soft)" },
    ".cm-found-line": { borderLeft: "2px solid var(--accent)" },
    ".cm-found-label": { fontFamily: "var(--sans)", fontSize: "11px", color: "var(--accent-text)", padding: "3px 8px", borderLeft: "2px solid var(--accent)", background: "var(--accent-soft)" },`);
  }
  return source;
}
