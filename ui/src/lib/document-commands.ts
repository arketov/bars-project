import { app } from './app.svelte';
import { entries, type EntryAction } from './entries.svelte';
import { copyText, documentAction } from './api';
import { findDocumentNode } from './tree';
import { adapterFor } from './command-adapters';
import { tabHost } from './workspace-tabs';
import type { DocumentPlace } from './menu-model';

/** Документ, на котором открыто меню: проект и корень фиксируются при открытии и проверяются перед командой. */
export interface DocumentCommandTarget {
  project: string;
  docsRoot: string;
  path: string;
  line: number;
  place: DocumentPlace;
  panelId?: string;
  element: HTMLElement;
}
const entryActions: Record<string, EntryAction> = { 'target.rename': 'rename', 'target.move': 'move', 'target.delete': 'trash' };

/** Редактор документа внутри цели: на нём зарегистрированы адаптеры правки текста. */
export function editorHostOf(element: HTMLElement): HTMLElement {
  return element.querySelector<HTMLElement>('.editor') ?? element;
}

/** Выполняет пункт меню документа; ошибка уходит вызывающему, ложного успеха нет. */
export async function executeDocumentCommand(id: string, target: DocumentCommandTarget, editor?: string): Promise<void> {
  if (target.project !== app.current || target.docsRoot !== app.docsRoot) throw new Error('Цель уже изменилась');
  if (id.startsWith('text.')) {
    const adapter = adapterFor(id, editorHostOf(target.element));
    if (!adapter) throw new Error('Редактор документа уже закрыт');
    const reason = adapter.reason();
    if (reason) throw new Error(reason);
    await adapter.run();
    return;
  }
  if (id.startsWith('tab.')) {
    const host = tabHost();
    if (!host || !target.panelId) throw new Error('Вкладка уже закрыта');
    await host.run(id, target.panelId);
    return;
  }
  if (entryActions[id]) {
    // Переименование, перенос и удаление идут по узлу дерева: тот же диалог и план ссылок, что в дереве.
    const node = findDocumentNode(app.tree, target.path);
    if (!node) throw new Error('Документа нет в дереве проекта');
    entries.open(entryActions[id], node.path);
    return;
  }
  if (id === 'document.newWindow') { await app.openInNewWindow(target.path); return; }
  if (id === 'target.open') {
    const hit = app.results?.results.find(hit => hit.path === target.path && hit.first_line === target.line);
    // Находка аудита, вхождение и входящая ссылка несут строку: документ открывается на ней.
    await app.openDocument(hit ?? { path: target.path, title: target.path, first_line: target.line, last_line: target.line });
    return;
  }
  const action = id === 'target.reveal' ? 'reveal' : id === 'target.copyPath' ? 'path' : id === 'target.checkIndex' ? 'check' : 'open';
  const result = await documentAction(target.project, target.path, action, action === 'open' ? editor : undefined);
  if (action === 'path') { await copyText(result); app.flash('Путь скопирован'); }
  if (action === 'check') app.flash('Проверка документа поставлена в очередь');
}
