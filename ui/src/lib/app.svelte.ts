// Состояние окна: проекты, дерево, поиск, просмотр документа, ход индексации.
// Всё, что знает воркер, спрашивается у него; окно хранит только выбор пользователя.

import { call, joinPath, openPath, pickFolder } from "./api";
import type { GraphReply,GraphSearchOutput,RetrievalMode } from './graph';
import { documentsIn } from "./format";
import { NewTopicsStore } from "./new-topics.svelte";
import { newRequestId } from "./new-topics";
import { NotificationCenter, type Notice } from "./notifications";
import { buildTree, searchScope, toggle, type SearchScope, type TreeNode } from "./tree";
import {
  BarsError,
  type ActivityOutput,
  type ModelsInfoOutput,
  type OperationEventsOutput,
  type Operation,
  type ProjectInfo,
  type ProjectsOutput,
  type ReadOutput,
  type SearchHit,
  type SearchOutput,
  type StatusOutput,
  type TreeOutput,
} from "./types";

export type View = "main" | "settings" | "indexing" | "graph";

export interface ViewedDocument {
  hit: DocumentReference;
  title: string;
  text: string;
  /** Документ пропал между выдачей и открытием. */
  missing: boolean;
}

export type DocumentReference = Pick<SearchHit, "path" | "title" | "first_line" | "last_line">;

const PROJECT_KEY = "bars-project";
const POLL_MS = 1000;
/** Список проектов читает индексы, поэтому без перемен обновляется реже, чем ход. */
const PROJECTS_EVERY = 5;

function remembered(): string | null {
  try {
    return localStorage.getItem(PROJECT_KEY);
  } catch {
    return null;
  }
}

class App {
  view = $state<View>("main");
  searchActive = $state(true);
  navigationVersion = $state(0);
  topicsVersion = $state(0);
  indexingOpen = $state(false);
  settingsTab = $state("models");
  projects = $state<ProjectInfo[]>([]);
  projectsLoaded = $state(false);
  activity = $state<ActivityOutput | null>(null);
  /** Текст ошибки, пока воркер не отвечает (error-states.md, «Воркер недоступен»). */
  workerDown = $state<string | null>(null);
  current = $state<string | null>(remembered());
  status = $state<StatusOutput | null>(null);

  docsRoot = $state("docs");
  tree = $state.raw<TreeNode[]>([]);
  selected = $state.raw<ReadonlySet<string>>(new Set());
  filter = $state("");

  query = $state("");
  retrievalMode = $state<RetrievalMode>('hybrid');
  includeRelated = $state(true);
  graphSearch = $state<GraphSearchOutput|null>(null);
  graphPreview = $state<GraphReply|null>(null);
  /** Число результатов на этот запрос; `null` — по умолчанию из настроек. */
  count = $state<number | null>(null);
  defaultCount = $state(5);
  results = $state<SearchOutput | null>(null);
  searchedQuery = $state("");
  searching = $state(false);
  searchError = $state<BarsError | null>(null);
  elapsedMs = $state(0);
  /** Порог на один запрос (empty-states.md, «Нет результатов поиска»). */
  minScore = $state<number | null>(null);
  /** Что нашлось ниже порога: лучшая оценка и самый низкий порог, который можно задать (empty-states.md). */
  emptyInfo = $state<{ best: number | null; lowest: number | null } | null>(null);
  viewer = $state<ViewedDocument | null>(null);
  /** «Новые темы» текущего проекта: счётчик строки дерева и экран. */
  topics = new NewTopicsStore((text, bad) => this.flash(text, bad));
  /** «Новые темы» открыты в правой панели, как файл: вместо просмотра документа. */
  topicsOpen = $state(false);
  /** Контекст отображённой выдачи для ручной отметки «Не хватило знаний» (NT-02): не поле ввода, а то, что искали. */
  displayed = $state<{ requestId: string; project: string; query: string; modules: string[]; files: string[]; mode: RetrievalMode; includeRelated: boolean; limit: number | null; minScore: number | null } | null>(null);
  /** ID обращения, которое пользователь уже отметил; повторная отметка идемпотентна и не нужна. */
  markedRequest = $state<string | null>(null);
  consentOpen = $state(false);
  deletingProject = $state<ProjectInfo | null>(null);
  deleteBusy = $state(false);
  deleteError = $state<string | null>(null);
  #epoch = 0;
  #searchId = 0;
  /** Сообщение внизу окна: результат действия или ошибка. */
  toast = $state<{ text: string; bad: boolean } | null>(null);
  notifications = $state<Notice[]>([]);
  models = $state<ModelsInfoOutput | null>(null);
  #notifications = new NotificationCenter();
  #refreshing = false;
  #modelsPending: Promise<void> | null = null;
  #actionAttempt = 0;
  #selectionVersion = 0;
  notificationError = $state<string | null>(null);
  notificationHistoryWarning = $state<string | null>(null);
  indexingHistory = $state<OperationEventsOutput | null>(null);
  indexingHistoryError = $state<string | null>(null);
  indexingHistoryLoading = $state(false);
  #indexingPending: Promise<void> | null = null;

