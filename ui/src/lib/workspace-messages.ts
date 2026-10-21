import type { DockviewMessages } from "dockview-core";

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
