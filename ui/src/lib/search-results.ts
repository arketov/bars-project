// Выдача окна: сборка карточек документов из плоского списка разделов, разбор операторов для поля запроса
// и подсветка слов (docs/features/window/global-search-design.md, «Проект выдачи: один список»).
import type { SearchHit, UnderstoodQuery } from "./types";

/** Разделов, видимых в карточке без «Ещё N разделов». */
export const VISIBLE_SECTIONS = 2;

export interface DocumentCard {
  path: string;
  title: string;
  folder: string;
  module: string;
  /** Разделы в порядке выдачи воркера: первый — лучший. */
  sections: SearchHit[];
}

/** Карточка документа — группа разделов с одним `path`; порядок карточек — по лучшему разделу. */
export function groupHits(hits: SearchHit[]): DocumentCard[] {
  const cards = new Map<string, DocumentCard>();
  for (const hit of hits) {
    const card = cards.get(hit.path);
    if (card) card.sections.push(hit);
    else cards.set(hit.path, { path: hit.path, title: hit.title, folder: folderOf(hit.path), module: hit.module, sections: [hit] });
  }
  return [...cards.values()];
}

/** Папка документа: путь без имени файла. */
export function folderOf(path: string): string {
  const cut = path.lastIndexOf("/");
  return cut < 0 ? "" : path.slice(0, cut);
}

/** Цепочка заголовков раздела; первый элемент, совпадающий с названием документа, не повторяется. */
export function sectionChain(hit: Pick<SearchHit, "heading_chain" | "title">): string[] {
  const parts = hit.heading_chain.split("/").filter(Boolean);
  return parts.length > 1 && parts[0] === hit.title ? parts.slice(1) : parts;
}

export const sectionsWord = (n: number) => {
  const m = n % 100, k = n % 10;
  return m > 10 && m < 15 ? "разделов" : k === 1 ? "раздел" : k > 1 && k < 5 ? "раздела" : "разделов";
};

// --- операторы запроса ---

export type TokenKind = "word" | "phrase" | "exclude" | "path";
export interface QueryToken { kind: TokenKind; text: string; start: number; end: number; value: string }

/**
 * Разбор для раскраски поля и чипов; правила — как в воркере (hybrid-search.md, «Схема баллов: операторы запроса»):
 * `"фраза"`, `-слово`, `-"фраза"`, `path:`/`папка:`; `-5`, `по-русски` и одиночная кавычка — обычные символы.
 * Итог поиска и подсветку ведёт ответ воркера (`understood`), это только первая подсказка до ответа.
 */
export function parseQuery(query: string): QueryToken[] {
  const re = /(-?"[^"]+")|(-\p{L}[^\s"]*)|((?:path|папка):(?:"[^"]+"|\S+))|(\S+)/giu;
  const tokens: QueryToken[] = [];
  for (const m of query.matchAll(re)) {
    const text = m[0], start = m.index ?? 0;
    const end = start + text.length;
    if (m[1]) {
      const exclude = text.startsWith("-");
      const value = text.slice(exclude ? 2 : 1, -1);
      if (/[\p{L}\p{N}]/u.test(value)) tokens.push({ kind: exclude ? "exclude" : "phrase", text, start, end, value });
      else tokens.push({ kind: "word", text, start, end, value: text });
    } else if (m[2]) tokens.push({ kind: "exclude", text, start, end, value: text.slice(1) });
    else if (m[3]) {
      const value = text.replace(/^(?:path|папка):/iu, "").replace(/^"|"$/g, "");
      if (value) tokens.push({ kind: "path", text, start, end, value });
      else tokens.push({ kind: "word", text, start, end, value: text });
    } else tokens.push({ kind: "word", text, start, end, value: text });
  }
  return tokens;
}

/** Запрос без оператора в позиции `start`: чип «×». */
export function dropToken(query: string, token: QueryToken): string {
  return (query.slice(0, token.start) + query.slice(token.end)).replace(/\s+/g, " ").trim();
}