  latestIndexingResult(path: string): Operation | null {
    return this.indexingHistory?.completed.filter(op => op.project === path && op.target_available)
      .sort((a, b) => b.sequence - a.sequence)[0] ?? null;
  }

  loadIndexingHistory(): Promise<void> {
    if (this.#indexingPending) return this.#indexingPending;
    this.#indexingPending = this.readIndexingHistory().finally(() => { this.#indexingPending = null; });
    return this.#indexingPending;
  }

  private async readIndexingHistory(): Promise<void> {
    this.indexingHistoryLoading = true;
    try {
      let epoch = this.indexingHistory?.epoch ?? this.#notifications.epoch;
      if (!epoch) epoch = (await call<OperationEventsOutput>("operation_events")).epoch;
      let output = await call<OperationEventsOutput>("operation_events", { epoch, after: 0 });
      // Перезапуск worker: повторяем чтение уже с новой эпохой, без старых итогов.
      if (output.epoch !== epoch) output = await call<OperationEventsOutput>("operation_events", { epoch: output.epoch, after: 0 });
      this.indexingHistory = output;
      this.indexingHistoryError = null;
    } catch (error) {
      this.indexingHistoryError = error instanceof Error ? error.message : String(error);
    } finally {
      this.indexingHistoryLoading = false;
    }
  }

  readNotifications(id?: string, version?: string): void { this.#notifications.read(id, version); this.publishNotifications(); }
  notificationVersion(id: string): string | undefined { return this.#notifications.version(id); }
  async retryNotification(item: Notice): Promise<void> {
    try {
      await call("operation_retry", { epoch: item.epoch, id: item.operation?.id });
      this.flash("Повтор принят в очередь");
      await this.refresh(true);
    } catch (error) { this.flash(error instanceof Error ? error.message : String(error), true); }
  }
  async visitNotification(item: Notice): Promise<void> {
    const selectionVersion = this.#selectionVersion;
    const epoch = this.#epoch;
    try {
      const output = await call<OperationEventsOutput>("operation_events", { epoch: item.epoch, after: Math.max(0, (item.operation?.sequence ?? 1) - 1) });
      const op = [...output.active, ...output.completed].find(op => op.id === item.operation?.id);
      if (output.epoch !== item.epoch || !op?.target_available) throw new Error("Прежняя цель недоступна: регистрация или настройки изменились либо история истекла");
      if (selectionVersion !== this.#selectionVersion || epoch !== this.#epoch) throw new Error("Выбор проекта изменился; прежний переход отменён");
      if (item.project) {
        if (this.current !== item.project) void this.select(item.project);
        this.openIndexing();
      } else this.openSettings("models");
    } catch (error) { this.flash(error instanceof Error ? error.message : String(error), true); }
  }
  clearNotifications(): void { this.#notifications.clear(); this.publishNotifications(); }
  loadModels(): Promise<void> {
    if (this.#modelsPending) return this.#modelsPending;
    this.#modelsPending = call<ModelsInfoOutput>("models_info").then(models => {
      this.models = models;
    }).finally(() => { this.#modelsPending = null; });
    return this.#modelsPending;
  }
  private publishNotifications(): void { this.notifications = this.#notifications.items.map(n => ({ ...n })); this.notificationHistoryWarning = this.#notifications.historyWarning; }

  #timer: ReturnType<typeof setInterval> | null = null;
  #tick = 0;
  #failures = 0;
  #reading = 0;

  get project(): ProjectInfo | null {
    return this.projects.find((project) => project.path === this.current) ?? null;
  }

  get scope(): SearchScope {
    return searchScope(this.tree, this.selected);
  }

  get shownCount(): number {
    return this.count ?? this.results?.applied.results ?? this.defaultCount;
  }

  /** Проекты, которым нужна переиндексация (Т-25). */
  get needReindex(): ProjectInfo[] {
    return this.projects.filter((project) => project.state === "reindex_required");
  }

  /** Сколько проектов требуют внимания: для иконки трея и точек. */
  get attention(): boolean {
    return (
      this.needReindex.length > 0 ||
      (this.activity?.problems.length ?? 0) > 0 ||
      this.projects.some((project) => project.state === "unavailable")
    );
  }

  start(): void {
    if (this.#timer) return;
    void this.refresh(true);
    this.#timer = setInterval(() => void this.refresh(false), POLL_MS);
  }

  stop(): void {
    if (this.#timer) clearInterval(this.#timer);
    this.#timer = null;
  }

  flash(text: string, bad = false): void {
    if (bad) {
      this.#notifications.error(`action:${++this.#actionAttempt}`, "Ошибка действия", text);
      this.publishNotifications();
    }
    this.toast = { text, bad };
    setTimeout(() => {
      if (this.toast?.text === text) this.toast = null;
    }, 5000);
  }

  async refresh(force: boolean): Promise<void> {
    if (this.#refreshing) return;
    this.#refreshing = true;
    this.#tick += 1;
    const epoch = this.#epoch;
    try {
      const activity = await call<ActivityOutput>("activity");
      if (epoch !== this.#epoch) return;
      const changed = JSON.stringify(activity.running?.project) !== JSON.stringify(this.activity?.running?.project)
        || activity.queue.length !== (this.activity?.queue.length ?? 0);
      this.activity = activity;
      try {
        const events = await call<OperationEventsOutput>("operation_events", { epoch: this.#notifications.epoch, after: this.#notifications.cursor });
        if (epoch !== this.#epoch) return;
        this.#notifications.events(events);
        if ((this.view === "indexing" || this.indexingOpen) && (this.indexingHistoryError || events.epoch !== this.indexingHistory?.epoch
          || events.completed.some(op => op.project === this.current))) void this.loadIndexingHistory();
        this.notificationError = null;
      } catch {
        this.notificationError = "Не удалось получить события. Сохранён последний снимок; исход операций пока неизвестен.";
      }
      // Отдельный сбой каталога не означает остановку загрузок или всего воркера.
      try { await this.loadModels(); } catch { /* Сохраняем последний снимок. */ }
      this.publishNotifications();
      this.#failures = 0;
      this.workerDown = null;
      this.#notifications.resolve("worker");
      if (force || changed || this.#tick % PROJECTS_EVERY === 0) await this.loadProjects();
      if (this.current) await this.loadStatus();
      this.syncTopics(force);
    } catch (error) {
      if (epoch !== this.#epoch) return;
      this.#failures += 1;
      // Одна осечка — не повод пугать пользователя.
      if (this.#failures >= 2) this.workerDown = error instanceof Error ? error.message : String(error);
      if (this.workerDown) {
        this.notificationError = "Нет связи с воркером · сохранён последний снимок";
        this.#notifications.disconnected(this.workerDown);
        this.publishNotifications();
      }
    } finally {
      this.#refreshing = false;
    }
  }

  async loadProjects(): Promise<void> {
    const epoch = this.#epoch;
    const { projects } = await call<ProjectsOutput>("projects");
    if (epoch !== this.#epoch) return;
    this.projects = projects;
    this.projectsLoaded = true;
    if (!projects.some((project) => project.path === this.current)) {
      const next = projects[0]?.path ?? null;
      if (next !== this.current) await this.select(next);
    } else if (this.tree.length === 0 && this.current) {
      await this.loadTree();
    }
  }

  /** Привязывает «Новые темы» к выбранному проекту и раз в несколько тиков обновляет счётчик строки дерева. */
  private syncTopics(force: boolean): void {
    this.topics.bind(this.current);
    if (!this.current || this.project?.state === "unavailable") return;
    if (force || this.#tick % PROJECTS_EVERY === 0) void this.topics.load(true);
  }

  async loadStatus(): Promise<void> {
    if (!this.current) return;
    const path = this.current, epoch = this.#epoch;
    try {
      const status = await call<StatusOutput>("status", { cwd: path, project: path });
      if (path === this.current && epoch === this.#epoch) this.status = status;
    } catch {
      if (path === this.current && epoch === this.#epoch) this.status = null;
    }
  }

  async loadTree(): Promise<void> {
    if (!this.current) {
      this.tree = [];
      return;
    }
    const path = this.current, epoch = this.#epoch;
    try {
      const output = await call<TreeOutput>("tree", { cwd: path, project: path });
      if (path !== this.current || epoch !== this.#epoch) return;
      this.docsRoot = output.docs_root;
      this.tree = buildTree(output.documents, output.docs_root);
    } catch {
      if (path === this.current && epoch === this.#epoch) this.tree = [];
    }
  }

  async select(path: string | null): Promise<void> {
    this.graphSearch=null;this.graphPreview=null;
    this.#selectionVersion++;
    this.#searchId++;
    this.searching = false;
    this.current = path;
    try {
      if (path) localStorage.setItem(PROJECT_KEY, path);
      else localStorage.removeItem(PROJECT_KEY);
    } catch {
      // Выбор не запомнится — окно откроется на первом проекте.
    }
    this.selected = new Set();
    this.tree = [];
    this.filter = "";
    this.results = null;
    this.searchedQuery = "";
    this.emptyInfo = null;
    this.searchError = null;
    this.displayed = null;
    this.markedRequest = null;
    this.closeViewer();
    this.topicsOpen = false;
    this.minScore = null;
    this.status = null;
    this.topics.bind(path);
    if (path) void this.topics.load(true);
    await this.loadTree();
    if (path) await this.loadStatus();
  }

  toggleNode(path: string, checked: boolean): void {
    this.selected = toggle(this.tree, this.selected, path, checked);
  }

  clearSelection(): void {
    this.selected = new Set();
  }

  async search(): Promise<void> {
    const project = this.project;
    const query = this.query.trim();
    if (!project || !query) return;
    const searchId = ++this.#searchId;
    const requestId = newRequestId();
    this.searching = true;
    this.searchError = null;
    this.results = null;
    this.graphSearch = null;
    this.displayed = null;
    this.emptyInfo = null;
    const started = performance.now();
    try {
      const { modules, files } = this.scope;
      const mode=this.retrievalMode;
      const includeRelated = this.includeRelated, limit = this.count, minScore = this.minScore;
      const output = await call<SearchOutput|GraphSearchOutput>(mode==='hybrid'?"search":"graph_search", {
        cwd: project.path,
        project: project.path,
        query,
        modules,
        files,
        ...(mode==='hybrid'?{}:{mode}),
        ...(mode==='mix'?{include_related:includeRelated}:{}),
        ...(limit === null ? {} : { limit }),
        ...(minScore === null ? {} : { min_score: minScore }),
        // Служебное поле окна: ID отображаемого обращения. Воркер считает по нему спрос на тему, ручная отметка опирается на него же.
        _window: { request_id: requestId },
      });
      if (searchId !== this.#searchId || this.current !== project.path) return;
      this.results = output;
      this.displayed = { requestId, project: project.path, query, modules, files, mode, includeRelated, limit, minScore };
      this.markedRequest = null;
      this.graphSearch='requested_mode' in output?output as GraphSearchOutput:null;
      this.#notifications.resolve(`search:${project.path}`);
      if (output.rerank_error) this.#notifications.error(`rerank:${project.path}`, `Ошибка rerank · ${project.name}`, "Поиск выполнен без rerank. Проверьте модель и подключение.", "models");
      else this.#notifications.resolve(`rerank:${project.path}`);
      this.publishNotifications();
      this.searchedQuery = query;
      const emptyInfo = output.results.length === 0&&mode!=='graph' ? await this.diagnoseEmpty(project.path, modules, files, query) : null;
      if (searchId === this.#searchId && this.current === project.path) this.emptyInfo = emptyInfo;
    } catch (error) {
      if (searchId !== this.#searchId || this.current !== project.path) return;
      this.results = null;
      this.displayed = null;
      this.graphSearch = null;
      this.searchError = error instanceof BarsError ? error : new BarsError(String(error));
      if (this.searchError.kind !== "model_loading") {
        this.#notifications.error(`search:${project.path}`, `Ошибка поиска · ${project.name}`, this.searchError.message);
        this.publishNotifications();
      }
      if (this.searchError.kind === "model_loading") this.retrySearchLater(project.path, query);
    } finally {
      if (searchId === this.#searchId) {
        this.elapsedMs = Math.round(performance.now() - started);
        this.searching = false;
      }
    }
  }

  /** Пока воркер отвечает `model_loading`, тот же поиск повторяется раз в две секунды (error-states.md). */
  private retrySearchLater(path: string, query: string): void {
    setTimeout(() => {
      const same = this.project?.path === path && this.query.trim() === query;
      if (same && !this.searching && this.searchError?.kind === "model_loading") void this.search();
    }, 2000);
  }

  /**
   * Пустая выдача: ещё один запрос с порогом 0 показывает, что воркер прижмёт к нижнему пределу
   * настроек, и лучшего кандидата. Окно не обходит порог — оно только объясняет пользователю, почему пусто.
   */
  async diagnoseEmpty(path: string, modules: string[], files: string[], query: string): Promise<{ best: number | null; lowest: number | null }> {
    try {
      const probe = await call<SearchOutput>("search", {
        cwd: path,
        project: path,
        query,
        modules,
        files,
        limit: 1,
        min_score: 0,
        // Служебное поле окна: диагностика объясняет пустую выдачу и спроса на тему не создаёт (NT-03).
        _window: { purpose: "diagnostic" },
      });
      return { best: probe.results[0]?.score ?? null, lowest: probe.applied.min_score };
    } catch {
      return { best: null, lowest: null };
    }
  }

  /** «Искать по всему проекту» и «Порог N для этого запроса» из пустой выдачи. */
  async searchWhole(): Promise<void> {
    this.clearSelection();
    await this.search();
  }

  async searchWithThreshold(minScore: number): Promise<void> {
    this.minScore = minScore;
    await this.search();
    this.minScore = null;
  }

  /** «Не хватило знаний»: тема по отображённой выдаче (NT-02). Повторная отметка того же обращения идемпотентна. */
  async markMissing(): Promise<void> {
    const displayed = this.displayed;
    if (!displayed || this.markedRequest === displayed.requestId) return;
    const created = await this.topics.markMissing(displayed);
    if (created === null) return;
    this.markedRequest = displayed.requestId;
    this.flash(created ? "Тема добавлена в «Новые темы»" : "Такая тема уже есть в «Новых темах»");
  }

  /** Нажатие на строку «Новые темы»: открывается в панели просмотра, не меняя область поиска и запрос. */
  openTopics(): void {
    this.navigationVersion++;
    this.topicsVersion++;
    this.view = "main";
    this.closeViewer();
    this.topicsOpen = true;
  }

  closeTopics(): void {
    this.topicsVersion++;
    this.topicsOpen = false;
  }

  async openDocument(hit: DocumentReference): Promise<void> {
    const project = this.project;
    if (!project) return;
    const reading = ++this.#reading;
    this.topicsOpen = false;
    try {
      // Путь индекса уже от корня проекта: исключаем docs/docs/x.md при совпадении имён.
      const output = await call<ReadOutput>("read", { cwd: project.path, project: project.path, path: `./${hit.path}` });
      if (reading !== this.#reading || this.current !== project.path) return;
      this.view = "main";
      this.viewer = { hit, title: output.title, text: output.text, missing: false };
    } catch (error) {
      if (reading !== this.#reading || this.current !== project.path) return;
      if (error instanceof BarsError && error.kind === "file_not_found") {
        this.view = "main";
        this.viewer = { hit, title: hit.title, text: "", missing: true };
      } else {
        this.flash(error instanceof Error ? error.message : String(error), true);
      }
    }
  }

  closeViewer(): void {
    this.#reading++;
    this.viewer = null;
  }

  async openInEditor(path: string): Promise<void> {
    const project = this.project;
    if (project) await openPath(joinPath(project.path, path));
  }

  async addProject(): Promise<void> {
    const path = await pickFolder();
    if (!path) return;
    try {
      await call("register", { path });
      await this.loadProjects();
      await this.select(path);
    } catch (error) {
      this.flash(error instanceof Error ? error.message : String(error), true);
    }
  }

  async removeProject(path: string): Promise<void> {
    try {
      await call("unregister", { cwd: path, project: path });
      await this.loadProjects();
    } catch (error) {
      this.flash(error instanceof Error ? error.message : String(error), true);
    }
  }

  requestDeleteProject(path: string): void {
    if (this.deleteBusy) return;
    const project = this.projects.find(p => p.path === path);
    if (!project) return;
    this.deleteError = null;
    this.deletingProject = { ...project };
  }

  cancelDeletion(): void {
    if (this.deleteBusy) return;
    this.deletingProject = null;
    this.deleteError = null;
  }

  async confirmDeletion(): Promise<void> {
    const project = this.deletingProject;
    if (!project || this.deleteBusy) return;
    this.deleteBusy = true;
    this.deleteError = null;
    try {
      await call("project_delete", { project: project.path, confirmed: true });
    } catch (error) {
      this.deleteError = error instanceof Error ? error.message : String(error);
      this.deleteBusy = false;
      return;
    }
    this.#epoch++;
    this.projects = this.projects.filter(p => p.path !== project.path);
    this.#notifications.forgetProject(project.path);
    this.publishNotifications();
    if (this.activity) this.activity = {
      ...this.activity,
      running: this.activity.running?.project === project.path ? null : this.activity.running,
      queue: this.activity.queue.filter(j => j.project !== project.path),
      problems: this.activity.problems.filter(p => p.project !== project.path),
    };
    this.deletingProject = null;
    this.deleteBusy = false;
    if (this.current === project.path) {
      this.query = "";
      this.view = "main";
      this.consentOpen = false;
      await this.select(this.projects[0]?.path ?? null);
    }
    this.flash(`Проект «${project.name}» удалён из bars`);
  }

  async reindexCheck(path: string): Promise<void> {
    try {
      await call("reindex", { cwd: path, project: path });
      await this.refresh(true);
    } catch (error) {
      this.flash(error instanceof Error ? error.message : String(error), true);
    }
  }

  async retryRebuild(path: string): Promise<void> {
    const project = this.projects.find(p => p.path === path);
    if (project?.state === "reindex_required") { this.consentOpen = true; return; }
    try {
      await call("reindex", { cwd: path, project: path, full: true });
      await this.refresh(true);
    } catch (error) { this.flash(error instanceof Error ? error.message : String(error), true); }
  }

  /** Пауза и продолжение очереди индексации (OQ-55). */
  async togglePause(): Promise<void> {
    try {
      await call(this.activity?.paused ? "index_resume" : "index_pause");
      this.activity = await call<ActivityOutput>("activity");
    } catch (error) {
      this.flash(error instanceof Error ? error.message : String(error), true);
    }
  }

  /** Остановка индексации проекта: идущее задание прерывается, ждущее снимается с очереди. */
  async cancel(path: string): Promise<void> {
    try {
      await call("index_cancel", { project: path });
      await this.refresh(true);
    } catch (error) {
      this.flash(error instanceof Error ? error.message : String(error), true);
    }
  }

  /** «Проверить сейчас»: ставит на проверку все проекты, пропавшие возвращаются в работу, если папка на месте. */
  async recheck(): Promise<void> {
    try {
      await call("check_all");
      await this.refresh(true);
    } catch (error) {
      this.flash(error instanceof Error ? error.message : String(error), true);
    }
  }

  /** Другой корень документации проекта: папка внутри проекта, запись в `.bars/config.toml` делает воркер. */
  async chooseDocsRoot(project: ProjectInfo): Promise<void> {
    const folder = await pickFolder();
    if (!folder) return;
    const base = project.path.replace(/\\/g, "/").replace(/\/+$/, "");
    const normalized = folder.replace(/\\/g, "/").replace(/\/+$/, "");
    if (normalized !== base && !normalized.startsWith(base + "/")) {
      this.flash("Папка документации должна лежать внутри проекта", true);
      return;
    }
    const relative = normalized === base ? "." : normalized.slice(base.length + 1);
    try {
      await call("set_docs_root", { project: project.path, docs_root: relative });
      await this.refresh(true);
    } catch (error) {
      this.flash(error instanceof Error ? error.message : String(error), true);
    }
  }

  /** Проект переехал: регистрируем новое место и снимаем старое (`.bars/` подхватывается как есть). */
  async relocateProject(project: ProjectInfo): Promise<void> {
    const folder = await pickFolder();
    if (!folder) return;
    try {
      await call("register", { path: folder });
      await call("unregister", { cwd: project.path, project: project.path });
      await this.loadProjects();
      await this.select(folder);
    } catch (error) {
      this.flash(error instanceof Error ? error.message : String(error), true);
    }
  }

  documentsOf(project: ProjectInfo): number {
    return documentsIn(project);
  }

  /** Процент идущего задания проекта или `null`, если оно не идёт. */
  progressOf(path: string): number | null {
    const running = this.activity?.running;
    if (!running || running.project !== path || running.docs_total === 0) return null;
    return Math.min(100, Math.round((running.docs_done / running.docs_total) * 100));
  }

  /** Сколько документов ждёт в проекте: идущее задание не дошло до конца и ждущее задание. */
  waitingOf(path: string): number {
    const running = this.activity?.running;
    if (running && running.project === path) return Math.max(0, running.docs_total - running.docs_done);
    return this.activity?.queue.find((job) => job.project === path)?.files ?? 0;
  }

  openIndexing(): void {
    this.navigationVersion++;
    this.view = "indexing";
    void this.loadIndexingHistory();
  }

  openSearch(): void {
    this.navigationVersion++;
    this.view = "main";
  }

  openSettings(tab = "models"): void {
    this.navigationVersion++;
    this.settingsTab = tab;
    this.view = "settings";
  }
}

export const app = new App();
