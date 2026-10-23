import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { app } from "./app.svelte";
import { call } from "./api";
import type { SearchOutput } from "./types";

vi.mock("./api", () => ({ call: vi.fn(), joinPath: vi.fn(), openPath: vi.fn(), pickFolder: vi.fn(), inTauri: () => false }));
const invoke = vi.mocked(call);
const project = { name: "П", path: "C:/проект", docs_root: "docs", state: "ready" as const, modules: [] };
const found: SearchOutput = {
  results: [{ path: "docs/a.md", module: "docs", title: "A", heading_chain: "A", first_line: 1, last_line: 2, text: "t", score: 0.7, score_kind: "rerank" }],
  applied: { results: 5, candidates: 30, rerank_candidates: 15, min_score: 0.1, score_kind: "rerank", clamped: false },
  indexing: false,
  pending: 0,
};
const listAnswer = { enabled: true, generation: 1, topics: [], store_path: "p", losses: 0 };

beforeEach(() => {
  app.projects = [project];
  app.current = project.path;
  app.topics.bind(project.path);
  app.tree = [];
  app.selected = new Set();
  app.retrievalMode = 'hybrid'; app.count = null; app.minScore = null;
  invoke.mockImplementation(async (method) => {
    if (method === "search") return found;
    if (method === "new_topics_mark_missing") return { topic_id: 1, created: true };
    if (method === "new_topics_list") return listAnswer;
    throw new Error(`unexpected ${method}`);
  });
});

it("поздний поиск не заменяет новый; снимок сохраняет отправленные параметры", async () => {
  const requests: ((value: unknown) => void)[] = [];
  invoke.mockImplementation((method) => method === 'text_search' ? Promise.resolve({documents:[],docs_total:0,hits_total:0,skipped:[],truncated:null,elapsed_ms:0}) : new Promise(done => requests.push(done)));
  app.query = 'первый'; const first = app.search();
  app.query = 'второй'; app.count = 7; const second = app.search();
  app.query = 'черновик'; app.count = 11;
  requests[1](found); await second;
  expect(app.displayed).toMatchObject({ query: 'второй', limit: 7, project: project.path, mode: 'hybrid' });
  const id = app.displayed!.requestId;
  requests[0]({ ...found, results: [] }); await first;
  expect(app.displayed!.requestId).toBe(id); expect(app.results!.results).toHaveLength(1);
});

it("новый поиск убирает старую выдачу; смена проекта отклоняет поздний ответ", async () => {
  app.query = 'первый'; await app.search();
  let resolve!: (value: unknown) => void;
  invoke.mockImplementation((method) => method === 'text_search' ? Promise.resolve({documents:[],docs_total:0,hits_total:0,skipped:[],truncated:null,elapsed_ms:0}) : new Promise(done => resolve = done));
  const pending = app.search();
  expect(app.results).toBeNull(); expect(app.displayed).toBeNull(); expect(app.searching).toBe(true);
  await app.select(null);
  resolve(found); await pending;
  expect(app.results).toBeNull(); expect(app.displayed).toBeNull(); expect(app.searching).toBe(false);
});

it("диагностика пустого ответа использует завершённый запрос, не изменённое поле", async () => {
  let resolve!: (value: unknown) => void;
  invoke.mockImplementation((method) => method === 'text_search' ? Promise.resolve({documents:[],docs_total:0,hits_total:0,skipped:[],truncated:null,elapsed_ms:0}) : new Promise(done => resolve = done));
  app.query = 'исходный'; const pending = app.search();
  app.query = 'изменённый'; invoke.mockImplementation(async method => method === 'search' ? found : listAnswer); resolve({ ...found, results: [] }); await pending;
  const diagnostic = invoke.mock.calls.find(([, args]) => (args as { _window?: { purpose?: string } })?._window?.purpose === 'diagnostic');
  expect(diagnostic?.[1]).toMatchObject({ query: 'исходный' });
});
afterEach(() => {
  app.topics.bind(null);
  app.results = null;
  app.displayed = null;
  app.markedRequest = null;
  app.topicsOpen = false;
  app.viewer = null;
  app.query = "";
  app.projects = [];
  app.current = null;
  vi.clearAllMocks();
});

it("поиск окна передаёт ID обращения, а ручная отметка берёт контекст отображённой выдачи, а не поля ввода", async () => {
  app.query = "  Как  включить сбор  ";
  app.selected = new Set(["docs"]);
  await app.search();
  const searchCall = invoke.mock.calls.find(([method]) => method === "search")!;
  const sent = (searchCall[1] as { _window: { request_id: string } })._window;
  expect(sent.request_id).toMatch(/^window:/);
  expect(app.displayed).toMatchObject({ requestId: sent.request_id, query: "Как  включить сбор" });

  // Пользователь успел поменять поле, но отметка относится к тому, что на экране.
  app.query = "совсем другой вопрос";
  await app.markMissing();
  const mark = invoke.mock.calls.find(([method]) => method === "new_topics_mark_missing")!;
  expect(mark[1]).toMatchObject({ project: project.path, request_id: sent.request_id, query: "Как  включить сбор" });
  expect(app.markedRequest).toBe(sent.request_id);

  // Повторная отметка того же обращения не отправляется.
  await app.markMissing();
  expect(invoke.mock.calls.filter(([method]) => method === "new_topics_mark_missing")).toHaveLength(1);
});

it("новый поиск — новое обращение: отметка снова доступна", async () => {
  app.query = "вопрос";
  await app.search();
  const first = app.displayed!.requestId;
  await app.markMissing();
  await app.search();
  expect(app.displayed!.requestId).not.toBe(first);
  expect(app.markedRequest).toBeNull();
});

it("смена проекта сбрасывает контекст отметки: чужой вопрос в этот проект не попадёт", async () => {
  app.query = "вопрос";
  await app.search();
  expect(app.displayed).not.toBeNull();
  await app.select(null);
  expect(app.displayed).toBeNull();
  await app.markMissing();
  expect(invoke.mock.calls.some(([method]) => method === "new_topics_mark_missing")).toBe(false);
});

it("«Новые темы» открываются как файл: вместо просмотра документа, область поиска и запрос не меняются", () => {
  app.query = "вопрос";
  app.selected = new Set(["docs"]);
  app.viewer = { hit: { path: "docs/a.md", title: "A", first_line: 0, last_line: 0 }, title: "A", text: "", missing: false };
  app.openTopics();
  expect(app.topicsOpen).toBe(true);
  expect(app.viewer).toBeNull();
  expect(app.view).toBe("main");
  expect(app.query).toBe("вопрос");
  expect([...app.selected]).toEqual(["docs"]);
  app.closeTopics();
  expect(app.topicsOpen).toBe(false);
});

it("открытие документа закрывает панель «Новых тем», но выбранная тема в состоянии остаётся", async () => {
  invoke.mockImplementation(async (method) => {
    if (method === "read") return { title: "A", text: "# A" };
    throw new Error(`unexpected ${method}`);
  });
  app.topics.selectedId = 4;
  app.openTopics();
  await app.openDocument({ path: "docs/a.md", title: "A", first_line: 1, last_line: 2 });
  expect(app.topicsOpen).toBe(false);
  expect(app.viewer?.hit.path).toBe("docs/a.md");
  expect(app.topics.selectedId).toBe(4);
  app.closeViewer();
});
