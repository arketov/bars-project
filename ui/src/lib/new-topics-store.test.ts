import { beforeEach, expect, it, vi } from "vitest";
import { call } from "./api";
import { NewTopicsStore } from "./new-topics.svelte";
import type { TopicDetail, TopicSummary, TopicsList } from "./new-topics";
import { BarsError } from "./types";

vi.mock("./api", () => ({ call: vi.fn() }));
const invoke = vi.mocked(call);
const P = "C:/проект";
const summary = (id: number, over: Partial<TopicSummary> = {}): TopicSummary => ({
  id, revision: 1, title: `Тема ${id}`, status: "open", first_seen: 1, last_seen: id, observations: 1,
  sources: ["window"], reasons: ["empty_result"], variants: 1, ...over,
});
const detailOf = (topic: TopicSummary): TopicDetail => ({ ...topic, variants_list: [{ id: topic.id * 10, text: topic.title, scope: "project" }], checks: [] });
const listOf = (...topics: TopicSummary[]): TopicsList => ({ enabled: true, generation: 1, topics, store_path: `${P}/.bars/new-topics.sqlite3`, losses: 0 });
let topics: TopicSummary[];
let flash: ReturnType<typeof vi.fn<(text: string, bad?: boolean) => void>>;
let store: NewTopicsStore;
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

beforeEach(() => {
  vi.clearAllMocks();
  topics = [summary(1), summary(2, { status: "materials_found", revision: 3 })];
  flash = vi.fn<(text: string, bad?: boolean) => void>();
  store = new NewTopicsStore(flash);
  store.bind(P);
  invoke.mockImplementation(async (method, params) => {
    const id = Number((params as { id?: number }).id);
    switch (method) {
      case "new_topics_list": return listOf(...topics);
      case "new_topics_get": return detailOf(topics.find((t) => t.id === id)!);
      case "new_topics_check": return detailOf(topics.find((t) => t.id === id)!);
      case "new_topics_delete": topics = topics.filter((t) => t.id !== id); return { accepted: true };
      case "new_topics_clear": topics = []; return { generation: 2 };
      case "new_topics_set_status": return { revision: 9 };
      default: throw new Error(`unexpected ${method}`);
    }
  });
});

it("ошибка чтения не выглядит нулём: счётчик пуст, список не подменяется пустым", async () => {
  invoke.mockRejectedValueOnce(new BarsError("доступ запрещён"));
  await store.load();
  expect(store.error).toBe("доступ запрещён");
  expect(store.count).toBeNull();
  expect(store.list).toBeNull();
  await store.load();
  expect(store.error).toBeNull();
  expect(store.count).toBe(2);
});

it("ответ прежнего проекта после смены проекта не показывается", async () => {
  let release!: (value: TopicsList) => void;
  invoke.mockImplementationOnce(() => new Promise<TopicsList>((resolve) => { release = resolve; }));
  const loading = store.load();
  store.bind("C:/другой");
  release(listOf(summary(7, { title: "Чужая" })));
  await loading;
  expect(store.list).toBeNull();
  expect(store.project).toBe("C:/другой");
});

it("позднее обновление списка не затирает более новое", async () => {
  let releaseOld!: (value: TopicsList) => void;
  invoke.mockImplementationOnce(() => new Promise<TopicsList>((resolve) => { releaseOld = resolve; }));
  const old = store.load(true);
  topics = [summary(5)];
  await store.load(true);
  releaseOld(listOf(summary(1), summary(2)));
  await old;
  expect(store.topics.map((t) => t.id)).toEqual([5]);
});

it("выбор темы читает карточку; исчезнувшая тема после обновления списка снимает выбор", async () => {
  await store.load();
  await store.select(1);
  expect(store.detail?.id).toBe(1);
  topics = [summary(2, { status: "materials_found", revision: 3 })];
  await store.load(true);
  expect(store.selectedId).toBeNull();
  expect(store.detail).toBeNull();
});

