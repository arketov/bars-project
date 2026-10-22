// Помощники разметки редактора (Т-29): команды `editor.*` над EditorView, переход по ячейкам таблицы и автопара `*`.
// Сочетания приходят из реестра клавиатуры (Т-45): сами команды клавиш не знают.

import { EditorSelection, type ChangeSpec, type EditorState } from "@codemirror/state";
import type { Line } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";

export type EditorCommand = (view: EditorView) => boolean;

/** Строки, которых касается выделение; конечная пустая строка выделения до её начала не считается. */
function selectedLines(state: EditorState): Line[] {
  const seen = new Set<number>();
  const lines: Line[] = [];
  for (const range of state.selection.ranges) {
    const first = state.doc.lineAt(range.from).number;
    let last = state.doc.lineAt(range.to).number;
    if (range.to > range.from && state.doc.line(last).from === range.to && last > first) last--;
    for (let n = first; n <= last; n++) {
      if (seen.has(n)) continue;
      seen.add(n);
      lines.push(state.doc.line(n));
    }
  }
  return lines;
}

/** Для нескольких строк пустые пропускаются; единственная строка берётся как есть. */
const contentLines = (lines: Line[]) => (lines.length > 1 ? lines.filter((line) => line.text.trim() !== "") : lines);

function toggleInline(view: EditorView, open: string, close = open): boolean {
  const { state } = view;
  view.dispatch(
    state.changeByRange((original) => {
      const doc = state.doc;
      // Пробелы и перевод строки на краях выделения (тройной щелчок выделяет строку с концом) в пару не входят.
      let { from, to } = original;
      while (from < to && /\s/.test(doc.sliceString(from, from + 1))) from++;
      while (to > from && /\s/.test(doc.sliceString(to - 1, to))) to--;
      const range = from === to ? EditorSelection.cursor(original.from) : EditorSelection.range(from, to);
      if (from === to) from = to = range.head;
      const before = doc.sliceString(Math.max(0, from - open.length), from);
      const after = doc.sliceString(to, to + close.length);
      // `*` в `**x**` — часть жирного, а не курсива.
      const part = (pos: number, char: string) => doc.sliceString(Math.max(0, pos), pos + 1) === char;
      const embedded = open === "*" && (part(from - open.length - 1, "*") || part(to + close.length, "*"));
      if (before === open && after === close && !embedded) {
        return {
          changes: [{ from: from - open.length, to: from }, { from: to, to: to + close.length }],
          range: EditorSelection.range(from - open.length, to - open.length),
        };
      }
      const selected = doc.sliceString(from, to);
      if (selected.length >= open.length + close.length && selected.startsWith(open) && selected.endsWith(close) && !(open === "*" && selected.startsWith("**"))) {
        return {
          changes: [{ from, to: from + open.length }, { from: to - close.length, to }],
          range: EditorSelection.range(from, to - open.length - close.length),
        };
      }
      return {
        changes: [{ from, insert: open }, { from: to, insert: close }],
        range: range.empty ? EditorSelection.cursor(from + open.length) : EditorSelection.range(from + open.length, to + open.length),
      };
    }),
    { userEvent: "input.markup", scrollIntoView: true },
  );
  return true;
}

function heading(level: number): EditorCommand {
  return (view) => {
    const lines = contentLines(selectedLines(view.state));
    const marker = /^(#{1,6})[ \t]+/;
    const same = lines.every((line) => marker.exec(line.text)?.[1].length === level);
    const changes: ChangeSpec[] = lines.map((line) => {
      const found = marker.exec(line.text);
      return { from: line.from, to: line.from + (found?.[0].length ?? 0), insert: same ? "" : `${"#".repeat(level)} ` };
    });
    view.dispatch({ changes, userEvent: "input.markup", scrollIntoView: true });
    return true;
  };
}

const LIST_ANY = /^(\s*)(?:[-*+] \[[ xX]\] |[-*+] |\d+[.)] )/;
const LIST_KIND = {
  bullet: /^\s*[-*+] (?!\[[ xX]\] )/,
  numbered: /^\s*\d+[.)] /,
  task: /^\s*[-*+] \[[ xX]\] /,
};

function list(kind: keyof typeof LIST_KIND): EditorCommand {
  return (view) => {
    const lines = contentLines(selectedLines(view.state));
    const same = lines.every((line) => LIST_KIND[kind].test(line.text));
    const changes: ChangeSpec[] = lines.map((line, index) => {
      const found = LIST_ANY.exec(line.text);
      const indent = found?.[1] ?? /^\s*/.exec(line.text)![0];
      const prefix = kind === "bullet" ? "- " : kind === "task" ? "- [ ] " : `${index + 1}. `;
      return { from: line.from, to: line.from + (found?.[0].length ?? indent.length), insert: same ? indent : indent + prefix };
    });
    view.dispatch({ changes, userEvent: "input.markup", scrollIntoView: true });
    return true;
  };
}

