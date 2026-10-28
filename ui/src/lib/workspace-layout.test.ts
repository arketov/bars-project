import { afterEach, expect, it } from "vitest";
import { createDockview, type DockviewApi } from "dockview-core";
import { decodeLayout, defaultLayout, documentId, dropCell, encodeLayout, layoutKey, legacyLayoutKey, rightZoneWidth, WorkspaceZones } from "./workspace-layout";
let api: DockviewApi, zones: WorkspaceZones;
const sidebar = { width: 300, graph: 200 };
afterEach(() => { api?.dispose(); document.body.replaceChildren(); });
function create() {
  const host = document.createElement("div"); document.body.append(host);
  api = createDockview(host, { closeEmptyGroups: true, createComponent: () => ({ element: document.createElement("div"), init() {} }) });
  api.layout(1200, 800); defaultLayout(api); zones = new WorkspaceZones(api); zones.restore(); zones.size();
}
function doc(path = "docs/a.md") {
  return api.addPanel({ id: documentId(path), component: "workspace", title: path, params: { kind: "document", project: "А", hit: { path, title: path, first_line: 2, last_line: 4 } }, position: { referenceGroup: zones.documentGroup(), direction: "within" } });
}
const sync = () => { zones.normalize(); zones.size(); };
it("первый документ создаёт правую зону 40%, последняя вкладка убирает её", () => {
  create(); expect(api.groups).toHaveLength(1);
  const p = doc(); sync(); expect(api.groups).toHaveLength(2); expect(p.group.width).toBe(480);
  p.api.close(); sync(); expect(api.groups).toHaveLength(1); expect(api.getPanel("search")).toBeDefined();
  expect(zones.state.rightWidth).toBe(480); expect(doc().group.width).toBe(480);
});
it("ширина переживает закрытие, сериализацию и сжатие окна", () => {
  create(); const p = doc(); sync(); p.group.api.setSize({ width: 570 }); sync();
  expect(zones.state.rightWidth).toBe(570);
  api.layout(500, 800); sync(); sync(); expect(zones.state.rightWidth).toBe(570);
  api.layout(1200, 800); sync(); expect(p.group.width).toBe(570);
  p.api.close(); sync();
  const saved = decodeLayout(encodeLayout(api, sidebar, zones.state), "А")!;
  api.fromJSON(saved.layout); zones.restore(saved.zones); sync();
  expect(doc().group.width).toBe(570);
  expect(rightZoneWidth(1200)).toBe(480); expect(rightZoneWidth(500, 900)).toBe(280);
});
it("верх/низ наследует правую роль; служебная вкладка удерживает зону", () => {
  create(); const p = doc(); sync();
  const graph = api.addPanel({ id: "graph", component: "workspace", params: { kind: "graph" }, position: { referencePanel: p.id, direction: "below" } }); sync();
  expect(zones.state.roles[graph.group.id]).toBe("right");
  p.api.close(); sync(); expect(api.groups).toHaveLength(2);
  graph.api.close(); sync(); expect(api.groups).toHaveLength(1);
});
it.each([false, true])("пустой центр: поиск справа, граф уже справа=%s; без дублей", (withGraph) => {
  create(); const p = doc(); sync();
  if (withGraph) api.addPanel({ id: "graph", component: "workspace", params: { kind: "graph" }, position: { referencePanel: p.id, direction: "within" } });
  api.getPanel("search")!.api.moveTo({ group: p.group, position: "center" }); sync();
  expect(zones.state.roles[api.getPanel("search")!.group.id]).toBe("right");
  expect(zones.state.roles[api.getPanel("graph")!.group.id]).toBe("center");
  expect(api.panels.filter(p => p.id === "graph")).toHaveLength(1);
  sync(); expect(api.groups).toHaveLength(2);
});
it("закрытие всего окна восстанавливает поиск; пользовательское деление центра сохраняется", () => {
  create(); api.getPanel("search")!.api.close(); sync(); expect(api.activePanel?.id).toBe("search");
  const logs = api.addPanel({ id: "logs", component: "workspace", params: { kind: "logs" }, position: { referencePanel: "search", direction: "below" } }); sync();
  expect(zones.state.roles[logs.group.id]).toBe("center");
  api.getPanel("search")!.api.close(); sync(); expect(api.getPanel("search")).toBeUndefined(); expect(api.groups).toHaveLength(1);
});
it("выбор правой группы не следует за перенесённым в центр документом", () => {
  create(); const a = doc(); sync(); const b = doc("docs/b.md");
  a.api.moveTo({ group: api.getPanel("search")!.group, position: "center" }); sync();
  expect(doc("docs/c.md").group).toBe(b.group); expect(a.group).not.toBe(b.group);
});
it("v2: пустая правая группа удаляется, её ширина сохраняется", () => {
  create(); const right = api.addGroup({ referenceGroup: api.groups[0], direction: "right", initialWidth: 530 });
  right.api.setSize({ width: 530 });
  const saved = decodeLayout(JSON.stringify({ version: 2, layout: api.toJSON(), sidebar }), "А")!;
  expect(saved.zones.rightWidth).toBe(530);
  api.fromJSON(saved.layout); zones.restore(saved.zones);
  expect(api.getGroup(right.id)).toBeUndefined(); expect(api.groups).toHaveLength(1); expect(doc().group.width).toBe(530);
});
it("v2: единственная непустая группа становится центром без потери вкладки", () => {
  create(); const p = doc(); api.getPanel("search")!.api.close();
  api.addGroup({ referenceGroup: p.group, direction: "left", skipSetActive: true }); p.api.setActive();
  const saved = decodeLayout(JSON.stringify({ version: 2, layout: api.toJSON(), sidebar }), "А")!;
  api.fromJSON(saved.layout); zones.restore(saved.zones);
  expect(api.groups).toHaveLength(1); expect(api.activePanel?.id).toBe(p.id); expect(zones.group("center")?.activePanel?.id).toBe(p.id);
});
it("сериализация сохраняет роли, деление, активность, адреса без текста и размеры навигации", () => {
  create(); const p = doc(); sync();
  const logs = api.addPanel({ id: "logs", component: "workspace", params: { kind: "logs" }, position: { referencePanel: p.id, direction: "below" } }); sync();
  p.api.setActive(); zones.activate();
  const saved = encodeLayout(api, sidebar, zones.state), stored = decodeLayout(saved, "А")!;
  api.fromJSON(stored.layout); zones.restore(stored.zones);
  expect(api.groups).toHaveLength(3); expect(api.activePanel?.id).toBe(p.id);
  expect(zones.group("right")?.id).toBe(stored.zones.last.right);
  expect(stored.sidebar).toEqual(sidebar); expect(saved).not.toContain('"text"');
  expect(() => decodeLayout(saved, "Б")).toThrow("Неверная ссылка");
  api.getPanel("logs")!.api.close(); sync(); expect(zones.state.roles[logs.group.id]).toBeUndefined();
});
it("повреждение и неизвестные версии отклоняются; v1 заменяется; ключи проектов и версий различны", () => {
  create(); expect(() => decodeLayout("{", "А")).toThrow();
  const value = JSON.parse(encodeLayout(api, sidebar, zones.state));
  value.version = 4; expect(() => decodeLayout(JSON.stringify(value), "А")).toThrow();
  value.version = 1; expect(decodeLayout(JSON.stringify(value), "А")).toBeNull();
  value.version = 3; value.layout.panels.search.params.kind = "__proto__";
  expect(() => decodeLayout(JSON.stringify(value), "А")).toThrow();
  value.layout.panels.search.params.kind = "search"; value.sidebar = { width: "x", graph: -5 };
  expect(decodeLayout(JSON.stringify(value), "А")!.sidebar).toEqual({ width: 250, graph: 240 });
  expect(layoutKey("А")).not.toBe(layoutKey("Б")); expect(layoutKey("А")).not.toBe(legacyLayoutKey("А"));
  value.zones.roles = { a: "x" }; expect(() => decodeLayout(JSON.stringify(value), "А")).toThrow("роли");
});

