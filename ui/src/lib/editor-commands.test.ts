import { afterEach, expect, it } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { insertNewlineContinueMarkup, markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { deleteStarPair, editorCommands, moveInTable, starInput } from "./editor-commands";

let view: EditorView | undefined;
afterEach(() => { view?.destroy(); view = undefined; document.body.replaceChildren(); });

/** `«…»` — выделение, `‸` — курсор; разметка в тексте документа не остаётся. */
function open(marked: string): EditorView {
  let doc = marked, anchor = 0, head = 0;
  const a = doc.indexOf("«"), c = doc.indexOf("‸");
  if (a >= 0) {
    doc = doc.replace("«", "");
    anchor = a;
    head = doc.indexOf("»");
    doc = doc.replace("»", "");
  } else if (c >= 0) {
    doc = doc.replace("‸", "");
    anchor = head = c;
  }
  view = new EditorView({
    parent: document.body,
    state: EditorState.create({ doc, selection: { anchor, head }, extensions: [markdown({ base: markdownLanguage })] }),
  });
  return view;
}
const text = () => view!.state.doc.toString();
const selected = () => view!.state.sliceDoc(view!.state.selection.main.from, view!.state.selection.main.to);
const cursor = () => view!.state.selection.main.head;
const run = (id: string) => expect(editorCommands[id](view!)).toBe(true);

it("жирный оборачивает выделение, повтор снимает разметку", () => {
  open("a «текст» b");
  run("editor.bold");
  expect([text(), selected()]).toEqual(["a **текст** b", "текст"]);
  run("editor.bold");
  expect([text(), selected()]).toEqual(["a текст b", "текст"]);
});

it("жирный не захватывает пробелы и конец строки на краях выделения", () => {
  open("«слово\n»следующая");
  run("editor.bold");
  expect(text()).toBe("**слово**\nследующая");
  open("а\n«\n»б");
  run("editor.bold");
  expect(text()).toBe("а\n****\nб");
});

it("жирный без выделения ставит пару и курсор между", () => {
  open("a ‸");
  run("editor.bold");
  expect([text(), cursor()]).toEqual(["a ****", 4]);
});

it("курсив внутри жирного не снимает его звёздочки", () => {
  open("**«x»**");
  run("editor.italic");
  expect(text()).toBe("***x***");
});

it("строчный код и ссылка", () => {
  open("«код»");
  run("editor.code");
  expect(text()).toBe("`код`");
  open("«слово»");
  run("editor.link");
  expect([text(), cursor()]).toEqual(["[слово]()", 8]);
  open("‸");
  run("editor.link");
  expect([text(), cursor()]).toEqual(["[[]]", 2]); // без выделения открывается выбор документа (Т-30)
});

it("заголовок ставится, меняет уровень и снимается; пустые строки пропускаются", () => {
  open("«один\n\nдва»");
  run("editor.heading2");
  expect(text()).toBe("## один\n\n## два");
  run("editor.heading3");
  expect(text()).toBe("### один\n\n### два");
  run("editor.heading3");
  expect(text()).toBe("один\n\nдва");
});

it("списки: маркированный, нумерованный и задача переключаются с сохранением отступа", () => {
  open("«а\n  б»");
  run("editor.bulletList");
  expect(text()).toBe("- а\n  - б");
  run("editor.task");
  expect(text()).toBe("- [ ] а\n  - [ ] б");
  run("editor.numberedList");
  expect(text()).toBe("1. а\n  2. б");
  run("editor.numberedList");
  expect(text()).toBe("а\n  б");
});

it("цитата переключается", () => {
  open("«а\nб»");
  run("editor.quote");
  expect(text()).toBe("> а\n> б");
  run("editor.quote");
  expect(text()).toBe("а\nб");
});

it("блок кода оборачивает строки, на пустой строке даёт пустой блок, повтор снимает ограждение", () => {
  open("«а\nб»");
  run("editor.codeBlock");
  expect(text()).toBe("```\nа\nб\n```");
  run("editor.codeBlock");
  expect(text()).toBe("а\nб");
  open("‸");
  run("editor.codeBlock");
  expect([text(), cursor()]).toEqual(["```\n\n```", 4]);
});

it("таблица вставляется после абзаца и выделяет первый заголовок", () => {
  open("абзац‸");
  run("editor.table");
  expect(text()).toBe("абзац\n\n| Столбец 1 | Столбец 2 | Столбец 3 |\n| --- | --- | --- |\n|  |  |  |");
  expect(selected()).toBe("Столбец 1");
});

const TABLE = "| а | бб |\n|---|:-:|\n| 1 | 2 |";

it("Tab в таблице выравнивает столбцы и переходит к следующей ячейке, пропуская разделитель", () => {
  open(TABLE.replace("| а |", "| а‸ |"));
  expect(moveInTable(view!, 1)).toBe(true);
  expect(text()).toBe("| а   | бб  |\n| --- | :-: |\n| 1   | 2   |");
  expect(selected()).toBe("бб");
  expect(moveInTable(view!, 1)).toBe(true);
  expect(selected()).toBe("1");
  expect(view!.state.doc.lineAt(cursor()).number).toBe(3);
});

it("Tab в последней ячейке добавляет строку, Shift+Tab возвращает назад", () => {
  open("| а | б |\n| --- | --- |\n| 1 | 2‸ |");
  expect(moveInTable(view!, 1)).toBe(true);
  expect(text().split("\n")).toHaveLength(4);
  expect(view!.state.doc.lineAt(cursor()).number).toBe(4);
  expect(moveInTable(view!, -1)).toBe(true);
  expect(selected()).toBe("2");
});

it("Tab вне таблицы и в одиночной строке с `|` не перехватывается", () => {
  open("обычный‸ текст");
  expect(moveInTable(view!, 1)).toBe(false);
  open("| не таблица‸ |");
  expect(moveInTable(view!, 1)).toBe(false);
});

it("`*`: пара, `**` из пары, шаг за закрывающую и выделение; не мешает маркеру списка и умножению", () => {
  open("слово ‸");
  expect(starInput(view!, 6, 6, "*")).toBe(true);
  expect([text(), cursor()]).toEqual(["слово **", 7]);
  expect(starInput(view!, 7, 7, "*")).toBe(true);
  expect([text(), cursor()]).toEqual(["слово ****", 8]);
  view!.dispatch({ changes: { from: 8, insert: "текст" }, selection: { anchor: 13 } });
  expect(starInput(view!, 13, 13, "*")).toBe(true);
  expect(cursor()).toBe(14);

  open("‸");
  expect(starInput(view!, 0, 0, "*")).toBe(false);
  open("2‸");
  expect(starInput(view!, 1, 1, "*")).toBe(false);
  open("«выбор»");
  expect(starInput(view!, 0, 5, "*")).toBe(true);
  expect([text(), selected()]).toEqual(["*выбор*", "выбор"]);
});

it("Backspace между двумя `*` убирает обе", () => {
  open("a *‸*");
  expect(deleteStarPair(view!)).toBe(true);
  expect(text()).toBe("a ");
  open("a ‸b");
  expect(deleteStarPair(view!)).toBe(false);
});

it("Enter продолжает список и задачу, пустой пункт заканчивает список", () => {
  open("- [x] сделано‸");
  expect(insertNewlineContinueMarkup(view!)).toBe(true);
  expect(text()).toBe("- [x] сделано\n- [ ] ");
  open("1. один‸");
  insertNewlineContinueMarkup(view!);
  expect(text()).toBe("1. один\n2. ");
  open("- ‸");
  insertNewlineContinueMarkup(view!);
  expect(text()).not.toContain("- \n- ");
});
