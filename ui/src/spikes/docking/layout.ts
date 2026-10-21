import type { DockviewApi, SerializedDockview, DockviewMessages } from "dockview-core";

const positionName = (position: string) => ({ left: "слева", right: "справа", top: "сверху", bottom: "снизу", center: "вкладкой" }[position] ?? "рядом");
export const messages: DockviewMessages = {
  panelOpened: title => `Открыто: ${title}`, panelClosed: title => `Закрыто: ${title}`,
  groupMaximized: title => `Развёрнуто: ${title}`, groupRestored: title => `Восстановлено: ${title}`,
  groupFloated: title => `Отделено: ${title}`, groupDocked: title => `Закреплено: ${title}`,
  groupPoppedOut: title => `Новое окно: ${title}`, closeTab: title => `Закрыть: ${title}`, closeTabPlain: () => "Закрыть вкладку",
  movePickTarget: (source, target, current, total) => `${source}: цель ${target}, ${current} из ${total}. Стрелки — выбрать, Enter — продолжить, Escape — отменить.`,
  movePickEdge: (position, target) => `${positionName(position)} от ${target}. Enter — переместить, Escape — отменить.`,
  moveCommitted: (source, target, position) => `${source}: ${positionName(position)} от ${target}`,
  moveCancelled: () => "Перенос отменён", moveNotAllowed: () => "Перенос недоступен", moveFloated: source => `Отделено: ${source}`,
};

// Версия приложения отдельно от формата библиотеки; тексты документов сюда не попадают.
export const key = "bars-docking-probe-v1";
export function save(api: DockviewApi): string {
  return JSON.stringify({ version: 1, layout: api.toJSON() });
}
export function restore(api: DockviewApi, text: string): void {
  const value = JSON.parse(text) as { version: number; layout: SerializedDockview };
  if (value.version !== 1 || !value.layout?.grid || !value.layout.panels) throw new Error("Неподдерживаемая раскладка");
  const allowed = new Set(["tree", "search", "document", "graph", "extra", "draft"]);
  for (const panel of Object.values(value.layout.panels)) {
    if (!allowed.has(panel.id) || panel.contentComponent !== "svelte") throw new Error("Неизвестная панель");
  }
  api.fromJSON(value.layout);
}
export function defaults(api: DockviewApi): void {
  api.clear();
  api.addPanel({ id: "search", component: "svelte", title: "Поиск" });
  api.addPanel({ id: "tree", component: "svelte", title: "Навигация", position: { referencePanel: "search", direction: "left" }, initialWidth: 280 });
  api.addPanel({ id: "graph", component: "svelte", title: "Граф", position: { referencePanel: "tree", direction: "below" } });
  api.addPanel({ id: "document", component: "svelte", title: "Документ", position: { referencePanel: "search", direction: "right" }, initialWidth: 380 });
  api.addPanel({ id: "extra", component: "svelte", title: "Дополнительно", position: { referencePanel: "document", direction: "below" } });
  api.addPanel({ id: "draft", component: "svelte", title: "Черновик", position: { referencePanel: "extra", direction: "within" } });
}
