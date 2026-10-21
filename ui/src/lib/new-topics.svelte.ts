// Состояние экрана «Новые темы». Всё знает воркер: окно хранит выбор пользователя (проект, тема, фильтр)
// и отбрасывает поздние ответы после смены проекта, очистки или удаления (design.md, «Внутренние операции и гонки»).

import { call } from "./api";
import {
  activeCount,
  isActive,
  visibleTopics,
  type TopicDetail,
  type TopicFilter,
  type TopicSort,
  type TopicsList,
  type TopicStatus,
  type TopicSummary,
} from "./new-topics";
import { BarsError } from "./types";

export interface BatchProgress {
  done: number;
  total: number;
  cancelled: boolean;
}

export type Flash = (text: string, bad?: boolean) => void;

const message = (error: unknown): string => (error instanceof Error ? error.message : String(error));
const isStale = (error: unknown): boolean => error instanceof BarsError && error.kind === "stale_ref";

export class NewTopicsStore {
  /** Проект, к которому привязан список; чужой список не показывается. */
  project = $state<string | null>(null);
  list = $state<TopicsList | null>(null);
  loading = $state(false);
  /** Ошибка чтения списка: не пустой список и не нулевой счётчик (NT-11). */
  error = $state<string | null>(null);
  filter = $state<TopicFilter>("active");
  query = $state("");
  sort = $state<TopicSort>("recent");
  selectedId = $state<number | null>(null);
  detail = $state<TopicDetail | null>(null);
  detailLoading = $state(false);
  detailError = $state<string | null>(null);
  checking = $state.raw<ReadonlySet<number>>(new Set());
  batch = $state<BatchProgress | null>(null);
  /** Идёт изменение (статус, имя, удаление, очистка, переключатель). */
  busy = $state(false);
  /** Прокрутка списка: возврат из документа её сохраняет. */
  scrollTop = 0;

  #flash: Flash;
  /** Растёт при смене проекта: ответ прежнего проекта отбрасывается. */
  #version = 0;
  #listSeq = 0;
  #detailSeq = 0;
  #batchToken = 0;

  constructor(flash: Flash = () => {}) {
    this.#flash = flash;
  }

  setFlash(flash: Flash): void {
    this.#flash = flash;
  }

  /** Число активных тем для строки дерева; `null` — списка нет или чтение не удалось. */
  get count(): number | null {
    return this.list && !this.error ? activeCount(this.list.topics) : null;
  }

  get topics(): TopicSummary[] {
    return this.list?.topics ?? [];
  }

  get visible(): TopicSummary[] {
    return visibleTopics(this.topics, this.filter, this.query, this.sort);
  }

  get selected(): TopicSummary | null {
    return this.topics.find((topic) => topic.id === this.selectedId) ?? null;
  }

  /** Привязка к проекту. Тот же проект — ничего не сбрасывается. */
  bind(project: string | null): void {
    if (project === this.project) return;
    this.#version++;
    this.#batchToken++;
    this.project = project;
    this.list = null;
    this.error = null;
    this.loading = false;
    this.filter = "active";
    this.query = "";
    this.selectedId = null;
    this.detail = null;
    this.detailError = null;
    this.detailLoading = false;
    this.checking = new Set();
    this.batch = null;
    this.busy = false;
  }

