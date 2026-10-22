<script lang="ts">
  // Редактор исходного Markdown на CodeMirror 6 (Т-29, ADR-0005). Текст и запись — в буфере `editors`;
  // здесь только вид: подсветка CommonMark и GFM, продолжение списков, автопары, подсветка найденного отрывка.
  import { onMount, untrack } from "svelte";
  import { Annotation, Compartment, EditorState, StateEffect, StateField } from "@codemirror/state";
  import { Decoration, EditorView, keymap, type DecorationSet } from "@codemirror/view";
  import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
  import { closeBrackets, closeBracketsKeymap } from "@codemirror/autocomplete";
  import { markdown, markdownKeymap, markdownLanguage } from "@codemirror/lang-markdown";
  import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
  import { languages } from "@codemirror/language-data";
  import { tags } from "@lezer/highlight";
  import { commandAdapter } from "./command-adapters";
  import { deleteStarPair, editorCommands, moveInTable, starInput } from "./editor-commands";

  let { text, readonly = false, hit = null, label = "Редактор Markdown", oninput }: {
    text: string;
    readonly?: boolean;
    /** Найденный отрывок, строки с 1; подсветка снимается первой правкой. */
    hit?: { first: number; last: number } | null;
    label?: string;
    oninput: (text: string) => void;
  } = $props();

  let host = $state<HTMLDivElement>();
  let view: EditorView | undefined;
  /** Последний текст, который редактор сам отдал наружу: возврат его через `text` не перезаписывает документ. */
  let echoed = untrack(() => text);
  const external = Annotation.define<boolean>();
  const editable = new Compartment();
  const setHit = StateEffect.define<{ from: number; to: number } | null>();
  const hitField = StateField.define<DecorationSet>({
    create: () => Decoration.none,
    update(value, tr) {
      for (const effect of tr.effects) {
        if (!effect.is(setHit)) continue;
        if (!effect.value) return Decoration.none;
        const marks = [];
        for (let n = tr.state.doc.lineAt(effect.value.from).number; n <= tr.state.doc.lineAt(effect.value.to).number; n++) {
          marks.push(Decoration.line({ class: "cm-hit" }).range(tr.state.doc.line(n).from));
        }
        return Decoration.set(marks);
      }
      return tr.docChanged ? Decoration.none : value;
    },
    provide: (field) => EditorView.decorations.from(field),
  });

  const style = HighlightStyle.define([
    { tag: [tags.heading1, tags.heading2, tags.heading3, tags.heading4, tags.heading5, tags.heading6], fontWeight: "700", color: "var(--text)" },
    { tag: tags.strong, fontWeight: "700" },
    { tag: tags.emphasis, fontStyle: "italic" },
    { tag: tags.strikethrough, textDecoration: "line-through" },
    { tag: [tags.link, tags.url], color: "var(--accent-text)" },
    { tag: tags.monospace, background: "var(--panel-2)" },
    { tag: tags.quote, color: "var(--muted)" },
    { tag: [tags.processingInstruction, tags.meta, tags.contentSeparator], color: "var(--muted)" },
    { tag: tags.comment, color: "var(--muted)" },
    { tag: [tags.keyword, tags.string, tags.number, tags.atom, tags.bool], color: "var(--accent-text)" },
  ]);

  const theme = EditorView.theme({
    "&": { height: "100%", background: "var(--panel)", color: "var(--text)", fontSize: "12px" },
    "&.cm-focused": { outline: "none" },
    ".cm-scroller": { fontFamily: "var(--mono)", lineHeight: "1.65", overflow: "auto" },
    ".cm-content": { padding: "10px 14px 60px", caretColor: "var(--accent)" },
    ".cm-cursor": { borderLeftColor: "var(--accent)" },
    ".cm-selectionBackground, &.cm-focused .cm-selectionBackground": { background: "var(--accent-soft)" },
    ".cm-hit": { background: "var(--accent-soft)" },
  });

  const placement = (pos: number) => EditorView.scrollIntoView(pos, { y: "center" });

  /** Ставит курсор в начало строки (с 1) и прокручивает к ней. */
  export function scrollToLine(line: number): void {
    if (!view) return;
    const doc = view.state.doc;
    const info = doc.line(Math.min(Math.max(1, line), doc.lines));
    view.dispatch({ selection: { anchor: info.from }, effects: placement(info.from) });
  }

  export function focus(): void {
    view?.focus();
  }

  /** Выполняет команду разметки `editor.*`; без вида или в документе только для чтения ничего не делает. */
  export function run(id: string): boolean {
    if (!view || readonly) return false;
    return editorCommands[id]?.(view) ?? false;
  }

  const adapter = Object.fromEntries(Object.keys(editorCommands).map((id) => [id, {
    run: () => { run(id); },
    reason: () => (readonly ? "Документ только для чтения" : undefined),
  }]));

  function showHit(next: { first: number; last: number } | null): void {
    if (!view) return;
    const doc = view.state.doc;
    if (!next || next.first < 1 || next.first > doc.lines) {
      view.dispatch({ effects: setHit.of(null) });
      return;
    }
    const from = doc.line(next.first).from;
    const to = doc.line(Math.min(Math.max(next.last, next.first), doc.lines)).to;
    view.dispatch({ effects: [setHit.of({ from, to }), placement(from)] });
  }

  onMount(() => {
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: text,
        extensions: [
          history(),
          closeBrackets(),
          syntaxHighlighting(style),
          markdown({ base: markdownLanguage, codeLanguages: languages }),
          keymap.of([
            { key: "Tab", run: (v) => !readonly && moveInTable(v, 1) },
            { key: "Shift-Tab", run: (v) => !readonly && moveInTable(v, -1) },
            { key: "Backspace", run: (v) => !readonly && deleteStarPair(v) },
            ...closeBracketsKeymap,
            ...markdownKeymap,
            // Mod-i — выбор родителя; сочетание отдано команде «Курсив» из реестра клавиатуры.
            ...defaultKeymap.filter((binding) => binding.key !== "Mod-i"),
            ...historyKeymap,
          ]),
          EditorView.inputHandler.of((v, from, to, input) => !readonly && starInput(v, from, to, input)),
          EditorView.lineWrapping,
          EditorView.contentAttributes.of({ "aria-label": label }),
          editable.of([EditorState.readOnly.of(readonly), EditorView.editable.of(!readonly)]),
          hitField,
          theme,
          EditorView.updateListener.of((update) => {
            if (!update.docChanged || update.transactions.some((tr) => tr.annotation(external))) return;
            echoed = update.state.doc.toString();
            oninput(echoed);
          }),
        ],
      }),
    });
    return () => view?.destroy();
  });

  $effect(() => {
    const next = text;
    if (!view || next === echoed) return;
    echoed = next;
    // Перечитанный с диска текст заменяет документ, курсор остаётся на прежнем смещении.
    const { anchor, head } = view.state.selection.main;
    const end = next.length;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: next },
      selection: { anchor: Math.min(anchor, end), head: Math.min(head, end) },
      annotations: external.of(true),
    });
  });

  $effect(() => {
    const locked = readonly;
    view?.dispatch({ effects: editable.reconfigure([EditorState.readOnly.of(locked), EditorView.editable.of(!locked)]) });
  });

  $effect(() => {
    showHit(hit ? { first: hit.first, last: hit.last } : null);
  });
</script>

<div class="editor" bind:this={host} use:commandAdapter={adapter}></div>

<style>
  .editor { height: 100%; min-height: 0; }
  .editor :global(.cm-editor) { height: 100%; }
</style>
