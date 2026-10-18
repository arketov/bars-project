import type { ActivityOutput, ModelsInfoOutput } from "./types";

export interface Notice {
  id: string;
  title: string;
  text: string;
  active: boolean;
  bad: boolean;
  unread: boolean;
  percent: number | null;
  target?: "models" | "indexing";
  project?: string;
}

/** Состояния опроса обновляют карточку, а не создают событие на каждый процент. */
export class NotificationCenter {
  items: Notice[] = [];
  private signatures = new Map<string, string>();

  put(input: Omit<Notice, "unread">, signature = input.title): void {
    const previous = this.items.find(n => n.id === input.id);
    const changed = this.signatures.get(input.id) !== signature;
    if (!previous && !changed) return; // Очищенная сохраняющаяся проблема.
    this.signatures.set(input.id, signature);
    const item = { ...input, unread: changed || (previous?.unread ?? true) };
    this.items = previous && !changed
      ? this.items.map(n => n.id === item.id ? item : n)
      : [item, ...this.items.filter(n => n.id !== item.id)];
    let completed = 0;
    this.items = this.items.filter(n => n.active || ++completed <= 100);
    // Включая сигнатуры очищенных ошибок: память центра тоже ограничена.
    if (this.signatures.size > 500) {
      const visible = new Set(this.items.map(n => n.id));
      for (const key of this.signatures.keys()) {
        if (!visible.has(key)) this.signatures.delete(key);
        if (this.signatures.size <= 400) break;
      }
    }
  }

  read(): void { this.items = this.items.map(n => ({ ...n, unread: false })); }
  clear(): void { this.items = this.items.filter(n => n.active); }
  resolve(id: string): void { this.signatures.delete(id); }
  forgetProject(project: string): void {
    this.items = this.items.filter(n => n.project !== project && !n.id.endsWith(`:${project}`));
    for (const id of this.signatures.keys()) if (id.endsWith(`:${project}`)) this.signatures.delete(id);
  }

  error(id: string, title: string, text: string, target?: Notice["target"], project?: string): void {
    this.put({ id, title, text, active: false, bad: true, percent: null, target, project }, title + text);
  }

  models(output: ModelsInfoOutput): void {
    for (const model of [...output.embed, ...output.rerank]) {
      const id = `model:${model.id}`;
      if (model.downloading) {
        const percent = model.download_percent ?? null;
        this.put({ id, title: `Загрузка · ${model.id}`, text: percent === 100 ? "Файлы скачаны · подготовка модели" : "Скачивание файлов модели", percent, active: true, bad: false, target: "models" }, "loading");
      } else if (model.download_error) {
        this.error(id, `Ошибка загрузки · ${model.id}`, model.download_error, "models");
      } else if (this.items.some(n => n.id === id && n.active)) {
        this.put({ id, title: `${model.downloaded ? "Модель скачана" : "Загрузка прервана"} · ${model.id}`, text: model.downloaded ? "Файлы модели готовы" : "Файлы модели не готовы. Повторите загрузку в настройках.", percent: null, active: false, bad: !model.downloaded, target: "models" });
      }
    }
    for (const role of ["embed", "rerank"] as const) {
      const runtime = output.runtime?.[role];
      const id = `runtime:${role}`;
      if (runtime?.state === "failed") this.error(id, `Ошибка модели · ${runtime.model}`, "Не удалось подготовить модель. Проверьте настройки моделей.", "models");
      else this.signatures.delete(id);
    }
  }

  activity(output: ActivityOutput): void {
    const live = new Set<string>();
    const job = output.running;
    if (job) {
      const id = `index:${job.project}`;
      live.add(id);
      this.put({ id, title: `Индексация · ${job.name}`, text: output.paused ? "Пауза запрошена · после текущего документа" : job.docs_total ? `${job.docs_done} из ${job.docs_total} документов` : "Обход папки", percent: job.docs_total ? Math.min(100, Math.round(job.docs_done / job.docs_total * 100)) : null, active: true, bad: false, target: "indexing", project: job.project }, "running");
    }
    for (const queued of output.queue) {
      if (queued.project === job?.project) continue;
      const id = `index:${queued.project}`;
      live.add(id);
      this.put({ id, title: `Индексация · ${queued.name}`, text: output.paused ? "Очередь на паузе" : "В очереди", percent: null, active: true, bad: false, target: "indexing", project: queued.project }, "queued");
    }
    for (const item of [...this.items]) {
      if (item.active && item.id.startsWith("index:") && !live.has(item.id)) {
        this.put({ ...item, title: item.title.replace("Индексация", "Выполнение закончено"), text: "Задание вышло из очереди. Подробности — в ходе индексации.", active: false, percent: null }, "ended");
      }
    }
    const errors = new Set<string>();
    for (const problem of output.problems) {
      const id = `index-error:${problem.project}`;
      errors.add(id);
      this.error(id, `Ошибка индексации · ${problem.name}`, problem.last_error || `Не обработаны документы: ${problem.files.length}`, "indexing", problem.project);
    }
    for (const id of this.signatures.keys()) if (id.startsWith("index-error:") && !errors.has(id)) this.signatures.delete(id);
  }
}
