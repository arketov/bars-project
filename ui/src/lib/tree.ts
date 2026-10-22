// Дерево «модуль → папки → файлы» с галочками (Т-14, main-window.md).
// Пути узлов считаются от корня документации; корневой модуль называется `.`, его файлы — `./имя`.

import { humanName, moduleLabel } from "./format";
import type { TreeDocument } from "./types";

export interface TreeNode {
  /** Путь от корня документации; у корневого модуля — `.`. */
  path: string;
  /** Имя как на диске. */
  name: string;
  /** Имя для людей; у модуля — «(корень)» для `.`. */
  label: string;
  kind: "module" | "dir" | "file";
  chunks: number;
  children: TreeNode[];
  /** Исходный путь индекса от корня проекта; только у файла. */
  documentPath?: string;
  /** Заголовок документа из индекса; только у файла. */
  title?: string;
  /** Исходный путь `Index.md` папки или модуля (Т-32); клик по имени папки открывает его. */
  indexPath?: string;
}

export type CheckState = "all" | "some" | "none";

export interface SearchScope {
  modules: string[];
  /** Файлы от корня документации. */
  files: string[];
}

/** Путь документа относительно корня документации. */
export function relativeToDocs(path: string, docsRoot: string): string {
  const root = docsRoot.replace(/\\/g, "/").replace(/^\.\/?/, "").replace(/\/+$/, "");
  const clean = path.replace(/\\/g, "/");
  return root && clean.startsWith(root + "/") ? clean.slice(root.length + 1) : clean;
}

export function buildTree(documents: TreeDocument[], docsRoot: string): TreeNode[] {
  const modules = new Map<string, TreeNode>();
  const ensureModule = (name: string): TreeNode => {
    let node = modules.get(name);
    if (!node) {
      node = { path: name, name, label: moduleLabel(name), kind: "module", chunks: 0, children: [] };
      modules.set(name, node);
    }
    return node;
  };
  for (const document of documents) {
    const relative = relativeToDocs(document.path, docsRoot);
    const parts = relative.split("/");
    const module = parts.length > 1 ? parts[0] : ".";
    let node = ensureModule(module);
    node.chunks += document.chunks;
    const rest = module === "." ? parts : parts.slice(1);
    // Файлы корня получают путь `./имя`: так корневой модуль `.` не путается с модулем-папкой.
    let path = module;
    rest.forEach((part, index) => {
      path = `${path}/${part}`;
      const isFile = index === rest.length - 1;
      let child = node.children.find((candidate) => candidate.path === path);
      if (!child) {
        child = {
          path,
          name: part,
          label: humanName(part),
          kind: isFile ? "file" : "dir",
          chunks: 0,
          children: [],
          ...(isFile ? { documentPath: document.path, title: document.title } : {}),
        };
        node.children.push(child);
      }
      child.chunks += document.chunks;
      node = child;
    });
  }
  const sortNodes = (nodes: TreeNode[]) => {
    // Папки раньше файлов, внутри — по имени; `Index.md` первым среди файлов.
    nodes.sort((a, b) => {
      if (a.kind !== b.kind) return a.kind === "dir" ? -1 : 1;
      const index = (n: TreeNode) => (n.name.toLowerCase() === "index.md" ? 0 : 1);
      return index(a) - index(b) || a.name.localeCompare(b.name, "ru");
    });
    nodes.forEach((node) => sortNodes(node.children));
  };
  const attachIndex = (node: TreeNode) => {
    const index = node.children.find((child) => child.kind === "file" && child.name.toLowerCase() === "index.md");
    if (index?.documentPath) node.indexPath = index.documentPath;
    node.children.forEach(attachIndex);
  };
  modules.forEach(attachIndex);
  const result = [...modules.values()];
  result.forEach((module) => sortNodes(module.children));
  // Корневой модуль первым, остальные по имени.
  result.sort((a, b) => (a.path === "." ? -1 : b.path === "." ? 1 : a.path.localeCompare(b.path, "ru")));
  return result;
}

/** Покрывает ли отмеченный путь `selected` путь `target`. */
export function covers(selected: string, target: string): boolean {
  return selected === target || target.startsWith(selected + "/");
}

export function leaves(node: TreeNode): string[] {
  return node.kind === "file" ? [node.path] : node.children.flatMap(leaves);
}

const isCovered = (sel: ReadonlySet<string>, path: string) => [...sel].some((s) => covers(s, path));

export function checkState(node: TreeNode, sel: ReadonlySet<string>): CheckState {
  if (isCovered(sel, node.path)) return "all";
  return leaves(node).some((leaf) => isCovered(sel, leaf)) ? "some" : "none";
}

/** Ставит или снимает галочку; снятие внутри отмеченного предка отмечает соседей (main-window.md). */
export function toggle(
  tree: TreeNode[],
  sel: ReadonlySet<string>,
  target: string,
  checked: boolean,
): Set<string> {
  const next = new Set(sel);
  if (checked) {
    for (const s of [...next]) if (s !== target && covers(target, s)) next.delete(s);
    next.add(target);
    return next;
  }
  const ancestor = [...next].find((s) => covers(s, target));
  for (const s of [...next]) if (covers(target, s)) next.delete(s);
  if (ancestor && ancestor !== target) {
    next.delete(ancestor);
    const start = findNode(tree, ancestor);
    if (start) expandAround(start, target, next);
  }
  return next;
}

function expandAround(node: TreeNode, target: string, into: Set<string>): void {
  for (const child of node.children) {
    if (child.path === target) continue;
    if (child.path !== target && covers(child.path, target)) expandAround(child, target, into);
    else into.add(child.path);
  }
}

export function findNode(tree: TreeNode[], path: string): TreeNode | undefined {
  for (const node of tree) {
    if (node.path === path) return node;
    const inner = findNode(node.children, path);
    if (inner) return inner;
  }
  return undefined;
}

/** Модули, затронутые выбором, для строки «Искать в: …» и подписи «Отмечено модулей: N из M». */
export function touchedModules(tree: TreeNode[], sel: ReadonlySet<string>): TreeNode[] {
  return tree.filter((module) => checkState(module, sel) !== "none");
}

/** Параметры поиска: целиком отмеченные модули — в `modules`, остальное — файлами. */
export function searchScope(tree: TreeNode[], sel: ReadonlySet<string>): SearchScope {
  const scope: SearchScope = { modules: [], files: [] };
  for (const module of tree) {
    const state = checkState(module, sel);
    if (state === "all") scope.modules.push(module.path);
    else if (state === "some") {
      const picked = leaves(module).filter((leaf) => isCovered(sel, leaf));
      scope.files.push(...picked.map((leaf) => leaf.replace(/^\.\//, "")));
    }
  }
  return scope;
}

/** Фильтр по показанному имени и по пути; совпавшая папка показывается целиком. */
export function filterTree(tree: TreeNode[], text: string): TreeNode[] {
  const needle = text.trim().toLowerCase();
  if (!needle) return tree;
  const matches = (node: TreeNode) =>
    node.label.toLowerCase().includes(needle) || node.path.toLowerCase().includes(needle);
  const walk = (node: TreeNode): TreeNode | null => {
    if (node.kind === "file") return matches(node) ? node : null;
    if (matches(node)) return node;
    const children = node.children.map(walk).filter((child): child is TreeNode => child !== null);
    return children.length ? { ...node, children } : null;
  };
  return tree.map(walk).filter((node): node is TreeNode => node !== null);
}
