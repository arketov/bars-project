// Вход в полный граф (2.3.5): строка действий над документом и кнопки состояния графа. Чистая логика без DOM:
// что доступно, почему недоступно и какой текст у кнопки. Дизайн — docs/features/graphrag/graph-actions-design.md.
import type { GraphStatus } from "../graph";

export type ActionId = "nearby" | "inbound" | "path" | "impact";

export interface GraphAction {
  id: ActionId;
  label: string;
  /** Что получит пользователь; показывается подсказкой. */
  hint: string;
  /** Действие подключено к карте; остальные видны, но недоступны с причиной до своего шага переноса. */
  ready: boolean;
}

export const GRAPH_ACTIONS: GraphAction[] = [
  { id: "nearby", label: "Что рядом", hint: "Документ и его соседи, глубина 1–3", ready: false },
  { id: "inbound", label: "Кто ссылается", hint: "Входящие ссылки с путями и строками", ready: false },
  { id: "path", label: "Путь до…", hint: "Путь по карте, не зависимость", ready: false },
  { id: "impact", label: "Что затронет правка", hint: "Документированные зависимости; ссылки-соседи отдельно", ready: false },
];

export const NO_SELECTION_TEXT = "Выберите документ на карте или найдите его по названию — граф покажет, что с ним связано.";
const NOT_CONNECTED = "Действие ещё не подключено к карте; оно появится в следующем обновлении графа.";

export interface ActionState {
  enabled: boolean;
  /** Причина недоступности или пояснение действия — для подсказки. */
  reason: string;
}

/** Доступность действия для выбранного объекта; `kind` — вид узла или `null`, если выбрано не узел. */
export function actionState(action: GraphAction, kind: "document" | "chunk" | "query" | null): ActionState {
  if (kind === null) return { enabled: false, reason: "Выберите документ на карте" };
  if (kind !== "document") return { enabled: false, reason: kind === "query" ? "Действие относится к документу, а не к запросу" : "Действие относится к документу: выберите его владельца" };
  if (!action.ready) return { enabled: false, reason: NOT_CONNECTED };
  return { enabled: true, reason: action.hint };
}

export interface BuildControls {
  /** Подпись кнопки запуска: «Обновить граф», «Продолжить» после остановки, «Повторить» после ошибки. */
  updateLabel: string;
  updateEnabled: boolean;
  updateReason: string;
  stopEnabled: boolean;
  stopReason: string;
  /** Идёт построение: запуск недоступен, остановка доступна. */
  building: boolean;
  /** Причина ожидания устаревшего слоя, если воркер её назвал. */
  staleReason: string | null;
}

const NEEDS_UPDATE_STATES = new Set(["stale", "failed", "cancelled", "not_built"]);

/** Какие из двух кнопок («Обновить граф», «Остановить построение») доступны; кнопка запуска — только когда она что-то меняет (ISS-56). */
export function buildControls(status: GraphStatus | null): BuildControls {
  const none = { updateLabel: "Обновить граф", updateEnabled: false, stopEnabled: false, building: false, staleReason: null as string | null };
  if (!status) return { ...none, updateReason: "Состояние графа ещё не прочитано", stopReason: "Построение не идёт" };
  if (status.availability === "disabled") return { ...none, updateReason: "Граф выключен в настройках проекта", stopReason: "Построение не идёт" };
  const building = !!status.active_job || status.state === "building" || status.job_state === "running" || status.job_state === "queued";
  const staleReason = Object.values(status.layers ?? {}).find((layer) => layer.freshness === "stale" && layer.stale_reason)?.stale_reason ?? null;
  if (building) return { ...none, building: true, staleReason, updateReason: "Идёт построение: сначала остановите его или дождитесь конца", stopEnabled: !!status.active_job, stopReason: status.active_job ? "Остановить текущее построение; автообновление будущих изменений остаётся" : "Построение стоит в очереди и ещё не началось" };
  const stopped = status.job_state === "stopped";
  const failed = status.job_state === "failed";
  const needs = stopped || failed || status.freshness === "stale" || status.freshness === "not_built" || NEEDS_UPDATE_STATES.has(status.state);
  const updateLabel = stopped ? "Продолжить" : failed ? "Повторить" : "Обновить граф";
  const updateReason = needs
    ? stopped ? "Продолжить остановленное построение" : failed ? "Повторить построение, закончившееся ошибкой" : "Перестроить выбранный слой"
    : "Граф актуален. Повтор не делает неизвестную полноту отношений известной";
  return { updateLabel, updateEnabled: needs, updateReason, stopEnabled: false, stopReason: "Построение не идёт", building: false, staleReason };
}
