import { afterEach, expect, it } from "vitest";
import { createDockview, type DockviewApi } from "dockview-core";
import { decodeLayout, defaultLayout, documentId, encodeLayout, ensureRightZone, layoutKey, removableEmptyGroups } from "./workspace-layout";
let api: DockviewApi;
const sidebar = { width: 300, graph: 200 };
afterEach(() => { api?.dispose(); document.body.replaceChildren(); });
function create() {
  const host = document.createElement("div"); document.body.append(host);
  api = createDockview(host, { closeEmptyGroups: false, createComponent: () => ({ element: document.createElement("div"), init() {} }) });
  api.layout(1440, 900); defaultLayout(api, 1440);
}
it("штатная раскладка: поиск и пустая правая зона, без навигации и графа", () => {
  create();
  expect(api.panels.map(p => p.id)).toEqual(["search"]);
  expect(api.groups).toHaveLength(2);
  expect(api.groups.find(g => g !== api.getPanel("search")!.group)!.panels).toHaveLength(0);
});
it("правая зона создаётся снова, когда осталась одна группа", () => {
  create();
  api.removeGroup(api.groups.find(g => g !== api.getPanel("search")!.group)!);
  expect(api.groups).toHaveLength(1);
  ensureRightZone(api, 1440);
  expect(api.groups).toHaveLength(2);
  ensureRightZone(api, 1440);
  expect(api.groups).toHaveLength(2);
});
it("опустевшая зона остаётся, пустая часть разделённой зоны убирается", () => {
  create();
  const searchGroup = api.getPanel("search")!.group;
  const right = api.groups.find(g => g !== searchGroup)!;
  api.addPanel({ id: "graph", component: "workspace", title: "Граф", params: { kind: "graph" }, position: { referenceGroup: right, direction: "within" } });
  api.getPanel("search")!.api.moveTo({ group: right, position: "center" });
  expect(searchGroup.panels).toHaveLength(0);
  expect(api.groups).toHaveLength(2);
  expect(removableEmptyGroups(api)).toEqual([]);
  api.getPanel("search")!.api.moveTo({ group: right, position: "bottom" });
  const bottom = api.getPanel("search")!.group;
  api.getPanel("search")!.api.moveTo({ group: searchGroup, position: "center" });
  expect(removableEmptyGroups(api)).toEqual([bottom.id]);
  ensureRightZone(api, 1440);
  expect(api.groups).toHaveLength(2);
  expect(removableEmptyGroups(api)).toEqual([]);
});
it("восстанавливает перенос, вертикальное разделение, активную вкладку и размеры колонки", () => {
  create();
  api.addPanel({ id: documentId("docs/a.md"), component: "workspace", title: "А", params: { kind: "document", project: "А", hit: { path: "docs/a.md", title: "А", first_line: 2, last_line: 4 } }, position: { referencePanel: "search", direction: "right" } });
  api.addPanel({ id: "graph", component: "workspace", title: "Граф", params: { kind: "graph" }, position: { referencePanel: "search", direction: "below" } });
  api.getPanel(documentId("docs/a.md"))!.api.setActive();
  const saved = encodeLayout(api, sidebar), groups = api.groups.length;
  const stored = decodeLayout(saved, "А")!;
  api.fromJSON(stored.layout);
  expect(api.groups).toHaveLength(groups);
  expect(api.activePanel?.id).toBe(documentId("docs/a.md"));
  expect(api.getPanel("graph")!.group).not.toBe(api.getPanel("search")!.group);
  expect(stored.sidebar).toEqual(sidebar);
  expect(saved).not.toContain('"text"');
  expect(() => decodeLayout(saved, "Б")).toThrow("Неверная ссылка");
});
it("отвергает повреждение, неизвестные панели и версии; версия 1 заменяется молча; ключи проектов различны", () => {
  create();
  expect(() => decodeLayout("{", "А")).toThrow();
  const value = JSON.parse(encodeLayout(api, sidebar));
  value.version = 3; expect(() => decodeLayout(JSON.stringify(value), "А")).toThrow();
  value.version = 1; expect(decodeLayout(JSON.stringify(value), "А")).toBeNull();
  value.version = 2; value.layout.panels.search.params.kind = "__proto__";
  expect(() => decodeLayout(JSON.stringify(value), "А")).toThrow();
  value.layout.panels.search.params.kind = "navigation";
  expect(() => decodeLayout(JSON.stringify(value), "А")).toThrow();
  value.layout.panels.search.params.kind = "search"; value.sidebar = { width: "x", graph: -5 };
  expect(decodeLayout(JSON.stringify(value), "А")!.sidebar).toEqual({ width: 250, graph: 240 });
  expect(layoutKey("А")).not.toBe(layoutKey("Б"));
});
