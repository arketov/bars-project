// Операции с файлами и папками дерева (Т-33): запрос из меню → диалог → команда → перечитанное дерево.
// Устройство — docs/features/knowledge-base/tree-and-files.md.

import { app } from "./app.svelte";
import { entryCreateDir, entryCreateDoc, entryMove, entryTrash, type EntryResult } from "./api";
import { findNode, projectPathOf, type TreeNode } from "./tree";

export type EntryAction = "newDoc" | "newDir" | "rename" | "move" | "trash";

export interface EntryRequest {
  action: EntryAction;
  project: string;
  docsRoot: string;
  /** Узел дерева: в нём создаётся новое, его переименовывают, переносят или удаляют. */
  node: TreeNode;
  /** Текущее имя на диске; у корня документации пусто. */
  name: string;
  /** Сколько документов внутри узла (для файла — 1). */
  docs: number;
}

/** Папки, куда можно перенести узел: корень документации и все папки дерева, кроме самого узла и его потомков. */
export interface Destination {
  /** Путь узла дерева; корень документации — `.`. */
  path: string;
  label: string;
  depth: number;
}

export function isDirectory(node: TreeNode): boolean {
  return node.kind !== "file";
}

function countDocs(node: TreeNode): number {
  return node.kind === "file" ? 1 : node.children.reduce((sum, child) => sum + countDocs(child), 0);
}

/** Имя папки или файла на диске из пути узла. */
export function nodeName(node: TreeNode): string {
  return node.path === "." ? "" : node.name;
}

/** Путь узла дерева без последнего звена: `a/b/x.md` → `a/b`; модуль и файл корня → `.`. */
export function parentNodePath(node: TreeNode): string {
  const trimmed = node.path.replace(/^\.\//, "");
  const slash = trimmed.lastIndexOf("/");
  return slash < 0 ? "." : trimmed.slice(0, slash);
}

export function destinations(tree: TreeNode[], moving: TreeNode): Destination[] {
  const list: Destination[] = [{ path: ".", label: "Корень документации", depth: 0 }];
  const walk = (nodes: TreeNode[], depth: number) => {
    for (const node of nodes) {
      if (node.kind === "file" || node.path === ".") continue;
      if (isDirectory(moving) && (node.path === moving.path || node.path.startsWith(moving.path + "/"))) continue;
      list.push({ path: node.path, label: node.name, depth });
      walk(node.children, depth + 1);
    }
  };
  walk(tree, 1);
  return list;
}

class Entries {
  request = $state<EntryRequest | null>(null);
  busy = $state(false);
  error = $state<string | null>(null);

  /** Открывает диалог для узла дерева; `nodePath` — путь узла (`.` — корень документации). */
  open(action: EntryAction, nodePath: string): void {
    const project = app.current;
    if (!project || this.busy) return;
    const node: TreeNode | undefined = nodePath === "."
      ? app.tree.find((module) => module.path === ".") ?? { path: ".", name: "", label: "Корень документации", kind: "module", chunks: 0, children: [] }
      : findNode(app.tree, nodePath);
    if (!node) return;
    if (node.path === "." && action !== "newDoc" && action !== "newDir") return;
    this.error = null;
    this.request = { action, project, docsRoot: app.docsRoot, node, name: nodeName(node), docs: countDocs(node) };
  }

  cancel(): void {
    if (this.busy) return;
    this.request = null;
    this.error = null;
  }

  /** Выполняет запрос; `value` — введённое имя (создание, переименование) или путь папки назначения (перемещение). */
  async submit(value: string): Promise<void> {
    const request = this.request;
    if (!request || this.busy) return;
    const { project, node } = request;
    if (app.current !== project || app.docsRoot !== request.docsRoot) {
      this.error = "Проект изменился. Закройте диалог и откройте его заново.";
      return;
    }
    const at = (nodePath: string) => projectPathOf(nodePath, request.docsRoot);
    this.busy = true;
    this.error = null;
    try {
      if (request.action === "rename" || request.action === "move" || request.action === "trash") {
        // Диск становится истиной: несохранённые правки уходят на диск до переименования и удаления.
        const { editors } = await import("./editors.svelte");
        const stuck = await editors.flush();
        if (stuck.some((buffer) => buffer.project === project && inside(buffer.path, at(node.path), node.kind === "file"))) {
          this.error = "В этом документе есть несохранённые правки, которые не удалось записать. Решите конфликт во вкладке документа.";
          return;
        }
      }
      if (app.current !== project || app.docsRoot !== request.docsRoot) {
        this.error = "Проект изменился. Закройте диалог и откройте его заново.";
        return;
      }
      let result: EntryResult;
      if (request.action === "newDoc") result = await entryCreateDoc(project, at(node.path), value.trim());
      else if (request.action === "newDir") result = await entryCreateDir(project, at(node.path), value.trim());
      else if (request.action === "rename") result = await entryMove(project, at(node.path), at(parentNodePath(node)), value.trim());
      else if (request.action === "move") result = await entryMove(project, at(node.path), at(value), request.name);
      else result = await entryTrash(project, at(node.path));
      if (result.status === "error") {
        this.error = result.message;
        return;
      }
      if (app.current !== project) {
        this.request = null;
        return;
      }
      this.request = null;
      await app.loadTree();
      app.selected = new Set([...app.selected].filter((path) => !!findNode(app.tree, path)));
      if (request.action === "newDoc" && result.status === "ok") {
        app.flash(`Документ создан: ${result.path}`);
        await app.openDocument({ path: result.path, title: result.path, first_line: 0, last_line: 0 });
      } else if (request.action === "newDir" && result.status === "ok") {
        app.flash("Папка создана вместе с index.md");
        await app.openDocument({ path: result.index_path ?? "", title: result.path, first_line: 0, last_line: 0 });
      } else if (result.status === "partial") {
        app.flash(`Папка создана, а index.md нет: ${result.message}`, true);
      } else if (request.action === "trash") {
        app.flash("Перемещено в корзину");
      } else {
        app.flash(request.action === "rename" ? "Переименовано" : "Перемещено");
      }
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error);
    } finally {
      this.busy = false;
    }
  }
}

/** Лежит ли `path` внутри узла `node` (для файла — это он сам). */
function inside(path: string, node: string, file: boolean): boolean {
  return file ? path === node : path === node || path.startsWith(node + "/");
}

export const entries = new Entries();