const quote: EditorCommand = (view) => {
  const lines = contentLines(selectedLines(view.state));
  const same = lines.every((line) => /^\s*> ?/.test(line.text));
  const changes: ChangeSpec[] = lines.map((line) => {
    if (!same) return { from: line.from, insert: "> " };
    const found = /^(\s*)> ?/.exec(line.text)!;
    return { from: line.from, to: line.from + found[0].length, insert: found[1] };
  });
  view.dispatch({ changes, userEvent: "input.markup", scrollIntoView: true });
  return true;
};

const codeBlock: EditorCommand = (view) => {
  const { state } = view;
  const lines = selectedLines(state);
  const first = lines[0], last = lines[lines.length - 1];
  const before = first.number > 1 ? state.doc.line(first.number - 1) : null;
  const after = last.number < state.doc.lines ? state.doc.line(last.number + 1) : null;
  if (before && after && /^\s*(```|~~~)/.test(before.text) && /^\s*(```|~~~)\s*$/.test(after.text)) {
    view.dispatch({ changes: [{ from: before.from, to: first.from }, { from: last.to, to: after.to }], userEvent: "input.markup" });
    return true;
  }
  if (lines.length === 1 && first.text.trim() === "") {
    view.dispatch({ changes: { from: first.from, to: first.to, insert: "```\n\n```" }, selection: { anchor: first.from + 4 }, userEvent: "input.markup", scrollIntoView: true });
    return true;
  }
  view.dispatch({
    changes: [{ from: first.from, insert: "```\n" }, { from: last.to, insert: "\n```" }],
    // Выделение остаётся на строках: повторная команда видит их и снимает ограждение.
    selection: { anchor: first.from + 4, head: last.to + 4 },
    userEvent: "input.markup",
    scrollIntoView: true,
  });
  return true;
};

const TABLE_TEMPLATE = "| Столбец 1 | Столбец 2 | Столбец 3 |\n| --- | --- | --- |\n|  |  |  |";

const table: EditorCommand = (view) => {
  const { state } = view;
  const line = state.doc.lineAt(state.selection.main.head);
  const blank = line.text.trim() === "";
  const at = blank ? line.from : line.to;
  const prefix = blank ? "" : "\n\n";
  const start = at + prefix.length + 2;
  view.dispatch({
    changes: { from: at, to: blank ? line.to : at, insert: prefix + TABLE_TEMPLATE },
    selection: { anchor: start, head: start + "Столбец 1".length },
    userEvent: "input.markup",
    scrollIntoView: true,
  });
  return true;
};

/** Ссылка на документ: путь вводится вручную; выбор документа через `[[` — этап 3.5.2 (Т-30). */
const link: EditorCommand = (view) => {
  const { state } = view;
  view.dispatch(
    state.changeByRange((range) => {
      if (range.empty) {
        return { changes: { from: range.from, insert: "[текст]()" }, range: EditorSelection.range(range.from + 1, range.from + 6) };
      }
      return {
        changes: [{ from: range.from, insert: "[" }, { from: range.to, insert: "]()" }],
        range: EditorSelection.cursor(range.to + 3),
      };
    }),
    { userEvent: "input.markup", scrollIntoView: true },
  );
  return true;
};

export const editorCommands: Record<string, EditorCommand> = {
  "editor.bold": (view) => toggleInline(view, "**"),
  "editor.italic": (view) => toggleInline(view, "*"),
  "editor.code": (view) => toggleInline(view, "`"),
  "editor.heading1": heading(1),
  "editor.heading2": heading(2),
  "editor.heading3": heading(3),
  "editor.bulletList": list("bullet"),
  "editor.numberedList": list("numbered"),
  "editor.task": list("task"),
  "editor.quote": quote,
  "editor.codeBlock": codeBlock,
  "editor.table": table,
  "editor.link": link,
};

// --- Таблицы ---------------------------------------------------------------------------------------------

const width = (text: string) => [...text].length;

function splitRow(text: string): string[] {
  let body = text.trim();
  if (body.startsWith("|")) body = body.slice(1);
  if (body.endsWith("|") && !body.endsWith("\\|")) body = body.slice(0, -1);
  const cells: string[] = [];
  let current = "";
  for (let i = 0; i < body.length; i++) {
    if (body[i] === "\\" && body[i + 1] === "|") {
      current += "\\|";
      i++;
    } else if (body[i] === "|") {
      cells.push(current.trim());
      current = "";
    } else current += body[i];
  }
  cells.push(current.trim());
  return cells;
}

const isDelimiter = (cells: string[]) => cells.every((cell) => /^:?-+:?$/.test(cell));

interface TableBlock { first: Line; last: Line; rows: string[][] }

/** Таблица GFM вокруг позиции: подряд идущие строки с `|`, вторая строка — разделитель. */
function tableAt(state: EditorState, pos: number): TableBlock | null {
  const { doc } = state;
  const startLine = doc.lineAt(pos);
  if (!/^\s*\|/.test(startLine.text)) return null;
  let first = startLine.number, last = startLine.number;
  while (first > 1 && /^\s*\|/.test(doc.line(first - 1).text)) first--;
  while (last < doc.lines && /^\s*\|/.test(doc.line(last + 1).text)) last++;
  const rows: string[][] = [];
  for (let n = first; n <= last; n++) rows.push(splitRow(doc.line(n).text));
  if (rows.length < 2 || !isDelimiter(rows[1])) return null;
  return { first: doc.line(first), last: doc.line(last), rows };
}

