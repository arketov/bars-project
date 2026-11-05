// Действия смыслового слоя полного графа (2.3.8): влияние, смысловой путь, контекст задачи, обзор тем, аудит связей.
// Воркер замещён записанными по контракту v1 ответами; разбор ответов отдельно проверен в semantic.test.ts.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, tick, unmount } from "svelte";
import PhysicalGraph from "./PhysicalGraph.svelte";
import { app } from "../app.svelte";
import { BarsError } from "../types";
import { physicalFixture as buildPhysicalFixture } from "../mock-physical";
import { queryFixture } from "../mock-query";

const modules = ["adr", "features", "manual"];
const documents = Array.from({ length: 36 }, (_, i) => ({
  path: `docs/${modules[i % 3]}/doc-${String(i).padStart(2, "0")}.md`,
  module: modules[i % 3], title: `Документ ${i}: ${modules[i % 3]}`,
  chunks: 2 + (i % 5) * 2,
}));
const physicalFixture = (params: Record<string, unknown>) => buildPhysicalFixture(params, documents);

const { rpc } = vi.hoisted(() => ({ rpc: vi.fn() }));
vi.mock("../api", () => ({
  call: rpc,
  inTauri: () => true,
  joinPath: async (a: string, b: string) => a + "/" + b,
  openPath: vi.fn(),
  pickFolder: vi.fn(),
}));

let component: ReturnType<typeof mount> | null = null;
let target: HTMLDivElement;
const settle = async () => {
  for (let i = 0; i < 4; i++) {
    await tick();
    await new Promise((r) => setTimeout(r, 5));
  }
  flushSync();
};
const button = (text: string) => [...target.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.trim().startsWith(text));
const strip = (name: string) => target.querySelector<HTMLElement>(`.qbar[aria-label="${name}"]`);
const act = async (id: string) => {
  target.querySelector<HTMLButtonElement>(`.entry-actions [data-action="${id}"]`)!.click();
  await settle();
};
const find = async (text: string) => {
  const input = target.querySelector<HTMLInputElement>('[aria-label="Найти узел графа"]')!;
  input.value = text;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  await settle();
  input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  await settle();
};

beforeEach(() => {
  localStorage.clear();
  rpc.mockReset();
  app.projects = [{ path: "C:/a", name: "A", docs_root: "docs", state: "ready", modules: [] }];
  app.current = "C:/a";
  app.tree = [];
  app.selected = new Set();
  app.graphFocus = null;
  app.activeDocument = null;
  app.results = null;
  app.displayed = null;
  app.searching = false;
  app.searchError = null;
  rpc.mockImplementation(async (method: string, params: Record<string, unknown>) => {
    if (method === "graph_status") return { state: "partial", snapshot: "mock-snapshot-0001", active_job: null, source_count: 36, error: null, method: "x" };
    if (method === "graph_physical") return physicalFixture(params);
    if (method === "graph_query_slice") return queryFixture(params, documents);
    return {};
  });
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  target.remove();
  app.current = null;
  app.projects = [];
});

