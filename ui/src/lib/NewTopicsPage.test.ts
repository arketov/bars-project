import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushSync, mount, unmount, type Component } from "svelte";
import NewTopicsPage from "./NewTopicsPage.svelte";
import Sidebar from "./Sidebar.svelte";
import { app } from "./app.svelte";
import { call } from "./api";
import type { TopicDetail, TopicSummary, TopicsList } from "./new-topics";
import { BarsError } from "./types";

vi.mock("./api", () => ({ call: vi.fn(), joinPath: vi.fn(), openPath: vi.fn(), pickFolder: vi.fn(), copyText: vi.fn(), folderTarget: vi.fn(), folderAction: vi.fn(), documentEditors: vi.fn(async () => []), documentAction: vi.fn(), inTauri: () => false }));
const invoke = vi.mocked(call);
const project = { name: "Проект", path: "C:/проект", docs_root: "docs", state: "ready" as const, modules: [] };
const mounted: ReturnType<typeof mount>[] = [];
const settle = async () => { for (let i = 0; i < 4; i++) { await new Promise((resolve) => setTimeout(resolve, 0)); flushSync(); } };
const summary = (id: number, over: Partial<TopicSummary> = {}): TopicSummary => ({
  id, revision: 1, title: `Вопрос ${id}`, status: "open", first_seen: 1_700_000_000, last_seen: 1_700_000_000 + id, observations: 2,
  sources: ["mcp"], reasons: ["empty_result"], variants: 1, ...over,
});
const detailOf = (topic: TopicSummary): TopicDetail => ({
  ...topic,
  variants_list: [{ id: topic.id * 10, text: topic.title, scope: "m=features;f=" }],
  checks: topic.status === "materials_found"
    ? [{ id: 1, topic_revision: topic.revision, at: 1_700_000_100, expanded: false, model: "m", params: "{}", conclusive: true, results: [{ variant_id: topic.id * 10, scope: "m=features;f=", outcome: "found", hits: [{ path: "docs/a.md", title: "Документ А", heading_chain: "А/Раздел", first_line: 3, last_line: 9, score: 0.8, score_kind: "rerank" }] }] }]
    : [],
});
let topics: TopicSummary[];
let enabled: boolean;
let listError: BarsError | null;

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function show(component: Component<any>) {
  const host = document.createElement("div");
  document.body.append(host);
  mounted.push(mount(component, { target: host }));
  flushSync();
  return host;
}
const button = (host: ParentNode, text: string) => [...host.querySelectorAll("button")].find((b) => b.textContent?.trim().startsWith(text)) as HTMLButtonElement | undefined;

beforeEach(() => {
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute("open", ""); };
  topics = [summary(1), summary(2, { status: "materials_found", revision: 3 }), summary(3, { status: "resolved" })];
  enabled = true;
  listError = null;
  app.projects = [project];
  app.current = project.path;
  app.topicsOpen = true;
  app.topics.bind(null);
  app.topics.bind(project.path);
  invoke.mockImplementation(async (method, params) => {
    const id = Number((params as { id?: number }).id);
    switch (method) {
      case "new_topics_list": {
        if (listError) throw listError;
        const list: TopicsList = { enabled, generation: 1, topics, store_path: "C:/проект/.bars/new-topics.sqlite3", losses: 0 };
        return list;
      }
      case "new_topics_get": return detailOf(topics.find((t) => t.id === id)!);
      case "new_topics_check": return detailOf(topics.find((t) => t.id === id)!);
      case "new_topics_delete": topics = topics.filter((t) => t.id !== id); return { accepted: true };
      case "new_topics_set_status": return { revision: 9 };
      case "new_topics_settings": enabled = Boolean((params as { enabled: boolean }).enabled); return { enabled, generation: 2 };
      default: throw new Error(`unexpected ${method}`);
    }
  });
});
afterEach(async () => {
  for (const component of mounted.splice(0)) await unmount(component);
  app.topics.bind(null);
  app.view = "main";
  app.topicsOpen = false;
  app.projects = [];
  app.current = null;
  app.results = null;
  app.displayed = null;
  document.body.replaceChildren();
  vi.clearAllMocks();
});

it("показывает список, по умолчанию только активные, и карточку выбранной темы", async () => {
  const host = show(NewTopicsPage);
  await settle();
  const rows = [...host.querySelectorAll(".topic")];
  expect(rows).toHaveLength(2);
  expect(host.textContent).not.toContain("Вопрос 3");
  expect(host.textContent).toContain("2 обращения");
  (rows[0] as HTMLButtonElement).click();
  await settle();
  expect(host.querySelector("h2")?.textContent).toContain("Вопрос 2");
  // Найденные материалы с путём и строками, решение остаётся за человеком.
  expect(host.textContent).toContain("docs/a.md:3–9");
  expect(button(host, "Знаний достаточно")).toBeDefined();
  expect(button(host, "Всё ещё не хватает")).toBeDefined();
  expect(host.textContent).toContain("Совпадения сами по себе не закрывают тему");
});

it("переключение фильтра показывает закрытые темы", async () => {
  const host = show(NewTopicsPage);
  await settle();
  button(host, "Закрытые")!.click();
  flushSync();
  expect([...host.querySelectorAll(".topic")].map((row) => row.textContent)).toEqual([expect.stringContaining("Вопрос 3")]);
});

