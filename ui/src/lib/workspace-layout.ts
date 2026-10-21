import type { DockviewApi, SerializedDockview } from "dockview-core";
import type { DocumentReference } from "./app.svelte";

export const panelTitles = { search: "Поиск", navigation: "Навигация", graph: "Граф", settings: "Настройки", logs: "Логи", mcp: "Анализ MCP", indexing: "Индексация", topics: "Новые темы" } as const;
export type PanelKind = keyof typeof panelTitles | "document";
export interface PanelParams { kind: PanelKind; project?: string; hit?: DocumentReference }
export const layoutKey = (project: string | null) => `bars-workspace-v1:${project ?? ""}`;
export const documentId = (path: string) => `document:${path}`;

export function defaultLayout(api: DockviewApi): void {
  api.clear();
  api.addPanel({ id: "search", component: "workspace", title: panelTitles.search, params: { kind: "search" } });
  api.addPanel({ id: "navigation", component: "workspace", title: panelTitles.navigation, params: { kind: "navigation" }, position: { referencePanel: "search", direction: "left" }, initialWidth: 250 });
  api.addPanel({ id: "graph", component: "workspace", title: panelTitles.graph, params: { kind: "graph" }, position: { referencePanel: "navigation", direction: "below" }, initialHeight: 240 });
  api.getPanel("search")!.api.setActive();
}

export function encodeLayout(api: DockviewApi): string {
  return JSON.stringify({ version: 1, layout: api.toJSON() });
}

export function decodeLayout(text: string, project: string | null): SerializedDockview {
  const value = JSON.parse(text);
  if (value?.version !== 1 || !value.layout?.grid || !value.layout.panels || Array.isArray(value.layout.panels)) throw new Error("Неизвестный формат раскладки");
  const panels = Object.values(value.layout.panels) as { id: string; contentComponent: string; params: PanelParams }[];
  if (panels.length > 100) throw new Error("Слишком много панелей");
  const seen = new Set<string>();
  for (const panel of panels) {
    const p = panel.params;
    if (panel.contentComponent !== "workspace" || !p || seen.has(panel.id)) throw new Error("Неизвестная панель");
    seen.add(panel.id);
    if (p.kind === "document") {
      if (p.project !== project || !p.hit || typeof p.hit.path !== "string" || typeof p.hit.title !== "string" || panel.id !== documentId(p.hit.path) || !Number.isInteger(p.hit.first_line) || !Number.isInteger(p.hit.last_line)) throw new Error("Неверная ссылка на документ");
    } else if (!Object.hasOwn(panelTitles, p.kind) || panel.id !== p.kind) throw new Error("Неизвестный вид панели");
  }
  return value.layout;
}