const sref = (kind: string, id: string) => ({ kind, id, revision: "r1" });
const sdoc = (n: number, module = modules[n % 3]) => ({ reference: sref("document", `doc-${n}`), path: `docs/${module}/doc-${String(n).padStart(2, "0")}.md`, docs_path: `${module}/doc-${String(n).padStart(2, "0")}.md`, module, title: `Документ ${n}: ${module}`, hash: `h${n}` });
const sentity = (id: string, label: string, docs: string[], ev: string[] = []) => ({ reference: sref("entity", id), label, namespace: "t", aliases: [], source_ids: docs, evidence: ev, roles: [] });
const sproof = (id: string, doc: number, quote: string, line: number) => ({ id, document: sref("document", `doc-${doc}`), span: { start: 0, end: 10, first_line: line, last_line: line }, quote, source_hash: `h${doc}`, method: "explicit-markdown-v6" });
const sedge = (id: string, from: string, to: string, predicate: string, evidence: string[], qualifiers: Record<string, string> = {}) => ({ id, from, to, predicate, polarity: "positive", qualifiers, origin: "explicit_rule", evidence });
const sreply = (operation: string, over: Record<string, unknown> = {}) => ({
  schema_version: 1, operation, snapshot: "mock-snapshot-0001", scope: { modules: [], files: [] }, layer_state: "partial", layer_states: { semantic: "partial" },
  coverage: { indexed: 36, processed: 36, failed: 0, denominator: 36, semantic_completeness: null, method: "t" }, nodes: [], sources: [], assertions: [], evidence: [], items: [],
  truncated: false, limit_reasons: [], next_cursor: null, warnings: ["Явные правила не гарантируют полноту смысловых отношений"], resolution: "resolved", candidates: [], missing_roles: [], budget_mode: "bytes", used_bytes: 2048, elapsed_ms: 1, method: "t", ...over,
});
const semantic = (handler: (request: Record<string, any>) => unknown) => {
  const previous = rpc.getMockImplementation()!;
  rpc.mockImplementation(async (method: string, params: Record<string, unknown>) => (method === "graph_query" ? handler(params.request as Record<string, any>) : previous(method, params)));
};
const queries = () => rpc.mock.calls.filter((c) => c[0] === "graph_query").map((c) => c[1].request as Record<string, any>);
const inspector = () => target.querySelector<HTMLElement>(".inspector")!;
const analysis = async (id: string) => {
  button("Анализ")!.click();
  await settle();
  target.querySelector<HTMLButtonElement>(`[data-analysis="${id}"]`)!.click();
  await settle();
};
const impactReply = () => sreply("impact", {
  nodes: [sentity("ent-0", "Порог", ["doc-4"]), sentity("ent-1", "Показ связей", ["doc-5"], ["ev1"])],
  sources: [sdoc(5), sdoc(4)],
  assertions: [sedge("a1", "ent-1", "ent-0", "depends_on", ["ev1"], { condition: "если выбран режим по смыслу" })],
  evidence: [sproof("ev1", 5, "Показ связей зависит от порога.", 7)],
  items: [{ type: "impact", node: sref("entity", "ent-1"), direct: true, path: { nodes: [sref("document", "doc-4"), sref("entity", "ent-1")], steps: [{ edge: "a1", reversed: true }], evidence: ["ev1"], complete: true } }],
});

// Сначала сущности документа (`graph`, вид сущностей, область — файл), затем влияние от них.
const impactHandler = (impact: () => unknown) => (request: Record<string, any>) => (request.operation === "graph" ? sreply("graph", { nodes: [sentity("ent-0", "Порог", ["doc-4"])] }) : impact());

it("неизвестные предикат и ограничение влияния остаются в подсказках (ISS-59)", async () => {
  semantic(impactHandler(() => {
    const raw = impactReply();
    (raw.assertions as ReturnType<typeof sedge>[])[0].predicate = "future_relation";
    return { ...raw, truncated: true, limit_reasons: ["future_limit"] };
  }));
  app.activeDocument = "docs/features/doc-04.md";
  component = mount(PhysicalGraph, { target });
  await settle();
  await act("impact");
  const box = target.querySelector<HTMLElement>('.viewbox[aria-label="Влияние изменения"]')!;
  expect(box.textContent).toContain("связано с");
  expect(box.textContent).toContain("причина ограничения не определена");
  expect(box.textContent).not.toMatch(/future_relation|future_limit/);
  const hints = [...box.querySelectorAll("[title]")].map(el => el.getAttribute("title")).join(" ");
  expect(hints).toContain("future_relation");
  expect(hints).toContain("future_limit");
});