it("новая правая зона занимает всю высоту рядом с разделённым центром", () => {
  create();
  api.addPanel({ id: "logs", component: "workspace", params: { kind: "logs" }, position: { referencePanel: "search", direction: "below" } }); sync();
  const p = doc(); sync(); expect(p.group.height).toBe(800); expect(p.group.width).toBe(480);
});
it("v2 сохраняет пользовательские деления внутри левой ветви как центр", () => {
  create(); const p = doc(); sync();
  const logs = api.addPanel({ id: "logs", component: "workspace", params: { kind: "logs" }, position: { referencePanel: "search", direction: "below" } });
  const mcp = api.addPanel({ id: "mcp", component: "workspace", params: { kind: "mcp" }, position: { referencePanel: logs.id, direction: "right" } });
  const saved = decodeLayout(JSON.stringify({ version: 2, layout: api.toJSON(), sidebar }), "А")!;
  expect(saved.zones.roles[mcp.group.id]).toBe("center"); expect(saved.zones.roles[p.group.id]).toBe("right");
});
it("последний resize сохраняется даже при закрытии до layout-события", () => {
  create(); const p = doc(); sync();
  const stop = api.onDidRemoveGroup(g => zones.removed(g));
  p.group.api.setSize({width:555}); p.api.close(); sync();
  expect(zones.state.rightWidth).toBe(555); stop.dispose();
});
it("суммарная ширина двух правых колонок переживает изменение окна", () => {
  create(); const p = doc(); sync();
  const logs = api.addPanel({id:'logs',component:'workspace',params:{kind:'logs'},position:{referencePanel:p.id,direction:'right'}}); sync();
  const remembered = zones.state.rightWidth!;
  api.layout(1500,800); sync(); sync();
  expect(zones.state.rightWidth).toBe(remembered);
  expect(p.group.width+logs.group.width).toBe(remembered);
});

