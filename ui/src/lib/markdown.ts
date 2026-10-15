// Отрисовка документа в окне просмотра: Markdown → HTML с подсветкой найденного раздела.
// Сырой HTML из документа не пропускается: файл — чужой текст, а окно имеет доступ к воркеру.

import { Marked, type Token, type Tokens } from "marked";

const marked = new Marked({ gfm: true, breaks: false });
marked.use({
  renderer: {
    // Сырой HTML показываем текстом.
    html(token: Tokens.HTML | Tokens.Tag) {
      return escapeHtml(token.text);
    },
    // Ссылки не открываются в окне: путь виден, переход невозможен.
    link(token: Tokens.Link) {
      return `<span class="a">${this.parser.parseInline(token.tokens)}</span>`;
    },
    image(token: Tokens.Image) {
      return `<span class="a">${escapeHtml(token.text || token.href)}</span>`;
    },
  },
});

export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/** Убирает frontmatter и возвращает, сколько строк он занял (номера строк документа не сдвигаются). */
export function stripFrontmatter(text: string): { body: string; lines: number } {
  const match = /^---\r?\n[\s\S]*?\r?\n---[ \t]*(\r?\n|$)/.exec(text);
  if (!match) return { body: text, lines: 0 };
  return { body: text.slice(match[0].length), lines: match[0].split("\n").length - 1 };
}

const countLines = (raw: string) => (raw.match(/\n/g) ?? []).length;

export interface Highlight {
  first: number;
  last: number;
}

/**
 * Рисует документ блоками верхнего уровня. Блоки, чьи строки пересекаются с `[first, last]`,
 * собираются в один элемент `section.hit` с подписью «строки N–M · найдено».
 */
export function renderDocument(text: string, highlight?: Highlight): string {
  const { body, lines: skipped } = stripFrontmatter(text);
  const tokens = marked.lexer(body);
  let line = skipped + 1;
  const parts: string[] = [];
  let open = false;
  for (const token of tokens as Token[]) {
    const start = line;
    const span = countLines(token.raw);
    const end = token.type === "space" ? start : Math.max(start, start + span - (token.raw.endsWith("\n") ? 1 : 0));
    line += span;
    if (token.type === "space") continue;
    const hit = !!highlight && start <= highlight.last && end >= highlight.first;
    if (hit && !open) {
      parts.push(
        `<section class="hit" id="hit"><span class="hitlabel">строки ${highlight.first}–${highlight.last} · найдено</span>`,
      );
      open = true;
    } else if (!hit && open) {
      parts.push("</section>");
      open = false;
    }
    parts.push(marked.parser([token] as Token[]));
  }
  if (open) parts.push("</section>");
  return parts.join("\n");
}

/** Текст чанка в карточке: тот же Markdown, без подсветки. */
export function renderChunk(text: string): string {
  return marked.parser(marked.lexer(text) as Token[]);
}

/** Краткий текст без разметки для двух строк под заголовком карточки. */
export function plainText(markdown: string): string {
  return markdown
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/^#{1,6}\s+/gm, "")
    .replace(/[*_`>|]/g, "")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/^\s*[-+]\s+/gm, "")
    .replace(/\s+/g, " ")
    .trim();
}