it("проверка идёт с ожидаемой ревизией; повторный запуск той же темы блокируется", async () => {
  await store.load();
  await store.select(2);
  let release!: (value: TopicDetail) => void;
  invoke.mockImplementationOnce(() => new Promise<TopicDetail>((resolve) => { release = resolve; }));
  const first = store.check(2);
  expect(store.checking.has(2)).toBe(true);
  expect(await store.check(2)).toBe(false);
  release(detailOf(topics[1]));
  expect(await first).toBe(true);
  expect(store.checking.has(2)).toBe(false);
  expect(invoke).toHaveBeenCalledWith("new_topics_check", { project: P, id: 2, revision: 3, expand: false });
  expect(invoke.mock.calls.filter(([m]) => m === "new_topics_check")).toHaveLength(1);
});

it("расширение на весь проект — только явный параметр", async () => {
  await store.load();
  await store.check(1, true);
  expect(invoke).toHaveBeenCalledWith("new_topics_check", { project: P, id: 1, revision: 1, expand: true });
});

it("устаревшая ревизия даёт сообщение и обновление, а не перезапись", async () => {
  await store.load();
  invoke.mockRejectedValueOnce(new BarsError("тема изменилась", "stale_ref", -32012));
  expect(await store.check(1)).toBe(false);
  expect(flash).toHaveBeenCalledWith(expect.stringContaining("изменилась или удалена"), true);
});

it("«Проверить все» идёт по активным темам по одной, показывает N/M и останавливается по отмене", async () => {
  topics = [summary(1), summary(2), summary(3, { status: "resolved" }), summary(4)];
  await store.load();
  const releases: Array<() => void> = [];
  invoke.mockImplementation(async (method, params) => {
    if (method === "new_topics_check") {
      await new Promise<void>((resolve) => releases.push(resolve));
      return detailOf(topics.find((t) => t.id === (params as { id: number }).id)!);
    }
    if (method === "new_topics_list") return listOf(...topics);
    throw new Error(`unexpected ${method}`);
  });
  const running = store.checkAll();
  await settle();
  expect(store.batch).toMatchObject({ done: 0, total: 3 });
  expect(releases).toHaveLength(1);
  releases[0]();
  await settle(); await settle();
  expect(store.batch).toMatchObject({ done: 1, total: 3 });
  store.cancelBatch();
  releases[1]();
  await running;
  const checked = invoke.mock.calls.filter(([m]) => m === "new_topics_check").map(([, p]) => (p as { id: number }).id);
  expect(checked).toEqual([1, 2]);
  expect(store.batch).toBeNull();
  expect(flash).toHaveBeenCalledWith(expect.stringContaining("остановлена"));
});

it("удаление и очистка сбрасывают выбор и перечитывают список", async () => {
  await store.load();
  await store.select(1);
  expect(await store.remove(1)).toBe(true);
  expect(store.selectedId).toBeNull();
  expect(store.topics.map((t) => t.id)).toEqual([2]);
  expect(invoke).toHaveBeenCalledWith("new_topics_delete", { project: P, id: 1, revision: 1 });
  expect(await store.clear()).toBe(true);
  expect(invoke).toHaveBeenCalledWith("new_topics_clear", { project: P, confirmed: true });
  expect(store.topics).toEqual([]);
});

it("ручная отметка отправляет контекст отображённой выдачи и обновляет список", async () => {
  invoke.mockImplementation(async (method) => {
    if (method === "new_topics_mark_missing") return { topic_id: 8, created: true };
    return listOf();
  });
  const created = await store.markMissing({ requestId: "window:r1", query: "Как это работает?", modules: ["features"], files: [] });
  expect(created).toBe(true);
  expect(invoke).toHaveBeenCalledWith("new_topics_mark_missing", { project: P, request_id: "window:r1", query: "Как это работает?", modules: ["features"], files: [] });
});

it("отказ воркера при отметке показывается и не помечает обращение отмеченным", async () => {
  invoke.mockRejectedValueOnce(new BarsError("сбор выключен", "invalid_params", -32007));
  expect(await store.markMissing({ requestId: "window:r2", query: "q", modules: [], files: [] })).toBeNull();
  expect(flash).toHaveBeenCalledWith("сбор выключен", true);
});
