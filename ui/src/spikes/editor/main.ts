// Исследование 3.1.2: CodeMirror 6 в окне bars. Не подключено к App.svelte.
import { EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { closeBrackets, closeBracketsKeymap } from "@codemirror/autocomplete";
import { markdown, markdownLanguage, markdownKeymap } from "@codemirror/lang-markdown";
import { LanguageDescription, syntaxHighlighting, defaultHighlightStyle, syntaxTree, ensureSyntaxTree } from "@codemirror/language";
import { languages } from "@codemirror/language-data";

const SECTION = [
  "---", "title: Раздел", "---", "",
  "## Раздел {n}", "",
  "Обычный абзац с **жирным**, *курсивом*, `кодом` и [ссылкой](../a/b.md). Русский текст для проверки переноса строк и подсветки разметки в длинных документах.", "",
  "- пункт один", "- [ ] задача", "- [x] сделано", "",
  "> цитата", "",
  "| а | б |", "|---|---|", "| 1 | 2 |", "",
  "```rust", "fn main() { println!(\"привет\"); }", "```", "",
].join("\n");

export const makeText = (bytes: number): string => {
  const parts: string[] = [];
  let size = 0;
  for (let n = 0; size < bytes; n++) {
    const part = SECTION.replace("{n}", String(n));
    parts.push(part);
    size += new TextEncoder().encode(part).length;
  }
  return parts.join("\n");
};

type Config = { lang: "plain" | "gfm" | "gfm+code"; wrap: boolean };

export function mount(parent: HTMLElement, doc: string, config: Config): EditorView {
  const extensions = [
    history(), closeBrackets(), syntaxHighlighting(defaultHighlightStyle),
    keymap.of([...closeBracketsKeymap, ...markdownKeymap, ...defaultKeymap, ...historyKeymap]),
    EditorView.theme({ "&": { height: "100%" }, ".cm-content": { fontFamily: "monospace", fontSize: "12px" } }),
  ];
  if (config.wrap) extensions.push(EditorView.lineWrapping);
  if (config.lang !== "plain") {
    extensions.push(markdown({ base: markdownLanguage, codeLanguages: config.lang === "gfm+code" ? languages : [] }));
  }
  return new EditorView({ parent, state: EditorState.create({ doc, extensions }) });
}

const median = (v: number[]) => [...v].sort((a, b) => a - b)[Math.floor(v.length / 2)];
const p95 = (v: number[]) => [...v].sort((a, b) => a - b)[Math.floor(v.length * 0.95)];
const frame = () => new Promise<number>((r) => requestAnimationFrame(() => r(performance.now())));

export async function measure(bytes: number, config: Config) {
  const host = document.getElementById("app")!;
  host.innerHTML = '<div id="h" style="height:600px;width:700px;overflow:hidden"></div>';
  const parent = document.getElementById("h")!;
  const text = makeText(bytes);
  const t0 = performance.now();
  const view = mount(parent, text, config);
  const created = performance.now() - t0;
  await frame();
  const firstPaint = performance.now() - t0;
  const typing: number[] = [];
  const mid = Math.floor(text.length / 2);
  for (let i = 0; i < 150; i++) {
    const s = performance.now();
    view.dispatch({ changes: { from: mid + i, insert: "ы" }, selection: { anchor: mid + i + 1 } });
    typing.push(performance.now() - s);
    if (i % 10 === 9) await frame();
  }
  const withPaint: number[] = [];
  for (let i = 0; i < 40; i++) {
    const s = performance.now();
    view.dispatch({ changes: { from: mid + 200 + i, insert: "ж" } });
    await frame();
    withPaint.push(performance.now() - s);
  }
  const scrollFrames: number[] = [];
  let last = await frame();
  for (let i = 0; i < 60; i++) {
    view.scrollDOM.scrollTop += 400;
    const now = await frame();
    scrollFrames.push(now - last);
    last = now;
  }
  const s2 = performance.now();
  view.state.doc.toString();
  const toString = performance.now() - s2;
  const tree = syntaxTree(view.state);
  const result = {
    bytes: text.length, utf8: new TextEncoder().encode(text).length, lines: view.state.doc.lines, config,
    createMs: +created.toFixed(1), firstPaintMs: +firstPaint.toFixed(1),
    dispatchMedianMs: +median(typing).toFixed(2), dispatchP95Ms: +p95(typing).toFixed(2),
    typeToPaintMedianMs: +median(withPaint).toFixed(1), typeToPaintP95Ms: +p95(withPaint).toFixed(1),
    scrollFrameMedianMs: +median(scrollFrames).toFixed(1), scrollFrameP95Ms: +p95(scrollFrames).toFixed(1),
    toStringMs: +toString.toFixed(2), treeLength: tree.length, domLines: view.dom.querySelectorAll(".cm-line").length,
    heapMB: (performance as unknown as { memory?: { usedJSHeapSize: number } }).memory ? +((performance as unknown as { memory: { usedJSHeapSize: number } }).memory.usedJSHeapSize / 1048576).toFixed(1) : null,
  };
  view.destroy();
  return result;
}

/** Полный разбор для структуры документа: highlight разбирает только видимую часть. */
export function fullParse(bytes: number) {
  const view = mount(document.createElement("div"), makeText(bytes), { lang: "gfm", wrap: false });
  const t = performance.now();
  const tree = ensureSyntaxTree(view.state, view.state.doc.length, 5000);
  const ms = performance.now() - t;
  let headings = 0;
  tree?.iterate({ enter: (n) => { if (/^(ATXHeading|SetextHeading)/.test(n.name)) headings++; } });
  const total = performance.now() - t;
  view.destroy();
  return { utf8: new TextEncoder().encode(view.state.doc.toString()).length, parsed: !!tree, parseMs: +ms.toFixed(1), withIterateMs: +total.toFixed(1), headings };
}

(window as unknown as Record<string, unknown>).spike = { makeText, mount, measure, fullParse, LanguageDescription, EditorView, EditorState };
