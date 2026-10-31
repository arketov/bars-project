import { app } from './app.svelte';
import { copyText } from './api';
import { cleanTitle, documentTargets, resolveWikilink, wikiName, wikiTarget, type LinkTarget } from './link-picker';
import { resolveDocumentLink } from './markdown';
import { followLink, missingLinkPath } from './follow-link.svelte';

/** Ссылка, на которой открыто меню: адресат как написан, документ со ссылкой, проект и корень на момент открытия. */
export interface LinkCommandTarget {
  project: string;
  docsRoot: string;
  /** Документ, в котором стоит ссылка: от него считаются относительные адреса. */
  source: string;
  raw: string;
  wiki: boolean;
  /** Видимый текст ссылки, нужен для копирования обычной битой ссылки. */
  text?: string;
  /** Цель, уже найденная индексом ссылок (строка панели «Ссылки»): важнее разбора текста ссылки. */
  known?: string;
}

/** Цель ссылки по тем же правилам, что у щелчка; `create` — путь для нового документа, если цели нет. */
export function resolveLink(source: string, raw: string, wiki: boolean, known?: string): { targets: LinkTarget[]; target: LinkTarget | null; create: string | null } {
  const targets = documentTargets(app.tree, app.docsRoot, true);
  // `[[#раздел]]` — ссылка на раздел этого же документа.
  const path = known || (wiki ? (wikiTarget(raw) ? resolveWikilink(targets, source, raw)?.path : source) : resolveDocumentLink(source, raw));
  const target = path ? targets.find(candidate => candidate.path === path) ?? null : null;
  return { targets, target, create: target ? null : missingLinkPath(source, app.docsRoot, raw, wiki) };
}

const fragment = (raw: string, wiki: boolean) => {
  const part = wiki ? raw.split('|')[0].split('#')[1] : raw.split('#')[1];
  if (!part) return '';
  try { return cleanTitle(decodeURIComponent(part)); } catch { return cleanTitle(part); }
};

/** Что кладёт «Скопировать ссылку»: `[[Название]]` цели в виде проекта, а для битой — ссылка как написана в документе. */
export function linkText(link: LinkCommandTarget): string {
  const { targets, target } = resolveLink(link.source, link.raw, link.wiki, link.known);
  if (!target) return link.wiki ? `[[${link.raw}]]` : `[${link.text || link.raw}](${link.raw})`;
  const section = fragment(link.raw, link.wiki);
  return `[[${wikiName(target, targets)}${section ? `#${section}` : ''}]]`;
}

/** Ссылка на заголовок открытого документа: `[[Название документа#Заголовок]]`; документа нет среди файлов проекта — `null`. */
export function headingLinkText(path: string, heading: string): string | null {
  const targets = documentTargets(app.tree, app.docsRoot, true);
  const target = targets.find(candidate => candidate.path === path);
  const section = cleanTitle(heading);
  return target ? `[[${wikiName(target, targets)}${section ? `#${section}` : ''}]]` : null;
}

/** Выполняет пункт меню ссылки; ошибка уходит вызывающему, ложного успеха нет. */
export async function executeLinkCommand(id: string, link: LinkCommandTarget): Promise<void> {
  if (link.project !== app.current || link.docsRoot !== app.docsRoot) throw new Error('Цель уже изменилась');
  if (id === 'link.copy') { await copyText(linkText(link)); app.flash('Ссылка скопирована'); return; }
  const { target } = resolveLink(link.source, link.raw, link.wiki, link.known);
  if (id === 'link.createTarget') {
    if (target) throw new Error('Документ уже существует');
    await followLink.open(link.project, link.source, link.raw, link.wiki);
    return;
  }
  if (!target) throw new Error('Документа по ссылке нет среди файлов проекта');
  if (id === 'document.newWindow') { await app.openInNewWindow(target.path); return; }
  if (id === 'target.open') { await app.openDocument({ path: target.path, title: target.title, first_line: 0, last_line: 0 }); return; }
  throw new Error(`Команда ${id} не относится к ссылке`);
}
