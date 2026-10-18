import { expect, it } from "vitest";
import { NotificationCenter } from "./notifications";
import type { ActivityOutput, ModelsInfoOutput, RunningJob } from "./types";

const models = (downloading: boolean, percent: number, downloaded = false, error: string | null = null): ModelsInfoOutput => ({
  runtime: null, models_dir: "test", rerank: [],
  embed: [{ id: "bge", dim: 1024, backend: "onnx", warnings: [], downloading, downloaded, download_percent: percent, download_error: error }],
});
const job: RunningJob = { project: "a", name: "Альфа", kind: "check", priority: "explicit", docs_done: 2, docs_total: 10, chunks_done: 3, chunks_per_second: 1, elapsed_seconds: 2, current_file: null, embed_device: null, attempt: 1 };
const activity = (running: RunningJob | null = null): ActivityOutput => ({ running, paused: false, queue: [], problems: [] });

it("updates one model card, preserving read state until completion", () => {
  const center = new NotificationCenter();
  center.models(models(true, 42)); center.read(); center.models(models(true, 64));
  expect(center.items).toHaveLength(1);
  expect(center.items[0]).toMatchObject({ percent: 64, active: true, unread: false });
  center.models(models(true, 100, true));
  expect(center.items[0].text).toContain("подготовка");
  center.models(models(false, 100, true));
  expect(center.items[0]).toMatchObject({ active: false, bad: false, unread: true });
  center.read(); center.models(models(false, 100, true));
  expect(center.items[0].unread).toBe(false);
});

it("does not report success for incomplete downloads or errors with cached weights", () => {
  const center = new NotificationCenter();
  center.models(models(true, 42)); center.models(models(false, 42));
  expect(center.items[0].bad).toBe(true);
  center.models(models(true, 100, true)); center.models(models(false, 100, true, "Сбой"));
  expect(center.items[0]).toMatchObject({ bad: true, text: "Сбой", active: false });
});

it("keeps active work on clear and suppresses unchanged cleared errors", () => {
  const center = new NotificationCenter();
  center.models(models(true, 30)); center.error("failure", "Ошибка", "Текст");
  center.clear(); center.error("failure", "Ошибка", "Текст");
  expect(center.items).toHaveLength(1);
  expect(center.items[0].active).toBe(true);
  center.error("failure", "Ошибка", "Другой текст");
  expect(center.items).toHaveLength(2);
});

it("tracks queue, pause, problems and recurrence without claiming success on cancel", () => {
  const center = new NotificationCenter();
  center.activity({ ...activity(job), queue: [{ project: "b", name: "Бета", kind: "check", priority: "explicit", files: 5 }] });
  expect(center.items.filter(n => n.active)).toHaveLength(2);
  center.read(); center.activity({ ...activity(job), paused: true });
  expect(center.items.find(n => n.project === "a")?.text).toContain("Пауза");
  center.activity(activity());
  expect(center.items.some(n => n.active)).toBe(false);
  expect(center.items.some(n => /успеш/i.test(n.title))).toBe(false);
  const failed = { ...activity(), problems: [{ project: "a", name: "Альфа", files: [], last_error: "Ошибка файла", rebuild_not_applied: true }] };
  center.activity(failed); center.read(); center.activity(failed);
  expect(center.items.find(n => n.id === "index-error:a")?.unread).toBe(false);
  center.clear(); center.activity(failed);
  expect(center.items).toHaveLength(0);
  center.activity(activity()); center.activity(failed);
  expect(center.items[0].unread).toBe(true);
});

it("bounds completed history without evicting active operations", () => {
  const center = new NotificationCenter(); center.models(models(true, 1));
  for (let i = 0; i < 150; i++) center.error(`error:${i}`, "Ошибка", `${i}`);
  expect(center.items).toHaveLength(101);
  expect(center.items.some(n => n.active)).toBe(true);
});
