import type { DockviewApi, DockviewGroupPanel, SerializedDockview } from "dockview-core";
import type { DocumentReference } from "./app.svelte";

// Навигация с мини-графом — постоянная колонка вне Dockview (OQ-117), поэтому её здесь нет.
export const panelTitles = { search: "Поиск", graph: "Граф", settings: "Настройки", logs: "Логи", mcp: "Анализ MCP", indexing: "Индексация", topics: "Новые темы", audits: "Аудиты" } as const;
export type PanelKind = keyof typeof panelTitles | "document";
export interface PanelParams { kind: PanelKind; project?: string; hit?: DocumentReference }
export interface SidebarSize { width: number; graph: number }
export type ZoneRole = "center" | "right";
export interface ZoneState { roles: Record<string, ZoneRole>; last: Partial<Record<ZoneRole, string>>; rightWidth?: number }
export interface StoredLayout { layout: SerializedDockview; sidebar: SidebarSize; zones: ZoneState }
export const defaultSidebar: SidebarSize = { width: 250, graph: 240 };
export const layoutKey = (project: string | null) => `bars-workspace-v3:${project ?? ""}`;
export const legacyLayoutKey = (project: string | null) => `bars-workspace-v2:${project ?? ""}`;
export const documentId = (path: string) => `document:${path}`;

export function defaultLayout(api: DockviewApi): void {
  api.clear();
  api.addPanel({ id: "search", component: "workspace", title: panelTitles.search, params: { kind: "search" } });
  api.getPanel("search")!.api.setActive();
}

type GridNode = { type: "leaf"; data: { id: string; views: string[] }; size?: number } | { type: "branch"; data: GridNode[]; size?: number };
/** Координаты из дерева Dockview работают и до измерения DOM, в том числе при миграции. */
function columns(layout: SerializedDockview): Map<string, { left: number; width: number }> {
  const result = new Map<string, { left: number; width: number }>();
  function walk(node: GridNode, horizontal: boolean, left: number, width: number) {
    if (node.type === "leaf") { result.set(node.data.id, { left, width }); return; }
    const total = node.data.reduce((n, child) => n + (child.size ?? 1), 0) || 1;
    let offset = left;
    for (const child of node.data) {
      const part = horizontal ? width * (child.size ?? 1) / total : width;
      walk(child, !horizontal, horizontal ? offset : left, part);
      if (horizontal) offset += part;
    }
  }
  walk(layout.grid.root as GridNode, layout.grid.orientation === "HORIZONTAL", 0, layout.grid.width);
  return result;
}
function inferredZones(layout: SerializedDockview): ZoneState {
  const boxes = columns(layout), roles: ZoneState["roles"] = {};
  function mark(node: GridNode, horizontal: boolean, role?: ZoneRole) {
    if (node.type === "leaf") { roles[node.data.id] = role ?? "center"; return; }
    node.data.forEach((child, i) => mark(child, !horizontal, role ?? (horizontal ? (i === 0 ? "center" : "right") : undefined)));
  }
  mark(layout.grid.root as GridNode, layout.grid.orientation === "HORIZONTAL");
  const nonempty = [...boxes.keys()].filter(id => {
    const find = (node: GridNode): boolean => node.type === "leaf" ? node.data.id === id && node.data.views.length > 0 : node.data.some(find);
    return find(layout.grid.root as GridNode);
  });
  const right = [...boxes].filter(([id]) => roles[id] === "right").map(([, b]) => b);
  const rightWidth = right.length ? Math.max(...right.map(b => b.left + b.width)) - Math.min(...right.map(b => b.left)) : undefined;
  if (nonempty.length === 1) roles[nonempty[0]] = "center";
  return { roles, last: {}, rightWidth };
}

export function rightZoneWidth(available: number, remembered?: number): number {
  // Те же минимумы передаются Dockview; на узком окне содержимое разворачивает CSS.
  return Math.round(Math.max(220, Math.min(remembered ?? available * .4, Math.max(220, available - 220))));
}

