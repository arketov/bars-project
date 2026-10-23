// Ссылки на документы как в Obsidian (Т-30): `[[Название]]`, при одинаковых именах — `[[папка/Название]]`.
// Выбор по `[[` предлагает существующие документы и ссылку на ещё не созданный по названию.

import { relativeToDocs, type TreeNode } from "./tree";

export interface LinkTarget {
  /** Путь документа от корня проекта, как в индексе. */
  path: string;
  /** Путь от корня документации. */
  docs: string;
  title: string;
}

export interface LinkOption {
  label: string;
  detail: string;
  /** Готовый текст ссылки. */
  insert: string;
  /** Пункт «создать ссылку»: файла пока нет. */
  create: boolean;
}

/** Плоский список документов дерева; `index.md` папок входит в список только по `withIndex`. */
export function documentTargets(nodes: TreeNode[], docsRoot: string, withIndex = false): LinkTarget[] {
  const out: LinkTarget[] = [];
  const walk = (node: TreeNode) => {
    if (node.kind === "file" && node.documentPath && (withIndex || node.name.toLowerCase() !== "index.md")) {
      out.push({ path: node.documentPath, docs: relativeToDocs(node.documentPath, docsRoot), title: node.title || node.label });
    }
    node.children.forEach(walk);
  };
  nodes.forEach(walk);
  return out;
}

/** Документы для выбора по `[[`: `Index.md` папок в выбор не входит (ссылки на него не ставят). */
export function linkTargets(nodes: TreeNode[], docsRoot: string): LinkTarget[] {
  return documentTargets(nodes, docsRoot);
}

const withoutMd = (path: string) => path.replace(/\.md$/i, "");
const stemOf = (path: string) => withoutMd(path.split("/").pop() ?? path);
const norm = (text: string) => text.toLowerCase().replace(/ё/g, "е");
const dirOf = (path: string) => path.split("/").slice(0, -1).join("/");

/** Как документ называется в `[[…]]`: имя файла без `.md`; путь от корня документации, если такое имя есть у другого файла. */
export function wikiName(target: LinkTarget, all: LinkTarget[]): string {
  const stem = stemOf(target.docs);
  const clash = all.some((other) => other.docs !== target.docs && stemOf(other.docs).toLowerCase() === stem.toLowerCase());
  return clash ? withoutMd(target.docs) : stem;
}

/** Имя из `Название`, `Название|текст`, `папка/Название#раздел`; `null` — ссылка на раздел этого же документа. */
export function wikiTarget(inner: string): string | null {
  const name = (inner.split("|")[0]?.split("#")[0] ?? "").trim();
  const clean = withoutMd(name).trim();
  return clean || null;
}

/** Документ по имени из `[[…]]`; те же правила, что в графе (`bars-graph`): регистр не важен, `/` — путь от корня документации. */
export function resolveWikilink(all: LinkTarget[], from: string, inner: string): LinkTarget | null {
  const name = wikiTarget(inner);
  if (!name) return null;
  const wanted = name.replace(/\\/g, "/").toLowerCase().replace(/^\/+|\/+$/g, "");
  const here = dirOf(from);
  const hits = all.filter((target) => {
    const stem = withoutMd(target.docs).toLowerCase();
    return wanted.includes("/") ? stem === wanted || stem.endsWith(`/${wanted}`) : stem.split("/").pop() === wanted;
  });
  hits.sort((a, b) => Number(dirOf(a.path) !== here) - Number(dirOf(b.path) !== here) || a.docs.length - b.docs.length || a.docs.localeCompare(b.docs));
  return hits[0] ?? null;
}

/** В `[[…]]` нельзя `[ ] | # ^` и перевод строки: из введённого названия они убираются. */
export function cleanTitle(title: string): string {
  return title.replace(/[[\]|#^\n\r]+/g, " ").replace(/\s+/g, " ").trim();
}

/** Пункты из поиска по смыслу: идут после совпадений по названию, документы из первой группы не повторяются. */
export function semanticOptions(targets: LinkTarget[], found: LinkTarget[], taken: LinkOption[]): LinkOption[] {
  const used = new Set(taken.map((option) => option.insert));
  const out: LinkOption[] = [];
  for (const target of found) {
    const insert = `[[${wikiName(target, targets)}]]`;
    if (used.has(insert)) continue;
    used.add(insert);
    out.push({ label: target.title, detail: `${target.docs} · по смыслу`, insert, create: false });
  }
  return out;
}

/** Пункты выбора по набранному после `[[`: существующие документы, затем «создать ссылку», если точного совпадения нет. */
export function linkOptions(targets: LinkTarget[], query: string, limit = 30): LinkOption[] {
  const q = norm(query.trim());
  const scored = targets
    .map((target) => {
      const title = norm(target.title), docs = norm(target.docs), stem = norm(stemOf(target.docs));
      const rank = !q ? 3 : title === q || stem === q ? 0 : title.startsWith(q) || stem.startsWith(q) ? 1 : title.includes(q) || stem.includes(q) ? 2 : docs.includes(q) ? 3 : -1;
      return { target, rank };
    })
    .filter((entry) => entry.rank >= 0)
    .sort((a, b) => a.rank - b.rank || a.target.title.localeCompare(b.target.title, "ru"))
    .slice(0, limit);
  const options: LinkOption[] = scored.map(({ target }) => ({
    label: target.title,
    detail: target.docs,
    insert: `[[${wikiName(target, targets)}]]`,
    create: false,
  }));
  const typed = cleanTitle(query);
  const exact = targets.some((t) => t.title.toLowerCase() === typed.toLowerCase() || stemOf(t.docs).toLowerCase() === typed.toLowerCase());
  if (typed && !exact) {
    options.push({ label: `Создать ссылку на «${typed}»`, detail: "файла пока нет", insert: `[[${typed}]]`, create: true });
  }
  return options;
}
