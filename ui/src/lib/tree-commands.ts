import { app } from './app.svelte';
import { entries, type EntryAction } from './entries.svelte';
import { copyText, documentAction, folderAction, folderTarget, type FolderTarget } from './api';
import { findNode } from './tree';
import { canStyleFolder, keyOf } from './appearance';
import { treeMenu } from './menu-model';
import { commandAdapter } from './command-adapters';

export function treeCommandAdapter(element: HTMLElement, path: string) {
  const target: TreeCommandTarget = { element, path, project: app.current!, docsRoot: app.docsRoot };
  const node = findNode(app.tree, path)!;
  const actions = treeMenu({ kind: node.kind, path, state: 'ready', canStyle: true }).map(item => [item.id, {
    reason: () => treeCommandReason(item.id, target), run: () => executeTreeCommand(item.id, target),
    identity: () => `${app.current}:${app.docsRoot}:${path}`,
  }]);
  return commandAdapter(element, Object.fromEntries(actions));
}

export interface TreeCommandTarget {
  project: string;
  docsRoot: string;
  path: string;
  element: HTMLElement;
  pinned?: FolderTarget;
}
/** Проверка цели и доступности общая для меню, клавиш и списка действий. */
export function treeCommandReason(id: string, target: TreeCommandTarget): string | undefined {
  if (target.project !== app.current || target.docsRoot !== app.docsRoot || !target.element.isConnected) return 'Цель уже изменилась';
  const node = findNode(app.tree, target.path);
  if (!node) return 'Узел уже удалён';
  const item = treeMenu({ kind: node.kind, path: node.path, state: app.project?.state ?? 'starting',
    workerDown: !!app.workerDown, missing: target.pinned?.missing,
    canStyle: node.kind === 'file' || canStyleFolder(keyOf(node.path)) }).find(item => item.id === id);
  return !item ? 'Действие не подходит этому объекту' : item.disabled ? item.hint : undefined;
}
export async function executeTreeCommand(id: string, target: TreeCommandTarget, editor?: string): Promise<void> {
  const reason = treeCommandReason(id, target);
  if (reason) throw new Error(reason);
  const node = findNode(app.tree, target.path)!;
  const actions: Record<string, EntryAction> = { 'file.create': 'newDoc', 'folder.create': 'newDir', 'target.rename': 'rename', 'target.move': 'move', 'target.delete': 'trash' };
  if (actions[id]) { entries.open(actions[id], node.path); return; }
  if (id === 'target.style') {
    target.element.dispatchEvent(new CustomEvent('bars-tree-style', { bubbles: true, detail: target })); return;
  }
  if (id === 'target.openExternal' && !editor) {
    target.element.dispatchEvent(new CustomEvent('bars-tree-editors', { bubbles: true, detail: target })); return;
  }
  if (id === 'target.open' || id === 'document.newWindow') {
    if (id === 'document.newWindow') await app.openInNewWindow(node.documentPath!);
    else await app.openDocument({ path: node.documentPath!, title: node.label, first_line: 0, last_line: 0 });
    return;
  }
  const action = id === 'target.reveal' ? 'reveal' : id === 'target.copyPath' ? 'path' : id === 'target.checkIndex' ? 'check' : 'open';
  if (node.kind === 'file') {
    const result = await documentAction(target.project, node.documentPath!, action, action === 'open' ? editor : undefined);
    if (action === 'path') { await copyText(result); app.flash('Путь скопирован'); }
    if (action === 'check') app.flash('Проверка документа поставлена в очередь');
  } else {
    const pinned = target.pinned ?? await folderTarget(target.project, target.docsRoot, node.path);
    // Пока закреплялась цель, пользователь мог сменить проект или корень.
    const changed = treeCommandReason(id, target); if (changed) throw new Error(changed);
    const result = await folderAction(pinned.id, action === 'reveal' ? 'open' : action as 'path' | 'check');
    if (action === 'path') { await copyText(result.absolute); app.flash(result.missing ? 'Путь скопирован; папка отсутствует' : 'Путь скопирован'); }
    if (action === 'check') app.flash(`Проверка проекта в очереди · №${result.operation_id}`);
  }
}
