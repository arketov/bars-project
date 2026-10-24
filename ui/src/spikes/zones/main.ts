import { createDockview, themeLight, type DockviewApi } from "dockview-core";
import "dockview/dist/styles/dockview.css";

// Спайк 3.2.2: правая зона по событиям указателя. Не подключён к App.svelte; результаты — docs/knowledge/window-docking.md.
const params = new URLSearchParams(location.search);
const closeEmpty = params.get("close") !== "false";
const edge = params.get("edge") !== "off";

const log: string[] = [];
const api: DockviewApi = createDockview(document.getElementById("host")!, {
  theme: themeLight, dndStrategy: "pointer", disableFloatingGroups: true, closeEmptyGroups: closeEmpty,
  dndEdges: edge ? { size: { value: 40, type: "percentage" }, activationSize: { value: 48, type: "pixels" } } : undefined,
  createComponent: o => { const element = document.createElement("div"); element.style.cssText = "padding:8px;height:100%"; element.textContent = o.name + ":" + o.id; return { element, init() {} }; },
});
api.onWillShowOverlay(e => log.push(`overlay ${e.kind} ${e.position}`));
api.onDidLayoutChange(() => log.push(`layout groups=${api.groups.length}`));
api.addPanel({ id: "search", component: "p", title: "Поиск" });
api.addPanel({ id: "graph", component: "p", title: "Граф", position: { referencePanel: "search", direction: "within" } });
api.addPanel({ id: "doc1", component: "p", title: "Док 1", position: { referencePanel: "search", direction: "right" } });
api.addPanel({ id: "doc2", component: "p", title: "Док 2", position: { referencePanel: "doc1", direction: "within" } });
api.addPanel({ id: "doc3", component: "p", title: "Док 3", position: { referencePanel: "doc1", direction: "within" } });
Object.assign(window, { __zones: { api, log, closeEmpty, edge } });

// Жест мышью событиями указателя с промежуточными движениями (синтетический left_click_drag края не даёт, см. журнал 21-10-2026).
const wait = (ms: number) => new Promise(r => setTimeout(r, ms));
const fire = (type: string, x: number, y: number, target?: Element) =>
  (target ?? document.elementFromPoint(x, y) ?? document.body).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, pointerId: 1, pointerType: "mouse", isPrimary: true, clientX: x, clientY: y, button: 0, buttons: type === "pointerup" ? 0 : 1 }));
const rect = (e: Element | null) => { if (!e) return null; const b = e.getBoundingClientRect(); return [b.x, b.y, b.width, b.height].map(Math.round); };
async function drag(panel: string, to: { x: number; y: number }, drop = true) {
  const tab = [...document.querySelectorAll<HTMLElement>(".dv-tab")].find(t => t.textContent?.trim() === api.getPanel(panel)!.api.title)!;
  const r = tab.getBoundingClientRect(), sx = r.x + r.width / 2, sy = r.y + r.height / 2;
  fire("pointerdown", sx, sy, tab);
  for (let i = 1; i <= 12; i++) { fire("pointermove", sx + (to.x - sx) * i / 12, sy + (to.y - sy) * i / 12); await wait(20); }
  await wait(450);
  const overlay = rect(document.querySelector(".dv-drop-target-selection"));
  const kind = document.querySelector(".dv-drop-target-selection")?.className ?? null;
  if (drop) { fire("pointerup", to.x, to.y); await wait(250); }
  return { overlay, kind };
}
const shape = () => api.groups.map(g => `${g.panels.map(p => p.id).join("+") || "∅"}@${Math.round(g.width)}x${Math.round(g.height)}`);
Object.assign((window as any).__zones, { drag, shape, wait });