  /** Читает список. `quiet` — обновление счётчика без индикатора загрузки. */
  async load(quiet = false): Promise<void> {
    const project = this.project;
    if (!project) return;
    const version = this.#version;
    const seq = ++this.#listSeq;
    if (!quiet) this.loading = true;
    try {
      const list = await call<TopicsList>("new_topics_list", { project });
      if (version !== this.#version || seq !== this.#listSeq) return;
      this.list = list;
      this.error = null;
      this.#reconcileSelection();
    } catch (error) {
      if (version !== this.#version || seq !== this.#listSeq) return;
      this.error = message(error);
    } finally {
      if (version === this.#version && seq === this.#listSeq) this.loading = false;
    }
  }

  /** После обновления списка выбранная тема либо исчезла, либо сменила ревизию (новое обращение). */
  #reconcileSelection(): void {
    if (this.selectedId === null) return;
    const topic = this.selected;
    if (!topic) {
      this.selectedId = null;
      this.detail = null;
      return;
    }
    if (this.detail && this.detail.id === topic.id && this.detail.revision !== topic.revision && !this.checking.has(topic.id)) {
      void this.loadDetail(topic.id, true);
    }
  }

  async select(id: number | null): Promise<void> {
    this.selectedId = id;
    this.detail = null;
    this.detailError = null;
    if (id !== null) await this.loadDetail(id);
  }

  async loadDetail(id: number, quiet = false): Promise<void> {
    const project = this.project;
    if (!project) return;
    const version = this.#version;
    const seq = ++this.#detailSeq;
    if (!quiet) this.detailLoading = true;
    try {
      const detail = await call<TopicDetail>("new_topics_get", { project, id });
      if (version !== this.#version || seq !== this.#detailSeq || this.selectedId !== id) return;
      this.detail = detail;
      this.detailError = null;
    } catch (error) {
      if (version !== this.#version || seq !== this.#detailSeq || this.selectedId !== id) return;
      if (isStale(error)) {
        this.selectedId = null;
        this.detail = null;
        await this.load(true);
        return;
      }
      this.detailError = message(error);
    } finally {
      if (version === this.#version && seq === this.#detailSeq) this.detailLoading = false;
    }
  }

  /** Известная ревизия темы: свежая из карточки, иначе из списка. */
  #revision(id: number): number | null {
    if (this.detail?.id === id) return this.detail.revision;
    return this.topics.find((topic) => topic.id === id)?.revision ?? null;
  }

  #staleNotice(): void {
    this.#flash("Тема изменилась или удалена; список обновлён", true);
    void this.load(true);
    if (this.selectedId !== null) void this.loadDetail(this.selectedId, true);
  }

  #markChecking(id: number, on: boolean): void {
    const next = new Set(this.checking);
    if (on) next.add(id);
    else next.delete(id);
    this.checking = next;
  }

  /** «Проверить знания». Возвращает `true`, если проверка записана воркером. */
  async check(id: number, expand = false): Promise<boolean> {
    const project = this.project;
    const revision = this.#revision(id);
    if (!project || revision === null || this.checking.has(id)) return false;
    const version = this.#version;
    this.#markChecking(id, true);
    try {
      const detail = await call<TopicDetail>("new_topics_check", { project, id, revision, expand });
      if (version !== this.#version) return false;
      if (this.selectedId === id) {
        this.detail = detail;
        this.detailError = null;
      }
      await this.load(true);
      return true;
    } catch (error) {
      if (version !== this.#version) return false;
      if (isStale(error)) this.#staleNotice();
      else this.#flash(`Проверка не выполнена: ${message(error)}`, true);
      return false;
    } finally {
      if (version === this.#version) this.#markChecking(id, false);
    }
  }

  /** «Проверить все»: активные темы по одной, с N/M; отмена прекращает оставшуюся очередь. */
  async checkAll(): Promise<void> {
    if (this.batch || !this.project) return;
    const ids = this.topics.filter(isActive).map((topic) => topic.id);
    if (!ids.length) return;
    const token = ++this.#batchToken;
    const progress: BatchProgress = $state({ done: 0, total: ids.length, cancelled: false });
    this.batch = progress;
    for (const id of ids) {
      if (token !== this.#batchToken || progress.cancelled) break;
      // Тема могла исчезнуть, пока шли предыдущие проверки.
      if (!this.topics.some((topic) => topic.id === id)) {
        progress.total -= 1;
        continue;
      }
      await this.check(id);
      if (token !== this.#batchToken) break;
      progress.done += 1;
    }
    if (token === this.#batchToken) {
      this.batch = null;
      if (progress.cancelled) this.#flash(`Проверка остановлена: проверено ${progress.done} из ${progress.total}`);
    }
  }

  cancelBatch(): void {
    if (this.batch) this.batch.cancelled = true;
  }

  async #mutate<T>(action: () => Promise<T>): Promise<T | null> {
    const version = this.#version;
    this.busy = true;
    try {
      return await action();
    } catch (error) {
      if (version !== this.#version) return null;
      if (isStale(error)) this.#staleNotice();
      else this.#flash(message(error), true);
      return null;
    } finally {
      if (version === this.#version) this.busy = false;
    }
  }

  async setStatus(id: number, status: Extract<TopicStatus, "open" | "resolved">): Promise<void> {
    const project = this.project, revision = this.#revision(id);
    if (!project || revision === null) return;
    const done = await this.#mutate(() => call("new_topics_set_status", { project, id, revision, status }));
    if (done) await this.#refreshAfterChange(id);
  }

  async rename(id: number, title: string): Promise<boolean> {
    const project = this.project, revision = this.#revision(id);
    const clean = title.trim();
    if (!project || revision === null || !clean) return false;
    const done = await this.#mutate(() => call("new_topics_rename", { project, id, revision, title: clean }));
    if (done) await this.#refreshAfterChange(id);
    return done !== null;
  }

  async remove(id: number): Promise<boolean> {
    const version = this.#version;
    const project = this.project, revision = this.#revision(id);
    if (!project || revision === null) return false;
    const done = await this.#mutate(() => call("new_topics_delete", { project, id, revision }));
    if (!done || version !== this.#version) return false;
    if (this.selectedId === id) {
      this.selectedId = null;
      this.detail = null;
    }
    await this.load(true);
    return true;
  }

  async clear(): Promise<boolean> {
    const project = this.project;
    if (!project) return false;
    const done = await this.#mutate(() => call("new_topics_clear", { project, confirmed: true }));
    if (!done) return false;
    this.#batchToken++;
    this.batch = null;
    this.selectedId = null;
    this.detail = null;
    await this.load(true);
    return true;
  }

  async setEnabled(enabled: boolean): Promise<void> {
    const project = this.project;
    if (!project) return;
    const done = await this.#mutate(() => call("new_topics_settings", { project, enabled }));
    if (done) await this.load(true);
  }

  /** Ручная отметка по отображённой выдаче. Возвращает `created` или `null` при отказе. */
  async markMissing(request: { requestId: string; query: string; modules: string[]; files: string[] }): Promise<boolean | null> {
    const project = this.project;
    if (!project) return null;
    const version = this.#version;
    try {
      const output = await call<{ topic_id: number; created: boolean }>("new_topics_mark_missing", {
        project,
        request_id: request.requestId,
        query: request.query,
        modules: request.modules,
        files: request.files,
      });
      if (version === this.#version) void this.load(true);
      return output.created;
    } catch (error) {
      this.#flash(message(error), true);
      return null;
    }
  }

  async #refreshAfterChange(id: number): Promise<void> {
    await this.load(true);
    if (this.selectedId === id) await this.loadDetail(id, true);
  }
}
