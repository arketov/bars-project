/** Адаптер сохраняет цель до переноса фокуса в список команд. */
export interface Adapter { element: HTMLElement; reason: () => string | undefined; run: () => void | Promise<void>; identity?: () => string | number | undefined }
const adapters = new Map<string, Set<Adapter>>();
export function commandAdapter(element: HTMLElement, actions: Record<string, { run: Adapter['run']; reason?: Adapter['reason']; identity?: Adapter['identity'] }>) {
  const entries = Object.entries(actions).map(([id, action]) => { const adapter = { element, run: action.run, reason: action.reason ?? (() => undefined), identity: action.identity }; const set = adapters.get(id) ?? new Set(); set.add(adapter); adapters.set(id, set); return { set, adapter }; });
  return { destroy() { entries.forEach(({set,adapter}) => set.delete(adapter)); } };
}
/** Запускает обработчик команды строки, на которой открыто меню; закрытая строка или недоступная команда — ошибка. */
export async function runAdapter(id: string, element: HTMLElement): Promise<void> {
  const adapter = adapterFor(id, element);
  if (!element.isConnected || !adapter) throw new Error('Строка уже закрыта');
  const reason = adapter.reason();
  if (reason) throw new Error(reason);
  await adapter.run();
}
export function adapterFor(id: string, target: HTMLElement | null): Adapter | undefined {
  const items = [...adapters.get(id) ?? []];
  return items.find(a => !!target && a.element.contains(target)) ?? (id === 'notifications.open' ? items.find(a => a.element.isConnected) : undefined);
}
