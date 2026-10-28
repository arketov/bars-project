import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, tick, unmount } from "svelte";
import PhysicalGraph from "./PhysicalGraph.svelte";
import { app } from "../app.svelte";
import { BarsError } from "../types";
import { physicalFixture as buildPhysicalFixture } from "../mock-physical";
import { queryFixture } from "../mock-query";

// Большой набор нужен только проверкам порций и пределов, не браузерному макету.
const modules = ["adr", "features", "manual"];
const documents = Array.from({ length: 36 }, (_, i) => ({
  path: `docs/${modules[i % 3]}/doc-${String(i).padStart(2, "0")}.md`,
  module: modules[i % 3], title: `Документ ${i}: ${modules[i % 3]}`,
  chunks: i === 7 ? 120 : 2 + (i % 5) * 2,
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
const physicalCalls = () => rpc.mock.calls.filter((c) => c[0] === "graph_physical").map((c) => c[1].request);
const settle = async () => {
  for (let i = 0; i < 4; i++) {
    await tick();
    await new Promise((r) => setTimeout(r, 5));
  }
  flushSync();
};
const panel = () => target.querySelector<HTMLElement>(".pg")!;
const button = (text: string) => [...target.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.trim().startsWith(text));

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
  app.graphSearch = null;
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

it("показывает остановку и ошибки файлов, продолжает явным построением", async () => {
  rpc.mockImplementation(async (method: string, params: Record<string, unknown>) => {
    if (method === "graph_status") return { state: "ready", freshness: "current", completeness: "partial", job_state: "stopped", snapshot: "mock-snapshot-0001", active_job: null, paused: true, file_errors_total: 1, file_errors: [{ path: "docs/Ошибка.md", code: "invalid_utf8", message: "Файл не является UTF-8" }] };
    if (method === "graph_physical") return physicalFixture(params);
    return {};
  });
  component = mount(PhysicalGraph, { target });
  await settle();
  expect(target.textContent).toContain("Построение остановлено");
  expect(target.textContent).toContain("Граф актуален");
  expect(target.textContent).toContain("Есть известные пропуски");
  expect(target.textContent).toContain("Файлов с ошибками: 1");
  expect(target.textContent).toContain("Ошибка.md: Файл не является UTF-8");
  expect(button("Остановить построение")!.disabled).toBe(true);
  expect(button("Продолжить")!.disabled).toBe(false);
  target.querySelector<HTMLButtonElement>('[aria-label="Настройки графа"]')!.click();
  await settle();
  const select = target.querySelector<HTMLSelectElement>('[aria-label="Слой для обновления"]')!;
  select.value = "structure";
  select.dispatchEvent(new Event("change", { bubbles: true }));
  await settle();
  button("Продолжить")!.click();
  await settle();
  expect(rpc.mock.calls.some(c => c[0] === "graph_build" && c[1].layer === "structure")).toBe(true);
});

it("загружает срез проекта с начальными пределами и показывает документы, режимы и счётчики", async () => {
  component = mount(PhysicalGraph, { target });
  await settle();
  const request = physicalCalls()[0];
  expect(request).toMatchObject({ limits: { max_documents: 500, max_chunks: 500, max_edges: 4000 }, expanded: [], snapshot: null, scope: { modules: [], files: [] } });
  expect(panel().dataset.loaded).toBe("true");
  expect(panel().dataset.nodes).toBe("36");
  expect(target.textContent).toContain("Документы 36 из 36");
  const radios = [...target.querySelectorAll('[role="radio"]')].map((b) => b.textContent!.replace(/\s+/g, " ").trim());
  expect(radios.some((r) => r.startsWith("Ссылки"))).toBe(true);
  expect(radios.some((r) => r.startsWith("По смыслу ") || r === "По смыслу")).toBe(true);
  // По умолчанию режим «Ссылки»: на карте только ссылки.
  expect(panel().dataset.mode).toBe("links");
  expect(Number(panel().dataset.links)).toBeGreaterThan(0);
});

it("мини-граф при открытом документе показывает его локальный граф: документ и соседей по ссылкам (Т-43, раунд 27)", async () => {
  const slice = await physicalFixture({ request: { scope: { modules: [], files: [] }, limits: { max_documents: 500, max_chunks: 500, max_edges: 4000, max_bytes: 1 << 20 }, expanded: [], snapshot: null } });
  const links = slice.edges.filter((e) => e.kind === "markdown_link");
  const focus = links[0].from;
  const neighbours = new Set([focus, ...links.filter((e) => e.from === focus).map((e) => e.to), ...links.filter((e) => e.to === focus).map((e) => e.from)]);
  app.activeDocument = slice.nodes[focus].path;
  target.className = "mini";
  component = mount(PhysicalGraph, { target });
  await settle();
  expect(Number(panel().dataset.nodes)).toBe(neighbours.size);
  expect(target.textContent).toContain("Локальный граф");
  expect(target.textContent).toContain(slice.nodes[focus].label);
  // Другой документ — граф перестраивается вокруг него; без документа — общий вид проекта.
  app.activeDocument = null;
  await settle();
  expect(panel().dataset.nodes).toBe("36");
});

it("смена режима перекладывает связи и сохраняется отдельно для полного графа", async () => {
  component = mount(PhysicalGraph, { target });
  await settle();
  const links = Number(panel().dataset.links);
  [...target.querySelectorAll<HTMLButtonElement>('[role="radio"]')].find((b) => b.textContent!.includes("Все связи"))!.click();
  await settle();
  expect(panel().dataset.mode).toBe("all");
  expect(Number(panel().dataset.links)).toBeGreaterThan(links);
  expect(JSON.parse(localStorage.getItem("bars.graph.v3.full.C:/a")!).mode).toBe("all");
  expect(localStorage.getItem("bars.graph.v3.mini.C:/a")).toBeNull();
});

it("раскрытие документа запрашивает его чанки порциями по 50 на том же снимке", async () => {
  component = mount(PhysicalGraph, { target });
  await settle();
  target.querySelector<HTMLButtonElement>('[aria-label="Показать списком"]')!.click();
  await settle();
  const row = [...target.querySelectorAll<HTMLButtonElement>(".list li button")].find((b) => b.textContent!.includes("Документ 7:"))!;
  row.click();
  await settle();
  button("Раскрыть чанки (120)")!.click();
  await settle();
  let request = physicalCalls().at(-1)!;
  expect(request.expanded).toEqual([{ document: "doc-7", shown: 50 }]);
  expect(request.snapshot).toBe("mock-snapshot-0001");
  expect(target.textContent).toContain("Показано 50 из 120 чанков");
  expect(target.textContent).toMatch(/По смыслу · чанки\s*\d+/);
  button("Ещё 50")!.click();
  await settle();
  request = physicalCalls().at(-1)!;
  expect(request.expanded).toEqual([{ document: "doc-7", shown: 100 }]);
  button("Свернуть чанки")!.click();
  await settle();
  expect(target.textContent).not.toContain("Показано 100 из 120 чанков");
});

it("усечённый срез объясняет причину и продолжается увеличением пределов на том же снимке", async () => {
  rpc.mockImplementation(async (method: string, params: Record<string, unknown>) => {
    if (method === "graph_status") return { state: "partial", snapshot: "mock-snapshot-0001", active_job: null };
    const request = params.request as { limits: { max_documents: number } };
    return physicalFixture({ ...params, request: { ...request, limits: { ...request.limits, max_documents: Math.min(request.limits.max_documents, 12) } } });
  });
  component = mount(PhysicalGraph, { target });
  await settle();
  expect(target.textContent).toContain("Граф усечён");
  expect(target.textContent).toContain("документов показано 12 из 36");
  button("Показать ещё")!.click();
  await settle();
  expect(physicalCalls().at(-1)!.limits.max_documents).toBe(1000);
  expect(physicalCalls().at(-1)!.snapshot).toBe("mock-snapshot-0001");
});

it("устаревший снимок при раскрытии приводит к повторной загрузке без снимка", async () => {
  component = mount(PhysicalGraph, { target });
  await settle();
  let first = true;
  rpc.mockImplementation(async (method: string, params: Record<string, unknown>) => {
    if (method === "graph_status") return { state: "partial", snapshot: "mock-snapshot-0001", active_job: null };
    if (first && (params.request as { snapshot: string | null }).snapshot) {
      first = false;
      throw new BarsError("снимок графа устарел", "snapshot_expired");
    }
    return physicalFixture(params);
  });
  target.querySelector<HTMLButtonElement>('[aria-label="Показать списком"]')!.click();
  await settle();
  [...target.querySelectorAll<HTMLButtonElement>(".list li button")].find((b) => b.textContent!.includes("Документ 7:"))!.click();
  await settle();
  button("Раскрыть чанки (120)")!.click();
  await settle();
  const sent = physicalCalls();
  expect(sent.at(-2)!.snapshot).toBe("mock-snapshot-0001");
  expect(sent.at(-1)!.snapshot).toBeNull();
  expect(panel().dataset.loaded).toBe("true");
});

it("граф не построен: показывает состояние и ставит построение, а не пустую карту", async () => {
  rpc.mockImplementation(async (method: string) => {
    if (method === "graph_status") return { state: "stale", snapshot: null, active_job: null };
    if (method === "graph_physical") throw new BarsError("граф ещё не готов: документы изменились после построения, граф ждёт обновления", "graph_not_ready");
    return {};
  });
  component = mount(PhysicalGraph, { target });
  await settle();
  expect(target.textContent).toContain("Граф ещё не построен или устарел");
  button("Обновить граф")!.click();
  await settle();
  expect(rpc.mock.calls.some((c) => c[0] === "graph_build")).toBe(true);
});

it("смена проекта сбрасывает модель и отбрасывает поздний ответ прежнего", async () => {
  app.projects = [
    { path: "C:/a", name: "A", docs_root: "docs", state: "ready", modules: [] },
    { path: "C:/b", name: "B", docs_root: "docs", state: "ready", modules: [] },
  ];
  const gate = { release: () => {} };
  rpc.mockImplementation(async (method: string, params: Record<string, unknown>) => {
    if (method === "graph_status") return { state: "partial", snapshot: "mock-snapshot-0001", active_job: null };
    if (params.project === "C:/a") await new Promise<void>((r) => (gate.release = r));
    return physicalFixture({ ...params, request: { ...(params.request as object), limits: { max_documents: params.project === "C:/a" ? 20 : 8, max_chunks: 500, max_edges: 4000, max_bytes: 1 << 20 } } });
  });
  component = mount(PhysicalGraph, { target });
  await settle();
  app.current = "C:/b";
  await settle();
  gate.release();
  await settle();
  expect(panel().dataset.nodes).toBe("8");
});

it("«В графе» из мини-графа выбирает узел в полном графе", async () => {
  component = mount(PhysicalGraph, { target });
  await settle();
  app.graphFocus = "doc-3";
  await settle();
  expect(app.graphFocus).toBeNull();
  expect(target.querySelector(".inspector h3")?.textContent).toContain("Документ 3");
});

const searchHit = (doc: number, score: number) => ({ path: `docs/${["adr", "features", "manual"][doc % 3]}/doc-${String(doc).padStart(2, "0")}.md`, module: "m", title: `Документ ${doc}`, heading_chain: "Раздел", first_line: 1, last_line: 4, text: "", score, score_kind: "rerank" as const });
const finishSearch = (id: string, query: string, docs: number[]) => {
  app.searching = false;
  app.searchError = null;
  app.results = { results: docs.map((d, i) => searchHit(d, 0.95 - i * 0.1)), applied: { results: docs.length, candidates: 50, rerank_candidates: 50, min_score: 0.1, score_kind: "rerank", clamped: false }, indexing: false, pending: 0 };
  app.displayed = { requestId: id, project: "C:/a", query, modules: [], files: [], mode: "hybrid", includeRelated: true, limit: null, minScore: null };
};

it("вид запроса: после поиска граф перестраивается вокруг запроса, «Общий вид» возвращает проект, новый поиск включает вид снова", async () => {
  component = mount(PhysicalGraph, { target });
  await settle();
  expect(panel().dataset.nodes).toBe("36");
  expect(target.querySelector(".qbar")).toBeNull();

  finishSearch("r1", "как работает граф", [3, 7, 11]);
  await settle();
  expect(target.querySelector(".qbar")).not.toBeNull();
  expect(target.querySelector(".qbar")!.textContent).toContain("Запрос «как работает граф»");
  expect(target.querySelector(".qbar")!.textContent).toContain("3 результатов");
  expect(target.querySelector(".qbar")!.textContent).toContain("оценка rerank");
  // Запрос, три результата и их окружение; не весь проект.
  const nodes = Number(panel().dataset.nodes);
  expect(nodes).toBeGreaterThanOrEqual(4);
  expect(nodes).toBeLessThan(36);
  expect(target.textContent).toContain("Запрос: 3 результатов");
  // Связь запроса с каждым результатом видна, даже в режиме «Ссылки».
  expect(Number(panel().dataset.links)).toBeGreaterThanOrEqual(3);
  // Граф ничего не запрашивал у воркера заново ради запроса.
  expect(physicalCalls()).toHaveLength(1);

  button("Общий вид")!.click();
  await settle();
  expect(target.querySelector(".qbar")).toBeNull();
  expect(panel().dataset.nodes).toBe("36");
  expect(target.textContent).toContain("Показать запрос в графе");

  button("Показать запрос в графе")!.click();
  await settle();
  expect(target.querySelector(".qbar")).not.toBeNull();

  button("Общий вид")!.click();
  await settle();
  finishSearch("r2", "другой запрос", [5]);
  await settle();
  expect(target.querySelector(".qbar")!.textContent).toContain("другой запрос");

  app.results = null;
  app.displayed = null;
  await settle();
  expect(target.querySelector(".qbar")).toBeNull();
  expect(panel().dataset.nodes).toBe("36");
  app.searching = false;
});

it("вход в прежний экран «Анализ связей» лежит в меню «Ещё» с пометкой «прежний экран» (ISS-55)", async () => {
  component = mount(PhysicalGraph, { target });
  await settle();
  expect(target.querySelector('[aria-label="Анализ связей: пути, влияние, контекст"]')).toBeNull();
  expect(target.querySelector('[role="menu"]')).toBeNull();
  target.querySelector<HTMLButtonElement>('[aria-label="Ещё"]')!.click();
  await settle();
  const item = target.querySelector<HTMLButtonElement>('[role="menuitem"]')!;
  expect(item.textContent).toContain("Анализ связей");
  expect(item.textContent).toContain("прежний экран");
  app.graphAnalysis = false;
  item.click();
  await settle();
  expect(app.graphAnalysis).toBe(true);
  expect(target.querySelector('[role="menu"]')).toBeNull();
  app.graphAnalysis = false;
});

it("«Открыть связи и основания» из выдачи включает вид запроса, даже если из него вышли в общий вид (ISS-55)", async () => {
  component = mount(PhysicalGraph, { target });
  await settle();
  finishSearch("r1", "как работает граф", [3, 7]);
  await settle();
  button("Общий вид")!.click();
  await settle();
  expect(target.querySelector(".qbar")).toBeNull();

  app.graphShowQuery = true;
  await settle();
  expect(target.querySelector(".qbar")).not.toBeNull();
  expect(app.graphShowQuery).toBe(false);
  expect(app.graphAnalysis).toBe(false);
  app.results = null;
  app.displayed = null;
  await settle();
  app.searching = false;
});

it("глубина окружения 0 оставляет запрос и результаты, пока идёт новый поиск прежний вид остаётся", async () => {
  component = mount(PhysicalGraph, { target });
  await settle();
  finishSearch("r1", "запрос", [3, 7]);
  await settle();
  const depth1 = Number(panel().dataset.nodes);
  [...target.querySelectorAll<HTMLButtonElement>(".depth button")].find((b) => b.textContent === "0")!.click();
  await settle();
  expect(panel().dataset.nodes).toBe("3");
  expect(depth1).toBeGreaterThan(3);
  // Поиск начался: результаты окна обнулены, но вид остаётся до завершения.
  app.searching = true;
  app.results = null;
  await settle();
  expect(target.querySelector(".qbar")).not.toBeNull();
  app.results = null;
  app.displayed = null;
  app.searching = false;
  await settle();
  expect(target.querySelector(".qbar")).toBeNull();
});

it("Rust получает область завершённого поиска, смена режима связей не вызывает IPC", async () => {
  component = mount(PhysicalGraph, { target }); await settle();
  finishSearch("projection", "индексация", [3, 7]); await settle();
  const calls = () => rpc.mock.calls.filter(c => c[0] === "graph_query_slice");
  expect(calls()).toHaveLength(1);
  expect(calls()[0][1].request).toMatchObject({id:"projection",scope:{modules:[],files:[]},depth:1,fanout:4,max_nodes:200});
  target.querySelector<HTMLButtonElement>('[role="tab"][aria-selected="false"]')?.click(); await settle();
  expect(calls()).toHaveLength(1);
});
it("поздний ответ прежнего поиска не заменяет новый граф", async () => {
  component = mount(PhysicalGraph, { target }); await settle();
  const previous = rpc.getMockImplementation()!;
  let finish!: (value: unknown) => void;
  let oldParams: Record<string, unknown> = {};
  rpc.mockImplementation(async (method: string, params: Record<string, unknown>) => {
    if (method === "graph_query_slice" && (params.request as {id:string}).id === "old") { oldParams=params; return new Promise(resolve => {finish=resolve;}); }
    return previous(method, params);
  });
  finishSearch("old", "старый запрос", [3]); await settle();
  finishSearch("new", "новый запрос", [7]); await settle();
  expect(target.querySelector(".qbar")?.textContent).toContain("новый запрос");
  finish(queryFixture(oldParams, documents)); await settle();
  expect(target.querySelector(".qbar")?.textContent).toContain("новый запрос");
});
it("ошибка нового среза видна, предыдущий граф и результаты остаются", async () => {
  component = mount(PhysicalGraph, { target }); await settle();
  finishSearch("kept", "сохранённый запрос", [3]); await settle();
  const previous = rpc.getMockImplementation()!;
  rpc.mockImplementation(async (method: string, params: Record<string, unknown>) => {
    if (method === "graph_query_slice") throw new BarsError("снимок графа устарел", "snapshot_expired");
    return previous(method,params);
  });
  const depth = target.querySelector<HTMLButtonElement>('.qbar [role="radio"][aria-checked="false"]')!;
  depth.click(); await settle();
  expect(target.querySelector('[role="alert"]')?.textContent).toContain("снимок графа устарел");
  expect(target.querySelector(".qbar")?.textContent).toContain("сохранённый запрос");
});
it("возврат к уже загруженной глубине отменяет поздний ответ новой глубины", async () => {
  component=mount(PhysicalGraph,{target});await settle();finishSearch("depth-race","индекс",[3]);await settle();
  const original=Number(panel().dataset.nodes);
  const previous=rpc.getMockImplementation()!;let finish!: (v:unknown)=>void;let pending:Record<string,unknown>={};
  rpc.mockImplementation(async (method:string,params:Record<string,unknown>)=>{
    if(method==="graph_query_slice"){pending=params;return new Promise(resolve=>{finish=resolve;});}return previous(method,params);
  });
  const depths=()=>[...target.querySelectorAll<HTMLButtonElement>('.qbar .depth button')];
  depths().find(b=>b.textContent==='0')!.click();await settle();
  depths().find(b=>b.textContent==='1')!.click();await settle();
  finish(queryFixture(pending,documents));await settle();
  expect(Number(panel().dataset.nodes)).toBe(original);
});

it("возврат из запроса восстанавливает выбранный узел общего вида", async () => {
  component = mount(PhysicalGraph, {target}); await settle();
  app.graphFocus = "doc-3"; await settle();
  expect(target.querySelector(".inspector h3")?.textContent).toContain("Документ 3");
  finishSearch("return-selection", "очередь", [7]); await settle();
  button("Общий вид")!.click(); await settle();
  expect(target.querySelector(".inspector h3")?.textContent).toContain("Документ 3");
});
it("явная карточка выбирает результат именно в срезе запроса", async () => {
  component = mount(PhysicalGraph, {target}); await settle();
  finishSearch("card-focus", "очередь", [7]); await settle();
  button("Общий вид")!.click(); await settle();
  app.graphFocus = searchHit(7, .8).path;
  app.graphShowQuery = true;
  await settle();
  expect(app.graphFocus).toBeNull();
  expect(target.querySelector(".inspector h3")?.textContent).toContain("Документ 7");
});

it("полный граф открывается на открытом документе: узел выбран, действия над документом доступны по причине (ISS-58)", async () => {
  app.activeDocument = "docs/features/doc-04.md";
  component = mount(PhysicalGraph, { target });
  await settle();
  expect(target.querySelector(".inspector h3")?.textContent).toContain("Документ 4");
  const row = target.querySelector('[aria-label="Действия над документом"]')!;
  expect(row.textContent).toContain("Документ 4");
  expect([...row.querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Что рядом", "Кто ссылается", "Путь до…", "Что затронет правка"]);
  // Действия ещё не подключены к карте: недоступны, причина — в подсказке.
  expect(row.querySelector<HTMLButtonElement>('[data-action="nearby"]')!.disabled).toBe(true);
  expect(row.querySelector<HTMLButtonElement>('[data-action="nearby"]')!.title).toContain("не подключено");
  // Смена активного документа уже открытую вкладку не перебрасывает.
  app.activeDocument = "docs/manual/doc-05.md";
  await settle();
  expect(target.querySelector(".inspector h3")?.textContent).toContain("Документ 4");
});

it("без документа вкладка открывается картой проекта и подсказывает, что выбрать; завершённый поиск важнее документа (OQ-204)", async () => {
  component = mount(PhysicalGraph, { target });
  await settle();
  expect(target.querySelector(".inspector h3")?.textContent).toBe("Выберите документ");
  expect(target.querySelector(".entry-hint")?.textContent).toContain("Выберите документ на карте");
  expect([...target.querySelectorAll<HTMLButtonElement>(".entry-actions button")].every((b) => b.disabled)).toBe(true);
  await unmount(component);
  component = null;
  app.activeDocument = "docs/adr/doc-03.md";
  finishSearch("s1", "очередь", [2, 5]);
  component = mount(PhysicalGraph, { target });
  await settle();
  expect(target.querySelector(".qbar")).not.toBeNull();
  expect(target.querySelector(".inspector h3")?.textContent).not.toContain("Документ 3");
});

it("«Обновить граф» и «Остановить построение» — разные кнопки; во время построения доступна только остановка (ISS-56)", async () => {
  rpc.mockImplementation(async (method: string, params: Record<string, unknown>) => {
    if (method === "graph_status") return { state: "building", freshness: "stale", completeness: "unknown", job_state: "running", snapshot: "mock-snapshot-0001", active_job: "job-7", source_count: 36, error: null, method: "x" };
    if (method === "graph_physical") return physicalFixture(params);
    return {};
  });
  component = mount(PhysicalGraph, { target });
  await settle();
  expect(button("Обновить граф")!.disabled).toBe(true);
  expect(button("Остановить построение")!.disabled).toBe(false);
  button("Остановить построение")!.click();
  await settle();
  expect(rpc.mock.calls.some((c) => c[0] === "graph_cancel" && c[1].job_id === "job-7")).toBe(true);
  expect(rpc.mock.calls.some((c) => c[0] === "graph_build")).toBe(false);
});

it("служебные сведения и слой обновления лежат в настройках графа, а не на первом экране", async () => {
  component = mount(PhysicalGraph, { target });
  await settle();
  expect(target.textContent).not.toContain("Снимок графа");
  expect(target.querySelector('[aria-label="Слой для обновления"]')).toBeNull();
  target.querySelector<HTMLButtonElement>('[aria-label="Настройки графа"]')!.click();
  await settle();
  expect(target.querySelector('[aria-label="Настройки графа"].pop, section[aria-label="Настройки графа"]')).not.toBeNull();
  expect(target.querySelector(".facts")?.textContent).toContain("Снимок графа");
  expect(target.querySelector('[aria-label="Слой для обновления"]')).not.toBeNull();
});