it("пустой список объясняет, что здесь появится, а ошибка чтения не выглядит пустым списком", async () => {
  topics = [];
  const host = show(NewTopicsPage);
  await settle();
  expect(host.textContent).toContain("Новых тем пока нет");
  expect(host.textContent).toContain("Содержимое внешних чатов не читается");
  expect(host.querySelector('[role="alert"]')).toBeNull();

  app.topics.bind(null);
  app.topics.bind(project.path);
  listError = new BarsError("не удалось открыть хранилище");
  await app.topics.load();
  await settle();
  const alert = host.querySelector('[role="alert"]')!;
  expect(alert.textContent).toContain("Не удалось прочитать список");
  expect(alert.textContent).toContain("не удалось открыть хранилище");
  expect(host.textContent).not.toContain("Новых тем пока нет");
  listError = null;
  button(alert as HTMLElement, "Повторить")!.click();
  await settle();
  expect(host.querySelector('[role="alert"]')).toBeNull();
});

it("выключенный сбор виден постоянной плашкой, включается кнопкой, старые темы остаются", async () => {
  enabled = false;
  const host = show(NewTopicsPage);
  await settle();
  expect(host.textContent).toContain("Сбор новых тем выключен");
  expect(host.querySelectorAll(".topic")).toHaveLength(2);
  button(host, "Включить сбор")!.click();
  await settle();
  expect(invoke).toHaveBeenCalledWith("new_topics_settings", { project: project.path, enabled: true });
  expect(host.textContent).not.toContain("Сбор новых тем выключен");
});

it("проверка идёт с ревизией темы; «весь проект» — отдельная явная кнопка при исходной области", async () => {
  const host = show(NewTopicsPage);
  await settle();
  (host.querySelectorAll(".topic")[1] as HTMLButtonElement).click();
  await settle();
  button(host, "Проверить знания")!.click();
  await settle();
  expect(invoke).toHaveBeenCalledWith("new_topics_check", { project: project.path, id: 1, revision: 1, expand: false });
  button(host, "Проверить по всему проекту")!.click();
  await settle();
  expect(invoke).toHaveBeenCalledWith("new_topics_check", { project: project.path, id: 1, revision: 1, expand: true });
});

it("удаление просит подтверждения: фокус на «Отмена», отмена ничего не удаляет", async () => {
  const host = show(NewTopicsPage);
  await settle();
  (host.querySelectorAll(".topic")[0] as HTMLButtonElement).click();
  await settle();
  button(host, "Удалить…")!.click();
  flushSync();
  const dialog = document.querySelector("dialog")!;
  expect(dialog.textContent).toContain("Новый запрос может добавить её снова");
  expect(document.activeElement?.textContent).toBe("Отмена");
  dialog.dispatchEvent(new Event("cancel", { cancelable: true }));
  flushSync();
  expect(document.querySelector("dialog")).toBeNull();
  expect(invoke.mock.calls.some(([method]) => method === "new_topics_delete")).toBe(false);

  button(host, "Удалить…")!.click();
  flushSync();
  button(document.querySelector("dialog")!, "Удалить")!.click();
  await settle();
  expect(invoke).toHaveBeenCalledWith("new_topics_delete", { project: project.path, id: 2, revision: 3 });
  expect(host.textContent).not.toContain("Вопрос 2");
});

it("«Проверить все» показывает N из M и позволяет остановиться", async () => {
  const host = show(NewTopicsPage);
  await settle();
  const gates: Array<() => void> = [];
  invoke.mockImplementation(async (method, params) => {
    if (method === "new_topics_check") { await new Promise<void>((resolve) => gates.push(resolve)); return detailOf(topics.find((t) => t.id === (params as { id: number }).id)!); }
    return { enabled, generation: 1, topics, store_path: "p", losses: 0 } satisfies TopicsList;
  });
  button(host, "Проверить все")!.click();
  await settle();
  expect(host.querySelector(".progress")?.textContent).toContain("1 из 2");
  button(host, "Остановить")!.click();
  flushSync();
  gates[0]();
  await settle();
  expect(host.querySelector(".progress")).toBeNull();
  expect(gates).toHaveLength(1);
});

it("первая строка дерева «Новые темы» стоит выше корня документации, не зависит от фильтра документов и открывает экран", async () => {
  app.view = "main";
  app.topicsOpen = false;
  app.tree = [];
  app.topics.bind(null);
  app.topics.bind(project.path);
  const host = show(Sidebar);
  await app.topics.load(true);
  await settle();
  const row = host.querySelector<HTMLButtonElement>(".topics-row")!;
  const docsRoot = host.querySelector(".docs-root")!;
  expect(row.compareDocumentPosition(docsRoot) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(row.querySelector("input[type=checkbox]")).toBeNull();
  expect(row.getAttribute("aria-label")).toBe("Новые темы: активных 2");
  app.filter = "нет такого документа";
  flushSync();
  expect(host.querySelector(".topics-row")).not.toBeNull();
  const selectedBefore = app.selected;
  row.click();
  flushSync();
  expect(app.view).toBe("main");
  expect(app.topicsOpen).toBe(true);
  expect(app.selected).toBe(selectedBefore);
  expect(host.querySelector(".topics-row")?.getAttribute("aria-current")).toBe("true");
  app.filter = "";
});

it("счётчик строки при ошибке чтения — прочерк, а не ноль", async () => {
  app.view = "main";
  listError = new BarsError("нет доступа");
  app.topics.bind(null);
  app.topics.bind(project.path);
  const host = show(Sidebar);
  await app.topics.load(true);
  await settle();
  expect(host.querySelector(".topics-row .ct")?.textContent).toBe("—");
});