/** Выравнивает столбцы по самой длинной ячейке; возвращает строки и ширины. */
function formatTable(rows: string[][]): { lines: string[]; widths: number[] } {
  const columns = Math.max(...rows.map((row) => row.length));
  const full = rows.map((row) => Array.from({ length: columns }, (_, i) => row[i] ?? ""));
  const widths = Array.from({ length: columns }, (_, j) =>
    Math.max(3, ...full.filter((_, i) => i !== 1).map((row) => width(row[j]))),
  );
  const lines = full.map((row, i) => {
    const cells = row.map((cell, j) => {
      if (i === 1) {
        const left = cell.startsWith(":"), right = cell.endsWith(":") && cell.length > 1;
        return (left ? ":" : "-") + "-".repeat(widths[j] - 2) + (right ? ":" : "-");
      }
      return cell + " ".repeat(widths[j] - width(cell));
    });
    return `| ${cells.join(" | ")} |`;
  });
  return { lines, widths };
}

/** Номер ячейки под курсором: считаются неэкранированные `|` слева от него, ведущий не в счёт. */
function cellAt(text: string, column: number): number {
  const head = text.slice(0, column);
  let pipes = 0;
  for (let i = 0; i < head.length; i++) {
    if (head[i] === "\\") i++;
    else if (head[i] === "|") pipes++;
  }
  return Math.max(0, pipes - (text.trimStart().startsWith("|") ? 1 : 0));
}

/** `Tab` / `Shift+Tab` в таблице: переход к следующей или предыдущей ячейке с выравниванием столбцов. */
export function moveInTable(view: EditorView, direction: 1 | -1): boolean {
  const { state } = view;
  const head = state.selection.main.head;
  const block = tableAt(state, head);
  if (!block) return false;
  const line = state.doc.lineAt(head);
  const columns = Math.max(...block.rows.map((row) => row.length));
  let row = line.number - block.first.number;
  let cell = Math.min(cellAt(line.text, head - line.from), columns - 1);
  const rows = block.rows.map((r) => [...r]);
  cell += direction;
  if (cell >= columns) { cell = 0; row++; }
  if (cell < 0) { cell = columns - 1; row--; }
  if (row === 1) row += direction;
  if (row < 0) return true;
  if (row >= rows.length) rows.push(Array.from({ length: columns }, () => ""));
  const { lines, widths } = formatTable(rows);
  let start = block.first.from;
  for (let i = 0; i < row; i++) start += lines[i].length + 1;
  start += 2 + widths.slice(0, cell).reduce((sum, w) => sum + w + 3, 0);
  view.dispatch({
    changes: { from: block.first.from, to: block.last.to, insert: lines.join("\n") },
    selection: { anchor: start, head: start + [...(rows[row][cell] ?? "")].length },
    userEvent: "input.markup",
    scrollIntoView: true,
  });
  return true;
}

// --- Автопара `*` ----------------------------------------------------------------------------------------

/** Ввод `*`: пара вокруг выделения, `**` из `*|*`, шаг за закрывающую `*`; в начале строки, в словах и в «2 * 3» не вмешивается. */
export function starInput(view: EditorView, from: number, to: number, text: string): boolean {
  if (text !== "*") return false;
  const { doc } = view.state;
  if (from !== to) {
    view.dispatch({
      changes: [{ from, insert: "*" }, { from: to, insert: "*" }],
      selection: { anchor: from + 1, head: to + 1 },
      userEvent: "input.type",
    });
    return true;
  }
  const line = doc.lineAt(from);
  const prev = from > line.from ? doc.sliceString(from - 1, from) : "";
  const next = from < line.to ? doc.sliceString(from, from + 1) : "";
  if (prev === "*" && next === "*") {
    view.dispatch({ changes: { from, insert: "**" }, selection: { anchor: from + 1 }, userEvent: "input.type" });
    return true;
  }
  const opened = line.text.slice(0, from - line.from).includes("*");
  if (next === "*" && opened) {
    view.dispatch({ selection: { anchor: from + 1 }, userEvent: "input.type" });
    return true;
  }
  const lineStart = line.text.slice(0, from - line.from).trim() === "";
  if (!lineStart && /^$|[\s(\[]/.test(prev) && /^$|[)\].,;:!?\s]/.test(next)) {
    view.dispatch({ changes: { from, insert: "**" }, selection: { anchor: from + 1 }, userEvent: "input.type" });
    return true;
  }
  return false;
}

/** `Backspace` между `*` и `*` убирает обе. */
export function deleteStarPair(view: EditorView): boolean {
  const { state } = view;
  const range = state.selection.main;
  if (!range.empty || range.from === 0) return false;
  if (state.doc.sliceString(range.from - 1, range.from + 1) !== "**") return false;
  view.dispatch({ changes: { from: range.from - 1, to: range.from + 1 }, userEvent: "delete.backward" });
  return true;
}
