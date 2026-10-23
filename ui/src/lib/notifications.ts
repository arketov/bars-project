import type { Operation, OperationEventsOutput } from "./types";

export interface Notice {
  id: string; title: string; text: string; active: boolean; bad: boolean; unread: boolean;
  percent: number | null; target?: "models" | "indexing" | "audits"; project?: string;
  operation?: Operation; epoch?: string; unknown?: boolean; updatedAt?: string;
}

export function operationText(op: Operation): string {
  const r = op.report;
  if (op.finished_at) {
    if (op.kind === "scan" && r) {
      const changes = r.added + r.changed + r.removed;
      const errors = r.problems ? ` · ошибок: ${r.problems}` : "";
      return changes || r.problems
        ? `Проверка завершена · найдено: новых ${r.added}, изменённых ${r.changed}, удалённых ${r.removed}${errors} · обновление в очереди`
        : `Проверка завершена · различий не найдено · проверено ${r.unchanged} документов`;
    }
    if (r && !r.index_applied) return `${op.state === "cancelled" ? "Отменено · " : ""}Переиндексация не применена; сохранён прежний индекс`;
    if (op.state === "cancelled") return `Отменено${op.docs_total !== null ? ` · обработано ${op.docs_done ?? 0} из ${op.docs_total} документов` : ""}${r?.index_applied ? " · изменения до остановки применены" : ""}`;
    if (op.state === "error") return op.error ?? "Ошибка операции";
    if (op.kind === "download") return "Файлы модели проверены и готовы";
    if (op.kind === "prepare") return "Модель готова";
    if (r?.problems) return `Обновлено с ошибками: ${r.problems} документов · добавлено ${r.added}, изменено ${r.changed}, удалено ${r.removed}`;
    if (r && op.state === "partial") return `Обновлено частично · добавлено ${r.added}, изменено ${r.changed}, удалено ${r.removed}`;
    if (r) return r.added + r.changed + r.removed === 0 ? `Индекс актуален · проверено ${r.unchanged} документов` : `Индекс обновлён · добавлено ${r.added}, изменено ${r.changed}, удалено ${r.removed}`;
    return "Результат недоступен";
  }
  const stages: Record<string, string> = { queued: "В очереди", queue_paused: "Очередь на паузе", pause_requested: "Пауза запрошена · после текущего документа", paused: "На паузе · вся очередь", stop_requested: "Остановка запрошена" };
  if (stages[op.state]) return stages[op.state] + (op.state === "queued" && op.queue_position !== undefined && op.queue_position !== null ? ` · перед ним ${op.queue_position} заданий` : "");
  if (op.kind === "download" || op.kind === "prepare") return op.percent === 100 ? (op.kind === "download" ? "Файлы 100 % · проверка файлов" : "Файлы 100 % · подготовка модели") : op.kind === "download" ? "Скачивание файлов модели" : "Подготовка модели";
  return op.docs_total === null ? "Обход папки" : `${op.docs_done ?? 0} из ${op.docs_total} документов`;
}

/** Снимки каталога/очереди не завершают операции. Итог приходит из журнала worker. */
export class NotificationCenter {
  items: Notice[] = [];
  epoch: string | null = null;
  cursor: number | null = null;
  connectionError: string | null = null;
  historyWarning: string | null = null;
  private signatures = new Map<string, string>();

