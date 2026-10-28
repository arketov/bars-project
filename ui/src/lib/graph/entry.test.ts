import { expect, it } from "vitest";
import { GRAPH_ACTIONS, actionState, buildControls } from "./entry";
import type { GraphStatus } from "../graph";

const status = (patch: Partial<GraphStatus>): GraphStatus => ({ state: "ready", snapshot: "s", active_job: null, source_count: 3, error: null, method: "x", ...patch });

it("действия документа: без выбора недоступны с причиной, у не-документа — тоже", () => {
  for (const action of GRAPH_ACTIONS) {
    expect(actionState(action, null)).toMatchObject({ enabled: false, reason: "Выберите документ на карте" });
    expect(actionState(action, "chunk").enabled).toBe(false);
    expect(actionState(action, "query").reason).toContain("запрос");
  }
  expect(GRAPH_ACTIONS.map((a) => a.label)).toEqual(["Что рядом", "Кто ссылается", "Путь до…", "Что затронет правка"]);
});

it("действия навигации подключены (2.3.6), «Что затронет правка» ждёт смыслового слоя (2.3.8)", () => {
  const ready = Object.fromEntries(GRAPH_ACTIONS.map((a) => [a.id, a.ready]));
  expect(ready).toEqual({ nearby: true, inbound: true, path: true, impact: false });
  const impact = GRAPH_ACTIONS.find((a) => a.id === "impact")!;
  expect(actionState(impact, "document")).toMatchObject({ enabled: false });
  const [first] = GRAPH_ACTIONS;
  expect(actionState(first, "document")).toEqual({ enabled: true, reason: first.hint });
});

it("кнопка обновления активна только для устаревшего, не построенного, остановленного и упавшего графа (ISS-56)", () => {
  expect(buildControls(status({ freshness: "current", completeness: "partial", job_state: "idle" })).updateEnabled).toBe(false);
  expect(buildControls(status({ freshness: "current", job_state: "idle" })).updateReason).toContain("актуален");
  expect(buildControls(status({ freshness: "stale" }))).toMatchObject({ updateEnabled: true, updateLabel: "Обновить граф" });
  expect(buildControls(status({ freshness: "not_built", state: "not_built" })).updateEnabled).toBe(true);
  expect(buildControls(status({ state: "cancelled" })).updateEnabled).toBe(true);
  expect(buildControls(status({ freshness: "current", job_state: "stopped" }))).toMatchObject({ updateEnabled: true, updateLabel: "Продолжить" });
  expect(buildControls(status({ freshness: "current", job_state: "failed" }))).toMatchObject({ updateEnabled: true, updateLabel: "Повторить" });
});

it("во время построения запуск недоступен, остановка доступна только у активного задания и не обещает паузы автообновления", () => {
  const running = buildControls(status({ job_state: "running", active_job: "job-1" }));
  expect(running).toMatchObject({ building: true, updateEnabled: false, stopEnabled: true });
  expect(running.stopReason).toContain("автообновление");
  const queued = buildControls(status({ job_state: "queued" }));
  expect(queued).toMatchObject({ building: true, stopEnabled: false });
  expect(buildControls(status({ freshness: "stale" })).stopEnabled).toBe(false);
});

it("выключенный или ещё не прочитанный граф не даёт ни запуска, ни остановки; причина ожидания устаревшего слоя берётся из воркера", () => {
  expect(buildControls(null)).toMatchObject({ updateEnabled: false, stopEnabled: false });
  expect(buildControls(status({ availability: "disabled" }))).toMatchObject({ updateEnabled: false, stopEnabled: false });
  const layers = { similarity: { state: "stale", freshness: "stale", stale_reason: "ждём паузу после правок", completeness: "partial", availability: "enabled", generation: null, fingerprint: "", source_revisions: {}, coverage: { unit: "x", processed: 0, total: 0, missing: 0, comparisons: 0, reason: null }, processed: 0, total: 0, failed: 0 } } as GraphStatus["layers"];
  expect(buildControls(status({ freshness: "stale", layers })).staleReason).toBe("ждём паузу после правок");
});
