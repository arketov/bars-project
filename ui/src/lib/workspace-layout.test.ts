import { afterEach, expect, it } from "vitest";
import { createDockview, type DockviewApi } from "dockview-core";
import { decodeLayout, defaultLayout, documentId, encodeLayout, layoutKey } from "./workspace-layout";
let api: DockviewApi;
afterEach(() => { api?.dispose(); document.body.replaceChildren(); });
function create() {
  const host = document.createElement("div"); document.body.append(host);
  api = createDockview(host, { createComponent: () => ({ element: document.createElement("div"), init() {} }) });
  api.layout(1440, 900); defaultLayout(api);
}
it("восстанавливает перенос, вертикальное разделение и активную вкладку", () => {
  create();
  api.addPanel({ id: documentId("docs/a.md"), component: "workspace", title: "А", params: { kind: "document", project: "А", hit: { path: "docs/a.md", title: "А", first_line: 2, last_line: 4 } }, position: { referencePanel: "search", direction: "right" } });
  api.getPanel("graph")!.api.moveTo({ group: api.getPanel("search")!.group, position: "bottom" });
  api.getPanel(documentId("docs/a.md"))!.api.setActive();
  const saved = encodeLayout(api), groups = api.groups.length;
  api.fromJSON(decodeLayout(saved, "А"));
  expect(api.groups).toHaveLength(groups);
  expect(api.activePanel?.id).toBe(documentId("docs/a.md"));
  expect(api.getPanel("graph")!.group).not.toBe(api.getPanel("navigation")!.group);
  expect(saved).not.toContain('"text"');
  expect(() => decodeLayout(saved, "Б")).toThrow("Неверная ссылка");
});
it("отвергает повреждение, неизвестные панели и версии; ключи проектов различны", () => {
  create();
  expect(() => decodeLayout("{", "А")).toThrow();
  const value = JSON.parse(encodeLayout(api));
  value.version = 2; expect(() => decodeLayout(JSON.stringify(value), "А")).toThrow();
  value.version = 1; value.layout.panels.search.params.kind = "__proto__";
  expect(() => decodeLayout(JSON.stringify(value), "А")).toThrow();
  expect(layoutKey("А")).not.toBe(layoutKey("Б"));
});
