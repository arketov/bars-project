// Операции с файлами и папками дерева (Т-33): запрос из меню → диалог → команда → перечитанное дерево.
// Устройство — docs/features/knowledge-base/tree-and-files.md.

import { app } from "./app.svelte";
import { entryCreateDir, entryCreateDoc, entryMove, entryPlanDelete, entryPlanMove, entryTrash, type DeletePlan, type EntryResult, type MovePreview } from "./api";
import { findNode, projectPathOf, type TreeNode } from "./tree";
import { moved } from "./moved.svelte";

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
  preview = $state<MovePreview | null>(null);
  previewValue = "";
  report = $state<{path: string; reason: string}[] | null>(null);
  /** Подтверждение удаления: `loading` — идёт подсчёт входящих ссылок, `unchecked` — индекс ссылок недоступен. */
  deleteState = $state<"loading" | "ready" | "unchecked">("loading");
  deletePlan = $state<DeletePlan | null>(null);
  /** Номер подсчёта: ответ закрытого или переоткрытого диалога его не перезаписывает. */
  private planSeq = 0;

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
    this.preview = null; this.report = null;
    this.deletePlan = null; this.deleteState = "loading";
    const request: EntryRequest = { action, project, docsRoot: app.docsRoot, node, name: nodeName(node), docs: countDocs(node) };
    this.request = request;
    if (action === "trash") void this.planDelete(request, ++this.planSeq);
  }

  /** Число входящих ссылок считается один раз, при открытии диалога (tree-and-files.md, «Удаление»). */
  private async planDelete(request: EntryRequest, seq: number): Promise<void> {
    let plan: DeletePlan | null = null;
    try {
      const answer = await entryPlanDelete(request.project, projectPathOf(request.node.path, request.docsRoot));
      if (answer.status === "ok") plan = answer;
    } catch {
      // Подсчёт не удался: диалог скажет «Ссылки не проверены», удаление остаётся возможным.
    }
    if (this.planSeq !== seq) return;
    this.deletePlan = plan;
    this.deleteState = plan?.links_checked ? "ready" : "unchecked";
  }

  cancel(): void {
    if (this.busy) return;
    this.request = null;
    this.error = null;
    this.preview = null; this.report = null;
    this.deletePlan = null;
    this.planSeq++;
  }

  /** Выполняет запрос; `value` — введённое имя (создание, переименование) или путь папки назначения (перемещение). */
  async submit(value: string): Promise<void> {
    const request = this.request;
    if (!request || this.busy) return;
    if (this.report) { this.cancel(); return; }
    if (request.action === "trash" && this.deleteState === "loading") return;
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
        if (stuck.some((buffer) => buffer.project === project && (request.action !== "trash" || inside(buffer.path, at(node.path), node.kind === "file")))) {
          this.error = "В этом документе есть несохранённые правки, которые не удалось записать. Решите конфликт во вкладке документа.";
          return;
        }
      }
      if (app.current !== project || app.docsRoot !== request.docsRoot) {
        this.error = "Проект изменился. Закройте диалог и откройте его заново.";
        return;
      }
      if (request.action === "rename" || request.action === "move") {
        const parent = at(request.action === "rename" ? parentNodePath(node) : value);
        const name = request.action === "rename" ? value.trim() : request.name;
        if (!this.preview || this.previewValue !== value) {
          const preview = await entryPlanMove(project, at(node.path), parent, name);
          if (preview.status === "error") { this.error = preview.message; return; }
          this.preview = preview; this.previewValue = value;
          return;
        }
      }
      let result: EntryResult;
      if (request.action === "newDoc") result = await entryCreateDoc(project, at(node.path), value.trim());
      else if (request.action === "newDir") result = await entryCreateDir(project, at(node.path), value.trim());
      else if (request.action === "rename") result = await entryMove(project, at(node.path), at(parentNodePath(node)), value.trim(), this.preview!.token);
      else if (request.action === "move") result = await entryMove(project, at(node.path), at(value), request.name, this.preview!.token);
      else result = await entryTrash(project, at(node.path));
      if (result.status === "error") {
        this.preview = null;
        this.error = result.message;
        return;
      }
      if (app.current !== project) {
        this.request = null;
        return;
      }
      this.report = result.failed?.length ? result.failed : null;
      if (!this.report) this.request = null;
      if (request.action === "trash") moved.trashed(project, at(node.path));
      if ((request.action === "rename" || request.action === "move") && result.path) {
        await moved.accept({project, from: at(node.path), to: result.path});
      }
      await app.loadTree();
      app.selected = new Set([...app.selected].filter((path) => !!findNode(app.tree, path)));
      if (request.action === "newDoc" && result.status === "ok") {
        app.flash(`Документ создан: ${result.path}`);
        await app.openDocument({ path: result.path, title: result.path, first_line: 0, last_line: 0 });
      } else if (request.action === "newDir" && result.status === "ok") {
        app.flash("Папка создана вместе с index.md");
        await app.openDocument({ path: result.index_path ?? "", title: result.path, first_line: 0, last_line: 0 });
      } else if (result.status === "partial") {
        app.flash(result.failed ? "Перемещено; часть ссылок не обновлена. Список — в диалоге." : `Папка создана, а index.md нет: ${result.message}`, true);
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