/** Запрос без всех операторов вида `kinds` («Без кавычек», «Без исключения»). */
export function withoutOperators(query: string, kinds: TokenKind[]): string {
  const drop = parseQuery(query).filter((t) => kinds.includes(t.kind));
  let result = query;
  for (const token of drop.reverse()) result = result.slice(0, token.start) + (token.kind === "phrase" ? token.value : "") + result.slice(token.end);
  return result.replace(/\s+/g, " ").trim();
}

export const hasOperators = (query: string, kind: TokenKind) => parseQuery(query).some((t) => t.kind === kind);

// --- подсветка ---

export interface Marker { words: string[]; phrases: string[] }

/** Что подсвечивать: слова и фразы запроса без исключённых. Ответ воркера важнее клиентского разбора. */
export function markerFor(query: string, understood?: UnderstoodQuery | null): Marker {
  if (understood) {
    return { words: wordsOf(understood.text ?? ""), phrases: understood.phrases ?? [] };
  }
  const tokens = parseQuery(query);
  return {
    words: tokens.filter((t) => t.kind === "word").flatMap((t) => wordsOf(t.text)),
    phrases: tokens.filter((t) => t.kind === "phrase").map((t) => t.value),
  };
}

const wordsOf = (text: string) => [...new Set(text.toLowerCase().match(/[\p{L}\p{N}_]+/gu) ?? [])].filter((w) => w.length > 1);
/** Основа для сравнения без стемминга воркера: у длинного слова отбрасываются две последние буквы. */
const stemOf = (word: string) => (word.length > 5 ? word.slice(0, word.length - 2) : word);

export interface Piece { text: string; mark: boolean }

/** Разбивает текст на куски с отметкой «подсвечено»: слово — по основе, фраза — дословно (регистр не важен). */
export function highlight(text: string, marker: Marker): Piece[] {
  const spans: [number, number][] = [];
  const lower = text.toLowerCase();
  for (const phrase of marker.phrases) {
    const needle = phrase.toLowerCase();
    if (!needle) continue;
    for (let at = lower.indexOf(needle); at >= 0; at = lower.indexOf(needle, at + needle.length)) spans.push([at, at + needle.length]);
  }
  const stems = marker.words.map(stemOf);
  if (stems.length) {
    for (const m of lower.matchAll(/[\p{L}\p{N}_]+/gu)) {
      const word = m[0], at = m.index ?? 0;
      if (stems.some((stem) => word.startsWith(stem))) spans.push([at, at + word.length]);
    }
  }
  if (!spans.length) return [{ text, mark: false }];
  spans.sort((a, b) => a[0] - b[0] || b[1] - a[1]);
  const merged: [number, number][] = [];
  for (const span of spans) {
    const last = merged[merged.length - 1];
    if (last && span[0] <= last[1]) last[1] = Math.max(last[1], span[1]);
    else merged.push([...span]);
  }
  const pieces: Piece[] = [];
  let pos = 0;
  for (const [from, to] of merged) {
    if (from > pos) pieces.push({ text: text.slice(pos, from), mark: false });
    pieces.push({ text: text.slice(from, to), mark: true });
    pos = to;
  }
  if (pos < text.length) pieces.push({ text: text.slice(pos), mark: false });
  return pieces;
}

/** Подсветка слов запроса в уже отрисованном разделе документа («Открыть раздел»): оборачивает текстовые узлы в `<mark>`. */
export function markTerms(root: Element, marker: Marker): void {
  if (!marker.words.length && !marker.phrases.length) return;
  const walker = root.ownerDocument.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const nodes: Text[] = [];
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (!(node.parentElement?.closest("mark"))) nodes.push(node as Text);
  }
  for (const node of nodes) {
    const pieces = highlight(node.data, marker);
    if (pieces.length === 1 && !pieces[0].mark) continue;
    const fragment = root.ownerDocument.createDocumentFragment();
    for (const piece of pieces) {
      if (piece.mark) {
        const mark = root.ownerDocument.createElement("mark");
        mark.textContent = piece.text;
        fragment.append(mark);
      } else fragment.append(piece.text);
    }
    node.replaceWith(fragment);
  }
}
