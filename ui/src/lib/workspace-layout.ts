import type { DockviewApi, SerializedDockview } from "dockview-core";
import type { DocumentReference } from "./app.svelte";

// Навигация с мини-графом — постоянная колонка вне Dockview (OQ-117), поэтому её здесь нет.
export const panelTitles = { search: "Поиск", graph: "Граф", settings: "Настройки", logs: "Логи", mcp: "Анализ MCP", indexing: "Индексация", topics: "Новые темы", audits: "Аудиты" } as const;
export type PanelKind = keyof typeof panelTitles | "document";
export interface PanelParams { kind: PanelKind; project?: string; hit?: DocumentReference }
export interface SidebarSize { width: number; graph: number }
export interface StoredLayout { layout: SerializedDockview; sidebar: SidebarSize }
export const defaultSidebar: SidebarSize = { width: 250, graph: 240 };
export const layoutKey = (project: string | null) => `bars-workspace-v2:${project ?? ""}`;
export const documentId = (path: string) => `document:${path}`;

export function defaultLayout(api: DockviewApi, width = 0): void {
  api.clear();
  api.addPanel({ id: "search", component: "workspace", title: panelTitles.search, params: { kind: "search" } });
  ensureRightZone(api, width);
  api.getPanel("search")!.api.setActive();
}

type GridNode = { type: "leaf"; data: { id: string } } | { type: "branch"; data: GridNode[] };
/** Пустые группы, которые можно убрать: внутри разделённой зоны или в лишней зоне. Зоны — дочерние узлы корня сетки. */
export function removableEmptyGroups(api: DockviewApi): string[] {
  const root = api.toJSON().grid.root as GridNode;
  if (root.type === "leaf") return [];
  const empty = new Set(api.groups.filter(g => g.panels.length === 0).map(g => g.id));
  const zones = root.data.length;
  const result: string[] = [];
  let remainingZones = zones;
  for (const zone of root.data) {
    if (zone.type === "leaf") {
      if (empty.has(zone.data.id) && remainingZones > 2) { result.push(zone.data.id); remainingZones--; }
      continue;
    }
    const leaves: string[] = [];
    (function walk(node: GridNode) { if (node.type === "leaf") leaves.push(node.data.id); else node.data.forEach(walk); })(zone);
    const emptyLeaves = leaves.filter(id => empty.has(id));
    // Разделённая зона теряет пустые части, но одна группа зоны остаётся.
    result.push(...(emptyLeaves.length === leaves.length ? emptyLeaves.slice(1) : emptyLeaves));
  }
  return result;
}

/**
 * Центр и правая зона есть всегда (OQ-117). Dockview создаётся с closeEmptyGroups:false:
 * опустевшая зона остаётся на месте и принимает вкладку; пустые части разделённой зоны и лишние
 * пустые зоны убираются; при одной группе справа создаётся пустая.
 */
export function ensureRightZone(api: DockviewApi, width = 0): void {
  for (const id of removableEmptyGroups(api)) { const group = api.getGroup(id); if (group) api.removeGroup(group); }
  if (api.groups.length !== 1) return;
  api.addGroup({ referenceGroup: api.groups[0], direction: "right", skipSetActive: true, ...(width > 0 ? { initialWidth: Math.round(width / 2) } : {}) });
}

export function encodeLayout(api: DockviewApi, sidebar: SidebarSize): string {
  const layout = api.toJSON();
  // ID живого вида сохраняется при переносе буфера; на диск пишутся ID текущих путей.
  const ids = new Map(Object.entries(layout.panels).map(([id, panel]) => {
    const params = panel.params as PanelParams;
    return [id, params?.kind === "document" && params.hit ? documentId(params.hit.path) : id];
  }));
  layout.panels = Object.fromEntries(Object.entries(layout.panels).map(([id, panel]) => {
    const next = ids.get(id)!; return [next, { ...panel, id: next }];
  }));
  const remap = (value: unknown): void => {
    if (!value || typeof value !== "object") return;
    if (Array.isArray(value)) { value.forEach(remap); return; }
    const node = value as Record<string, unknown>;
    if (Array.isArray(node.views)) node.views = node.views.map(id => typeof id === "string" ? ids.get(id) ?? id : id);
    if (typeof node.activeView === "string") node.activeView = ids.get(node.activeView) ?? node.activeView;
    Object.values(node).forEach(remap);
  };
  remap({grid:layout.grid,floatingGroups:layout.floatingGroups,popoutGroups:layout.popoutGroups,edgeGroups:layout.edgeGroups});
  return JSON.stringify({ version: 2, layout, sidebar });
}

const size = (value: unknown, fallback: number) => typeof value === "number" && Number.isFinite(value) && value > 0 && value < 10000 ? Math.round(value) : fallback;

/** null — раскладка прежней версии: молча заменяется штатной. */
export function decodeLayout(text: string, project: string | null): StoredLayout | null {
  const value = JSON.parse(text);
  if (value?.version === 1) return null;
  if (value?.version !== 2 || !value.layout?.grid || !value.layout.panels || Array.isArray(value.layout.panels)) throw new Error("Неизвестный формат раскладки");
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
  return { layout: value.layout, sidebar: { width: size(value.sidebar?.width, defaultSidebar.width), graph: size(value.sidebar?.graph, defaultSidebar.graph) } };
}