it("полоса 48 px: справа открывает зону, в углу выигрывает верх/низ, мало места — в группу", () => {
  const center = { role: "center" as const, open: true };
  expect(dropCell(1160, 400, 1200, 800, 480, center)).toBe("right");
  expect(dropCell(1100, 400, 1200, 800, 480, center)).toBe("center");
  expect(dropCell(1160, 10, 1200, 800, 480, center)).toBe("top");
  expect(dropCell(1160, 790, 1200, 800, 480, center)).toBe("bottom");
  expect(dropCell(1160, 400, 1200, 800, 480, { role: "center", open: false })).toBe("center");
  expect(dropCell(1160, 400, 600, 800, 480, center)).toBe("center");
  expect(dropCell(10, 10, 1200, 230, 480, center)).toBe("left");
  expect(dropCell(10, 400, 300, 800, 480, center)).toBe("center");
  const right = { role: "right" as const, open: false };
  expect(dropCell(400, 790, 480, 800, 480, right)).toBe("bottom");
  expect(dropCell(470, 400, 480, 800, 480, right)).toBe("center");
  expect(dropCell(10, 400, 480, 800, 480, right)).toBe("center");
  expect(dropCell(400, 10, 480, 230, 480, right)).toBe("center");
});
it("клетка сброса: зона предлагается только центру без зоны, на всю высоту, у края окна", () => {
  create(); const search = api.getPanel("search")!.group;
  expect(zones.cell(search, 1190, 400, 1200, 800)).toBe("right");
  const p = doc(); sync();
  expect(zones.cell(search, 700, 400, 700, 800)).toBe("center");
  expect(zones.cell(p.group, 400, 790, 480, 800)).toBe("bottom");
  p.api.close(); sync();
  const below = api.addPanel({ id: "graph", component: "workspace", params: { kind: "graph" }, position: { referencePanel: "search", direction: "below" } }); sync();
  expect(zones.cell(below.group, 1190, 200, 1200, 400)).toBe("center");
});
it("зона, появившаяся сбросом, получает размер превью, а не половину группы", () => {
  create();
  const graph = api.addPanel({ id: "graph", component: "workspace", params: { kind: "graph" }, position: { referencePanel: "search", direction: "right" } });
  expect(graph.group.width).toBe(600); sync();
  expect(graph.group.width).toBe(zones.previewWidth()); expect(zones.previewWidth()).toBe(480);
  expect(zones.state.rightWidth).toBeUndefined();
  graph.group.api.setSize({ width: 540 }); sync(); expect(zones.state.rightWidth).toBe(540);
});