it("«Что затронет правка»: запрос по документу, прямое влияние с основанием и условием, ссылки-соседи отдельно, «Общий вид» снимает вид", async () => {
  semantic(impactHandler(impactReply));
  app.activeDocument = "docs/features/doc-04.md";
  const open = vi.spyOn(app, "openDocument").mockResolvedValue(undefined);
  component = mount(PhysicalGraph, { target });
  await settle();
  await act("impact");
  expect(queries()[0]).toMatchObject({ operation: "graph", view: "entities", scope: { modules: [], files: ["features/doc-04.md"] } });
  expect(queries()[1]).toMatchObject({ operation: "impact", roots: [{ kind: "entity", id: "ent-0", revision: "r1" }], scope: { modules: [], files: [] } });
  expect(strip("Влияние изменения")?.textContent).toContain("прямое 1 · косвенное 0 · ссылки-соседи 1");
  const box = target.querySelector<HTMLElement>('.viewbox[aria-label="Влияние изменения"]')!;
  expect(box.textContent).toContain("Проверено сущностей документа в смысловом слое: 1");
  expect(box.textContent).toContain("Прямое · 1");
  expect(box.textContent).toContain("Показ связей зависит от Порог");
  expect(box.textContent).toContain("условие: если выбран режим по смыслу");
  expect(box.textContent).toContain("проверить описание");
  expect(box.textContent).toContain("Ссылки-соседи · 1");
  expect(box.textContent).toContain("Документ 3: adr");
  expect(box.textContent).toContain("не доказанная зависимость");
  expect(box.textContent).toContain("Смысловой слой неполон");
  expect(target.textContent).toContain("Результат означает «проверить описание», а не «сломано»");
  [...box.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Открыть основание")!.click();
  expect(open).toHaveBeenCalledWith({ path: "docs/manual/doc-05.md", title: "Документ 5: manual", first_line: 7, last_line: 7, expectedRevision: "h5" });
  open.mockRestore();
  button("Общий вид")!.click();
  await settle();
  expect(strip("Влияние изменения")).toBeNull();
  expect(target.querySelector('.viewbox[aria-label="Влияние изменения"]')).toBeNull();
});

it("косвенное влияние показывает основание каждого шага и открывает шестой источник без скрытого лимита", async () => {
  semantic(impactHandler(() => {
    const raw = impactReply();
    const proofs = Array.from({ length: 6 }, (_, i) => sproof(`second-${i}`, 6, `Основание второго шага ${i}`, 20 + i));
    return { ...raw,
      nodes: [...raw.nodes, sentity("ent-2", "Приёмка", ["doc-6"])],
      sources: [...raw.sources, sdoc(6)],
      assertions: [...raw.assertions, sedge("a2", "ent-2", "ent-1", "verifies", proofs.map(p => p.id), { platform: "Windows" })],
      evidence: [...raw.evidence, ...proofs],
      items: [{ type: "impact", node: sref("entity", "ent-2"), direct: false,
        path: { nodes: [sref("entity", "ent-0"), sref("entity", "ent-1"), sref("entity", "ent-2")],
          steps: [{ edge: "a1", reversed: true }, { edge: "a2", reversed: true }], evidence: ["ev1", ...proofs.map(p => p.id)], complete: true } }],
    };
  }));
  app.activeDocument = "docs/features/doc-04.md";
  const open = vi.spyOn(app, "openDocument").mockResolvedValue(undefined);
  component = mount(PhysicalGraph, { target });
  await settle();
  await act("impact");
  const box = target.querySelector<HTMLElement>('.viewbox[aria-label="Влияние изменения"]')!;
  expect(box.textContent).toContain("Шаг 1: Показ связей зависит от Порог");
  expect(box.textContent).toContain("Шаг 2: Приёмка проверяет Показ связей");
  expect(box.textContent).toContain("platform: Windows");
  expect(box.textContent).toContain("Основание второго шага 5");
  const buttons = [...box.querySelectorAll<HTMLButtonElement>("button")].filter(b => b.textContent === "Открыть основание");
  expect(buttons).toHaveLength(7);
  buttons[6].click();
  expect(open).toHaveBeenCalledWith({ path: "docs/adr/doc-06.md", title: "Документ 6: adr", first_line: 25, last_line: 25, expectedRevision: "h6" });
  open.mockRestore();
});

it("«Что затронет правка»: ошибка слоя — ошибка с «Повторить»; пустой ответ не доказывает «ничего не затронет», ссылки-соседи остаются", async () => {
  let fail = true;
  semantic(impactHandler(() => {
    if (fail) throw new BarsError("слой не построен", "graph_not_ready");
    return sreply("impact");
  }));
  app.activeDocument = "docs/features/doc-04.md";
  component = mount(PhysicalGraph, { target });
  await settle();
  await act("impact");
  const box = () => target.querySelector<HTMLElement>('.viewbox[aria-label="Влияние изменения"]')!.textContent!;
  expect(box()).toContain("Смысловой слой ещё не построен");
  expect(box()).toContain("Это не значит, что зависимостей нет");
  expect(box()).not.toContain("Документированных зависимостей не найдено");
  fail = false;
  [...target.querySelectorAll<HTMLButtonElement>(".viewbox button")].find((b) => b.textContent === "Повторить")!.click();
  await settle();
  expect(box()).toContain("Документированных зависимостей не найдено; смысловой слой неполон");
  expect(box()).toContain("Ссылки-соседи · 2");
  expect(strip("Влияние изменения")?.textContent).toContain("ссылки-соседи 2");
});

it("«Что затронет правка»: у документа нет сущностей смыслового слоя — влияние не запрашивается, ответ не обещает «ничего не затронет»", async () => {
  semantic(() => sreply("graph"));
  app.activeDocument = "docs/features/doc-04.md";
  component = mount(PhysicalGraph, { target });
  await settle();
  await act("impact");
  expect(queries().map((r) => r.operation)).toEqual(["graph"]);
  const box = target.querySelector<HTMLElement>('.viewbox[aria-label="Влияние изменения"]')!.textContent!;
  expect(box).toContain("В документе нет сущностей смыслового слоя");
  expect(box).toContain("Это не значит, что правка ничего не затронет");
  expect(box).toContain("Ссылки-соседи · 2");
});

it("«Смысловой путь» из пути по карте: запрос по двум документам, шаги с основаниями, переключатель структуры и возврат к пути по карте", async () => {
  semantic((request) => sreply("paths", {
    nodes: [sentity("ent-0", "Порог", ["doc-4"]), sentity("ent-1", "Показ связей", ["doc-6"], ["ev1"])],
    sources: [sdoc(4), sdoc(6)],
    assertions: [sedge("a1", "ent-1", "ent-0", "depends_on", ["ev1"])],
    evidence: [sproof("ev1", 6, "Показ связей зависит от порога.", 3)],
    items: [{ type: "path", path: { nodes: [sref("document", "doc-4"), sref("entity", "ent-0"), sref("entity", "ent-1"), sref("document", "doc-6")], steps: [{ edge: "a1", reversed: true }], evidence: ["ev1"], complete: true } }],
    method: request.path_kind,
  }));
  app.activeDocument = "docs/features/doc-04.md";
  component = mount(PhysicalGraph, { target });
  await settle();
  await act("path");
  await find("Документ 6:");
  target.querySelector<HTMLButtonElement>('[data-semantic="meaning-path"]')!.click();
  await settle();
  expect(queries().at(-1)).toMatchObject({ operation: "paths", path_kind: "semantic", from: { kind: "document", id: "doc-4" }, to: { kind: "document", id: "doc-6" } });
  expect(strip("Смысловой путь")?.textContent).toContain("Документ 4: features → Документ 6: adr");
  expect(strip("Смысловой путь")?.textContent).toContain("1 вариант");
  expect(strip("Путь по карте")).toBeNull();
  const steps = target.querySelector<HTMLElement>('.viewbox[aria-label="Шаги смыслового пути"]')!;
  expect(steps.textContent).toContain("1. Показ связей зависит от Порог");
  expect(steps.textContent).toContain("ход против направления утверждения");
  expect(steps.textContent).toContain("Показ связей зависит от порога.");
  expect(steps.textContent).toContain("Кратчайший путь не объясняет причину");
  strip("Смысловой путь")!.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click();
  await settle();
  expect(queries().at(-1)).toMatchObject({ path_kind: "mixed" });
  button("Путь по карте")!.click();
  await settle();
  expect(strip("Смысловой путь")).toBeNull();
  expect(strip("Путь по карте")?.textContent).toContain("Документ 4: features → Документ 6: adr");
});

it("«Смысловой путь»: пустой ответ называет границу слоя, неоднозначный конец — кандидаты без угаданного пути", async () => {
  let answer: Record<string, unknown> = sreply("paths");
  semantic(() => answer);
  app.activeDocument = "docs/features/doc-04.md";
  component = mount(PhysicalGraph, { target });
  await settle();
  await act("path");
  await find("Документ 6:");
  target.querySelector<HTMLButtonElement>('[data-semantic="meaning-path"]')!.click();
  await settle();
  const box = () => target.querySelector<HTMLElement>('.viewbox[aria-label="Шаги смыслового пути"]')!.textContent!;
  expect(box()).toContain("Смысловых путей в области не найдено; смысловой слой неполон — отсутствие пути не доказывает отсутствие связи");
  expect(box()).toContain("Структурные шаги (ссылки) можно включить");
  answer = sreply("paths", { resolution: "ambiguous", candidates: [sentity("ent-7", "Показ", ["doc-4"]), sentity("ent-8", "Показ", ["doc-6"])] });
  strip("Смысловой путь")!.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click();
  await settle();
  expect(box()).toContain("Имя конца пути неоднозначно");
  expect(target.querySelectorAll('.viewbox[aria-label="Шаги смыслового пути"] .conns li').length).toBe(2);
});

it("«Контекст задачи»: форма в «Анализе», запрос с профилем и бюджетом, источники по ролям, непокрытые роли и оценочный бюджет", async () => {
  semantic(() => sreply("context", {
    sources: [sdoc(5)],
    evidence: [sproof("ev1", 5, "Граф строится по ссылкам.", 11)],
    items: [{ type: "role_group", role: "requirement", evidence: ["ev1"] }],
    missing_roles: ["verification"], budget_mode: "estimated", used_bytes: 3072, applied: { max_bytes: 12288 },
  }));
  app.query = "как работает граф";
  component = mount(PhysicalGraph, { target });
  await settle();
  await analysis("context");
  const area = target.querySelector<HTMLTextAreaElement>('[aria-label="Задача для контекста"]')!;
  expect(area.value).toBe("как работает граф");
  const profile = target.querySelector<HTMLSelectElement>('[aria-label="Профиль задачи"]')!;
  profile.value = "investigation";
  profile.dispatchEvent(new Event("change", { bubbles: true }));
  await settle();
  [...inspector().querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Собрать контекст")!.click();
  await settle();
  expect(queries()[0]).toMatchObject({ operation: "context", query: "как работает граф", task_profile: "investigation", token_budget: 4096, scope: { modules: [], files: [] } });
  const box = target.querySelector<HTMLElement>('.viewbox[aria-label="Контекст задачи"]')!;
  expect(box.textContent).toContain("Требования · 1");
  expect(box.textContent).toContain("Граф строится по ссылкам.");
  expect(box.textContent).toContain("Непокрытые роли: Проверки");
  expect(box.textContent).toContain("«не найдено в обработанных отношениях», а не «их нет»");
  expect(box.textContent).toContain("оценочный (по размеру ответа, не токены вашей модели)");
  expect(strip("Контекст задачи")?.textContent).toContain("исследование · 1 из 2 ролей · 3.0 из 12 КБ");
  app.query = "";
});

it("«Собрать контекст» из подграфа передаёт документы подграфа областью файлов от корня документации", async () => {
  semantic(() => sreply("context"));
  app.activeDocument = "docs/features/doc-04.md";
  component = mount(PhysicalGraph, { target });
  await settle();
  [...inspector().querySelectorAll<HTMLButtonElement>(".actions button")].find((b) => b.textContent === "В подграф")!.click();
  await settle();
  [...strip("Подграф")!.querySelectorAll("button")].find((b) => b.textContent === "Собрать контекст")!.click();
  await settle();
  const form = target.querySelector<HTMLElement>('.viewbox[aria-label="Контекст задачи"]')!;
  expect(form.querySelector<HTMLInputElement>('input[type="checkbox"]')!.checked).toBe(true);
  expect(form.textContent).toContain("Подграф передан областью документов");
  const area = form.querySelector<HTMLTextAreaElement>("textarea")!;
  area.value = "очередь документов";
  area.dispatchEvent(new Event("input", { bubbles: true }));
  await settle();
  [...form.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Собрать контекст")!.click();
  await settle();
  expect(queries()[0]).toMatchObject({ operation: "context", scope: { modules: [], files: ["features/doc-04.md"] } });
  expect(target.textContent).toContain("Источников по ролям не найдено");
});

it("«Обзор тем»: темы номерами на документах, показ одной темы, «В подграф», переключение группировки", async () => {
  semantic((request) => sreply("explore", {
    sources: [sdoc(3), sdoc(4), sdoc(9)],
    items: request.grouping === "modules"
      ? [{ type: "topic", id: "m1", label: "features", members: [sref("document", "doc-4")], representatives: [sref("document", "doc-4")] }]
      : [
        { type: "topic", id: "t1", label: "Граф / Связи", members: [sref("document", "doc-3"), sref("document", "doc-4")], representatives: [sref("document", "doc-3")] },
        { type: "topic", id: "t2", label: "Модели", members: [sref("document", "doc-9")], representatives: [sref("document", "doc-9")] },
      ],
    method: "document_communities_modularity_v1; title representatives; no generated summaries",
  }));
  component = mount(PhysicalGraph, { target });
  await settle();
  await analysis("topics");
  expect(queries()[0]).toMatchObject({ operation: "explore", analysis: "topics", whole_scope: true, grouping: "communities" });
  expect(strip("Обзор тем")?.textContent).toContain("2 тем · 3 документов");
  const box = target.querySelector<HTMLElement>('.viewbox[aria-label="Темы проекта"]')!;
  expect(box.textContent).toContain("1. Граф / Связи");
  expect(box.textContent).toContain("2 документов");
  expect(box.textContent).toContain("Названия собраны из заголовков представителей, а не придуманы");
  box.querySelector<HTMLButtonElement>(".conns.topics li button[aria-pressed]")!.click();
  await settle();
  expect(box.querySelector<HTMLButtonElement>('.conns.topics li button[aria-pressed="true"]')!.textContent).toBe("Показать все темы");
  [...box.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "В подграф")!.click();
  await settle();
  expect(strip("Подграф")?.textContent).toContain("Подграф: 2 узлов · 2 документов");
  [...strip("Обзор тем")!.querySelectorAll<HTMLButtonElement>('[role="radio"]')].find((b) => b.textContent === "Модули")!.click();
  await settle();
  expect(queries().at(-1)).toMatchObject({ grouping: "modules" });
  expect(strip("Обзор тем")?.textContent).toContain("1 тем");
});

it("аудит сохраняет техническое предупреждение вместе с причиной усечения (ISS-59)", async () => {
  semantic(() => sreply("explore", { truncated: true, limit_reasons: ["max_bytes"], warnings: ["Диагностика future_code"] }));
  component = mount(PhysicalGraph, { target });
  await settle();
  await analysis("audit");
  const box = target.querySelector<HTMLElement>('.viewbox[aria-label="Аудит связей"]')!;
  expect(box.textContent).not.toContain("future_code");
  const hints = [...box.querySelectorAll("[title]")].map(el => el.getAttribute("title")).join(" ");
  expect(hints).toContain("future_code");
  expect(hints).toContain("max_bytes");
});

it("«Аудит связей»: три прохода, разделы с оговорками, ошибка одного прохода не стирает остальные, хабы по числу ссылок", async () => {
  semantic((request) => {
    if (request.analysis === "coverage") throw new BarsError("слой не построен", "graph_not_ready");
    if (request.analysis === "conflict_candidates") return sreply("explore");
    return sreply("explore", {
      sources: [sdoc(4), sdoc(7)],
      evidence: [sproof("ev9", 7, "[Мокап](../x.html)", 5)],
      items: [
        { type: "audit", kind: "structural_isolate", node: sref("document", "doc-4"), evidence: [], detail: "Нет индексированных Markdown-ссылок; смысловая изоляция не доказана" },
        { type: "audit", kind: "target_not_indexed", node: sref("section", "sec-1"), evidence: ["ev9"], detail: "x" },
      ],
    });
  });
  component = mount(PhysicalGraph, { target });
  await settle();
  await analysis("audit");
  expect(queries().map((r) => r.analysis).sort()).toEqual(["conflict_candidates", "connectivity", "coverage"]);
  expect(strip("Аудит связей")?.textContent).toContain("7 находок");
  const box = target.querySelector<HTMLElement>('.viewbox[aria-label="Аудит связей"]')!;
  const tab = (text: string) => [...box.querySelectorAll<HTMLButtonElement>(".tabs button")].find((b) => b.textContent!.startsWith(text))!;
  expect(tab("Изоляты").textContent).toBe("Изоляты · 1");
  expect(tab("Неразрешённые ссылки").textContent).toBe("Неразрешённые ссылки · 1");
  expect(tab("Кандидаты пробелов").textContent).toBe("Кандидаты пробелов · !");
  expect(tab("Хабы").textContent).toMatch(/^Хабы · \d+$/);
  expect(box.textContent).toContain("смысловая изоляция не доказана");
  tab("Неразрешённые ссылки").click();
  await settle();
  expect(box.textContent).toContain("[Мокап](../x.html)");
  expect(box.textContent).toContain("цель ссылки не найдена в индексе");
  tab("Кандидаты пробелов").click();
  await settle();
  expect(box.textContent).toContain("Кандидат, не вердикт");
  expect(box.textContent).toContain("Смысловой слой ещё не построен");
  expect(box.textContent).toContain("Повторить аудит");
  tab("Кандидаты конфликтов").click();
  await settle();
  expect(box.textContent).toContain("Не найдено");
  expect(box.textContent).toContain("Это не доказательство, что проблем нет");
});

it("ответ действия, пришедший после «Общего вида», на карту не ложится", async () => {
  let release: (value: unknown) => void = () => {};
  semantic(() => new Promise((resolve) => { release = resolve; }));
  app.activeDocument = "docs/features/doc-04.md";
  component = mount(PhysicalGraph, { target });
  await settle();
  await act("impact");
  expect(target.textContent).toContain("Читаем смысловой слой");
  button("Общий вид")!.click();
  await settle();
  release(impactReply());
  await settle();
  expect(strip("Влияние изменения")).toBeNull();
  expect(target.querySelector('.viewbox[aria-label="Влияние изменения"]')).toBeNull();
});
