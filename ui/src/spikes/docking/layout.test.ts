import { afterEach, expect, it, vi } from "vitest";
import { createDockview, type DockviewApi } from "dockview-core";
import { defaults, restore, save } from "./layout";

vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} });
const docks: DockviewApi[] = [];
afterEach(() => { docks.splice(0).forEach(api => api.dispose()); document.body.replaceChildren(); });
function dock() {
  const host = document.createElement("div");
  document.body.append(host);
  const api = createDockview(host, { disableAutoResizing: true, createComponent: () => ({ element: document.createElement("textarea"), init() {} }) });
  docks.push(api); api.layout(1280, 740); defaults(api);
  return { api, host };
}
it("перенос сохраняет DOM и ввод, разделение переживает сериализацию", () => {
  const { api, host } = dock();
  const draft = api.getPanel("draft")!;
  const element = draft.view.content.element as HTMLTextAreaElement;
  element.value = "Не терять правки";
  draft.api.moveTo({ group: api.getPanel("search")!.group });
  expect(draft.view.content.element).toBe(element);
  expect(element.value).toBe("Не терять правки");
  expect(host.contains(element)).toBe(true);
  draft.api.moveTo({ group: api.getPanel("search")!.group, position: "bottom" });
  const panels = api.panels.map(p => p.id).sort();
  const groups = api.groups.length;
  restore(api, save(api));
  expect(api.panels.map(p => p.id).sort()).toEqual(panels);
  expect(api.groups.length).toBe(groups);
});
it("другие версии, неизвестные компоненты и повреждённый JSON отклоняются", () => {
  const { api } = dock();
  for (const data of ["{broken", '{"version":2}', save(api).replace('"svelte"', '"foreign"')]) {
    expect(() => restore(api, data)).toThrow();
    defaults(api);
    expect(api.panels.length).toBe(6);
  }
});
