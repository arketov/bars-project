import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount, tick } from "svelte";
import GraphPage from "./GraphPage.svelte";
import { app } from "./app.svelte";
import records from "./fixtures/graph.json";
import type { GraphReply } from "./graph";
const { rpc, destroy, set } = vi.hoisted(() => ({
  rpc: vi.fn(),
  destroy: vi.fn(),
  set: vi.fn(),
}));
vi.mock("./api", () => ({
  call: rpc,
  inTauri: () => true,
  joinPath: async (a: string, b: string) => a + "/" + b,
  openPath: vi.fn(),
  pickFolder: vi.fn(),
}));
vi.mock("./graph-renderer", () => ({
  GraphRenderer: class {
    set = set;
    destroy = destroy;
    pinned = () => false;
    pin = vi.fn();
    stop = vi.fn();
    fit = vi.fn();
    zoom = vi.fn();
    layout = vi.fn();
  },
}));
let component: ReturnType<typeof mount> | null = null,
  target: HTMLDivElement;
const output = () =>
  structuredClone(records["graph:documents"]) as unknown as GraphReply;
const settle = async () => {
  await tick();
  await Promise.resolve();
  flushSync();
  await tick();
};
beforeEach(() => {
  rpc.mockReset();
  destroy.mockClear();
  set.mockClear();
  app.projects = [
    { path: "C:/a", name: "A", docs_root: "docs", state: "ready", modules: [] },
    { path: "C:/b", name: "B", docs_root: "docs", state: "ready", modules: [] },
  ];
  app.current = "C:/a";
  app.tree = [];
  app.selected = new Set();
  app.graphPreview = null;
  target = document.createElement("div");
  document.body.append(target);
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close = function () {
    this.removeAttribute("open");
    this.dispatchEvent(new Event("close"));
  };
  rpc.mockImplementation(async (method: string) =>
    method === "graph_status"
      ? {
          state: "partial",
          snapshot: output().snapshot,
          active_job: null,
          error: null,
        }
      : method === "read"
        ? { title: "Текущий файл", text: "# Текущий файл" }
        : output(),
  );
});
afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  target.remove();
  app.current = null;
  app.projects = [];
});
it("выбор неоднозначной вершины сохраняет операцию и сторону пути", async () => {
  const candidates = output().nodes.slice(0, 2);
  rpc.mockImplementation(async (method: string) =>
    method === "graph_status"
      ? { state: "partial", snapshot: output().snapshot }
      : { ...output(), resolution: "ambiguous", candidates },
  );
  component = mount(GraphPage, { target });
  await settle();
  const operation = target.querySelector<HTMLSelectElement>(
    ".graph-controls select",
  )!;
  operation.value = "paths";
  operation.dispatchEvent(new Event("change", { bubbles: true }));
  await settle();
  [...target.querySelectorAll<HTMLButtonElement>(".graph-candidates button")]
    .find((button) => button.textContent === "Начало пути")!
    .click();
  await settle();
  let request = rpc.mock.calls
    .filter((call) => call[0] === "graph_query")
    .at(-1)![1].request;
  expect(request.operation).toBe("paths");
  expect(request.from).toEqual(candidates[0].reference);
  [...target.querySelectorAll<HTMLButtonElement>(".graph-candidates button")]
    .find((button) => button.textContent === "Конец пути")!
    .click();
  await settle();
  request = rpc.mock.calls
    .filter((call) => call[0] === "graph_query")
    .at(-1)![1].request;
  expect(request.operation).toBe("paths");
  expect(request.from).toEqual(candidates[0].reference);
  expect(request.to).toEqual(candidates[0].reference);
});
it("показывает отрицание и условия каждого перехода", async () => {
  const path = structuredClone(records.paths) as unknown as GraphReply;
  const item = path.items.find((item) => item.type === "path")!;
  if (item.type !== "path") throw Error("нет пути в фикстуре");
  const assertion = path.assertions.find(
    (edge) => edge.id === item.path.steps[0].edge,
  )!;
  assertion.polarity = "negative";
  assertion.qualifiers = { condition: "если отключена сеть" };
  rpc.mockImplementation(async (method: string) =>
    method === "graph_status"
      ? { state: "partial", snapshot: path.snapshot }
      : path,
  );
  component = mount(GraphPage, { target });
  await settle();
  expect(target.querySelector(".graph-inspector")?.textContent).toContain(
    "НЕ " + assertion.predicate,
  );
  expect(target.querySelector(".graph-inspector")?.textContent).toContain(
    "если отключена сеть",
  );
});
it("не показывает запоздавший ответ другого проекта и уничтожает renderer", async () => {
  let resolveFirst!: (value: GraphReply) => void;
  const first = new Promise<GraphReply>((resolve) => (resolveFirst = resolve));
  rpc.mockImplementation((method: string, params: { project: string }) =>
    method === "graph_status"
      ? Promise.resolve({ state: "partial", snapshot: output().snapshot })
      : params.project === "C:/a"
        ? first
        : Promise.resolve({
            ...output(),
            nodes: output().nodes.map((n) => ({ ...n, label: "Проект B" })),
          }),
  );
  component = mount(GraphPage, { target });
  await settle();
  expect(rpc.mock.calls.filter((c) => c[0] === "graph_query")).toHaveLength(1);
  app.current = "C:/b";
  await settle();
  resolveFirst({
    ...output(),
    nodes: output().nodes.map((n) => ({ ...n, label: "Устаревший A" })),
  });
  await settle();
  expect(
    set.mock.lastCall?.[0].nodes.every(
      (n: { label: string }) => n.label === "Проект B",
    ),
  ).toBe(true);
  await unmount(component);
  component = null;
  expect(destroy).toHaveBeenCalled();
});
it("передаёт scope и открывает текущий файл с диапазоном evidence", async () => {
  component = mount(GraphPage, { target });
  await settle();
  const button = [...target.querySelectorAll("button")].find(
    (b) => b.textContent === "Списком",
  )!;
  button.click();
  await settle();
  target.querySelector<HTMLButtonElement>(".graph-list button")!.click();
  await settle();
  const source = [...target.querySelectorAll("button")].find(
    (b) => b.textContent === "Открыть основание ↗",
  )!;
  expect(source).toBeDefined();
  source.click();
  await settle();
  expect(target.querySelector("dialog")?.hasAttribute("open")).toBe(true);
  [...target.querySelectorAll("button")]
    .find((b) => b.textContent === "Открыть текущий файл")!
    .click();
  await settle();
  const read = rpc.mock.calls.find((c) => c[0] === "read")![1];
  expect(read.path).toMatch(/^\.\/docs\//);
  expect(app.viewer?.hit.first_line).toBeGreaterThan(0);
  expect(app.viewer?.text).toBe("# Текущий файл");
  const request = rpc.mock.calls.find((c) => c[0] === "graph_query")![1]
    .request;
  expect(request.scope).toEqual({ modules: [], files: [] });
  expect(target.textContent).toContain("revision:");
  expect(target.textContent).toContain("Отсутствие извлечённой связи");
});