  put(input: Omit<Notice, "unread">, signature = input.title, attention = true): void {
    const previous = this.items.find(n => n.id === input.id);
    const changed = this.signatures.get(input.id) !== signature;
    if (!previous && !changed) return;
    this.signatures.set(input.id, signature);
    const item = { ...input, unread: changed && attention || (previous?.unread ?? false) };
    this.items = previous && !changed ? this.items.map(n => n.id === item.id ? item : n) : [item, ...this.items.filter(n => n.id !== item.id)];
    this.items.sort((a, b) => Number(Boolean(b.active || b.unknown)) - Number(Boolean(a.active || a.unknown)) || (b.updatedAt ?? "").localeCompare(a.updatedAt ?? ""));
    let completed = 0;
    this.items = this.items.filter(n => n.active || n.unknown || ++completed <= 100);
    if (this.signatures.size > 1500) {
      const visible = new Set(this.items.map(n => n.id));
      for (const key of this.signatures.keys()) if (!visible.has(key)) this.signatures.delete(key);
    }
  }
  read(id?: string, signature?: string): void {
    this.items = this.items.map(n => (!id || n.id === id && (!signature || this.signatures.get(id) === signature)) ? { ...n, unread: false } : n);
  }
  version(id: string): string | undefined { return this.signatures.get(id); }
  clear(): void { this.items = this.items.filter(n => n.active || n.unknown); }
  resolve(id: string): void { this.signatures.delete(id); }
  forgetProject(project: string): void {
    this.items = this.items.map(n => n.project === project && n.operation ? { ...n, operation: { ...n.operation, target_available: false } } : n);
  }
  error(id: string, title: string, text: string, target?: Notice["target"], project?: string): void {
    this.put({ id, title, text, active: false, bad: true, percent: null, target, project, updatedAt: new Date().toISOString() }, title + text);
  }
  disconnected(message: string): void { this.connectionError = message; }

  events(output: OperationEventsOutput): void {
    const previousCursor = output.epoch === this.epoch ? this.cursor : null;
    const first = this.epoch === null;
    const changedEpoch = !first && output.epoch !== this.epoch;
    if (changedEpoch) this.historyWarning = "Воркер перезапущен. Прежние незавершённые операции имеют неизвестный исход.";
    else if (output.gap) this.historyWarning = "Часть истории вытеснена. Итоги вне последних 1000 завершений недоступны.";
    const active = new Set(output.active.map(op => `${output.epoch}:${op.id}`));
    const completed = new Set(output.completed.map(op => `${output.epoch}:${op.id}`));
    this.connectionError = null;
    for (const item of [...this.items]) if (item.operation && (item.active || item.unknown) && (changedEpoch || !active.has(item.id) && !completed.has(item.id))) {
      this.put({ ...item, active: false, unknown: true, text: "Исход неизвестен · подтверждённый итог недоступен", percent: null }, "unknown");
    }
    this.epoch = output.epoch;
    this.cursor = output.cursor;
    for (const op of [...output.active, ...output.completed]) {
      if (op.finished_at && previousCursor !== null && op.sequence <= previousCursor) continue;
      const operationId = `${output.epoch}:${op.id}`;
      const problemId = `problem:${output.epoch}:${op.generation}:${op.project ?? `${op.model}:${op.variant}`}`;
      const terminal = op.finished_at !== null;
      const bad = ["error", "partial"].includes(op.state);
      const id = op.source !== "explicit" && terminal && bad ? problemId : operationId;
      if (terminal && id !== operationId) this.items = this.items.filter(n => n.id !== operationId);
      if (terminal && op.state === "success") this.signatures.delete(problemId);
      const previous = this.items.find(n => n.id === id);
      const silent = op.source !== "explicit" && op.state === "success" && op.report && op.report.added + op.report.changed + op.report.removed === 0 && !op.report.problems;
      if (silent) { this.items = this.items.filter(n => n.id !== id); continue; }
      const names: Record<string, string> = { scan: "Проверка файлов", check: "Проверка индекса", files: "Проверка изменений", rebuild: "Переиндексация", download: "Скачивание модели", prepare: "Подготовка модели" };
      const percent = terminal ? null : op.percent ?? (op.docs_total ? Math.min(100, Math.round((op.docs_done ?? 0) / op.docs_total * 100)) : null);
      const signature = id === problemId ? `${op.state}:${op.error}:${JSON.stringify(op.report)}` : terminal ? `${op.sequence}:${op.state}` : "active";
      this.put({ id, title: names[op.kind] ?? "Операция", text: operationText(op), active: !terminal, bad, percent, operation: op, epoch: output.epoch, project: op.project ?? undefined, target: op.project ? "indexing" : "models", updatedAt: this.signatures.get(id) === signature ? previous?.updatedAt : terminal ? op.finished_at! : previous?.updatedAt ?? op.started_at }, signature, terminal || op.source === "explicit" && !first);
    }
  }
}
