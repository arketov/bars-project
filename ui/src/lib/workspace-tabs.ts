/** Вкладки рабочей области для меню документа: окно с Dockview регистрирует хозяина, отдельное окно документа — нет. */
export interface TabState { others: number; right: number; canMove: boolean }
export interface TabHost {
  /** Вкладка, к которой относится элемент: собственная вкладка или активная вкладка группы, чьё содержимое нажато. */
  panelAt(element: HTMLElement): string | undefined;
  state(panelId: string): TabState | undefined;
  run(id: string, panelId: string): Promise<void>;
}
let current: TabHost | null = null;
export function registerTabHost(host: TabHost): () => void {
  current = host;
  return () => { if (current === host) current = null; };
}
export const tabHost = (): TabHost | null => current;
export const noOtherTabs = 'В группе нет других вкладок';
export const noRightTabs = 'Справа нет вкладок в группе';
export const cannotMoveTab = 'Нет другой группы, а в этой одна вкладка';