/** Роли не зависят от названия/наличия вкладки поиска. Нормализация — после завершения операции Dockview. */
export class WorkspaceZones {
  state: ZoneState = { roles: {}, last: {} };
  private available = 0;
  private measured = 0;
  private constrained = new Set<string>();
  constructor(private api: DockviewApi) {}
  restore(state?: ZoneState) {
    this.state = state ? structuredClone(state) : inferredZones(this.api.toJSON());
    this.constrained.clear();
    this.available = 0; this.measured = 0;
    this.normalize();
  }
  group(role: ZoneRole) {
    const groups = this.api.groups.filter(g => this.state.roles[g.id] === role);
    return groups.find(g => g.id === this.state.last[role]) ?? groups[0];
  }
  activate() {
    const group = this.api.activeGroup, role = group && this.state.roles[group.id];
    if (group && role) this.state.last[role] = group.id;
  }
  removed(group: DockviewGroupPanel) {
    // Закрытие может опередить отложенное onDidLayoutChange после движения разделителя.
    if (this.state.roles[group.id] === "right" && !this.api.groups.some(g => g !== group && this.state.roles[g.id] === "right") && this.api.width === this.available && group.width > 0 && group.width !== this.measured) this.state.rightWidth = Math.round(group.width);
    this.constrained.delete(group.id);
  }
  private assign() {
    const boxes = columns(this.api.toJSON()), roles = this.state.roles;
    for (const [id, b] of boxes) {
      if (!this.constrained.has(id)) { this.api.getGroup(id)?.api.setConstraints({ minimumWidth: 220, minimumHeight: 120 }); this.constrained.add(id); }
      if (roles[id]) continue;
      const sibling = [...boxes].find(([other, box]) => other !== id && roles[other] && Math.abs(box.left - b.left) < 1);
      const centers = [...boxes].filter(([other]) => roles[other] === "center");
      roles[id] = sibling ? roles[sibling[0]] : centers.length ? (b.left > Math.min(...centers.map(([, box]) => box.left)) ? "right" : "center") : Object.values(roles).includes("center") ? "right" : "center";
    }
    for (const id of Object.keys(roles)) if (!boxes.has(id)) delete roles[id];
    for (const role of ["center", "right"] as const) if (!this.api.getGroup(this.state.last[role] ?? "")) delete this.state.last[role];
  }
  normalize() {
    this.assign();
    for (const group of [...this.api.groups]) if (!group.panels.length) this.api.removeGroup(group);
    if (!this.group("center")) {
      const right = this.group("right");
      const group = right ? this.api.addGroup({ direction: "left", skipSetActive: true }) : this.api.addGroup();
      this.state.roles[group.id] = "center";
      const kind = this.api.getPanel("search") ? "graph" : "search";
      const existing = this.api.getPanel(kind);
      const active = this.api.activePanel;
      if (existing) existing.api.moveTo({ group, position: "center", skipSetActive: true });
      else this.api.addPanel({ id: kind, component: "workspace", title: panelTitles[kind], params: { kind }, position: { referenceGroup: group, direction: "within" }, inactive: true });
      if (active) active.api.setActive(); else group.activePanel?.api.setActive();
    }
    this.assign(); this.activate();
  }
  documentGroup() {
    const existing = this.group("right");
    if (existing) return existing;
    const group = this.api.addGroup({ direction: "right", initialWidth: rightZoneWidth(this.api.width, this.state.rightWidth) });
    this.state.roles[group.id] = "right";
    group.api.setConstraints({ minimumWidth: 220, minimumHeight: 120 });
    group.api.setSize({ width: rightZoneWidth(this.api.width, this.state.rightWidth) });
    return group;
  }
  size() {
    const available = this.api.width;
    const rightColumns = () => [...columns(this.api.toJSON())].filter(([id]) => this.state.roles[id] === "right");
    const width = () => {
      const boxes = rightColumns().map(([, b]) => b);
      return boxes.length ? Math.round(Math.max(...boxes.map(b => b.left + b.width)) - Math.min(...boxes.map(b => b.left))) : 0;
    };
    const measured = width();
    if (available !== this.available) {
      this.available = available;
      if (measured && this.state.rightWidth) {
        const target = rightZoneWidth(available, this.state.rightWidth);
        // Меняем границу зон: изменение отдельной правой колонки двигает её внутренний разделитель.
        const center = [...columns(this.api.toJSON())].filter(([id]) => this.state.roles[id] === "center").sort((a, b) => b[1].left - a[1].left)[0];
        if (center) this.api.getGroup(center[0])?.api.setSize({ width: center[1].width + measured - target });
      }
      this.measured = width();
    } else {
      if (measured > 0 && measured !== this.measured) this.state.rightWidth = measured;
      this.measured = measured;
    }
  }
}

export function encodeLayout(api: DockviewApi, sidebar: SidebarSize, zones: ZoneState = inferredZones(api.toJSON())): string {
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
  return JSON.stringify({ version: 3, layout, sidebar, zones });
}

const size = (value: unknown, fallback: number) => typeof value === "number" && Number.isFinite(value) && value > 0 && value < 10000 ? Math.round(value) : fallback;

/** null — раскладка прежней версии: молча заменяется штатной. */
export function decodeLayout(text: string, project: string | null): StoredLayout | null {
  const value = JSON.parse(text);
  if (value?.version === 1) return null;
  if (![2, 3].includes(value?.version) || !value.layout?.grid || !value.layout.panels || Array.isArray(value.layout.panels)) throw new Error("Неизвестный формат раскладки");
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
  const zones: ZoneState = value.version === 2 ? inferredZones(value.layout) : value.zones;
  if (!zones || !zones.roles || Array.isArray(zones.roles) || !zones.last || Array.isArray(zones.last) || Object.values(zones.roles).some(role => role !== "center" && role !== "right")) throw new Error("Неверные роли групп");
  zones.rightWidth = zones.rightWidth === undefined ? undefined : size(zones.rightWidth, 0) || undefined;
  return { layout: value.layout, zones, sidebar: { width: size(value.sidebar?.width, defaultSidebar.width), graph: size(value.sidebar?.graph, defaultSidebar.graph) } };
}
