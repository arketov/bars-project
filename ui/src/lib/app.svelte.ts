// Состояние окна: проекты, дерево, поиск, просмотр документа, ход индексации.
// Всё, что знает воркер, спрашивается у него; окно хранит только выбор пользователя.

import { call, documentWindowOpen, joinPath, openInMainWindow, openPath, pickFolder, treeRead } from "./api";
import { TextSearchStore } from './text-search.svelte';
import { editors } from './editors.svelte';
import { confirmProjectLeave } from './unsaved.svelte';
import type { GraphReply,GraphSearchOutput,RetrievalMode } from './graph';
import { documentsIn } from "./format";
import { AuditsStore } from "./audits.svelte";
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
  type SettingsDoc,
  type SettingsOutput,
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

export type DocumentReference = Pick<SearchHit, "path" | "title" | "first_line" | "last_line"> & { column?: number; length?: number; expectedRevision?: string; /** Открыт из выдачи или «В тексте»: только такой вкладке уместно «Обновить выдачу». */ fromSearch?: boolean };

const PROJECT_KEY = "bars-project";
const POLL_MS = 1000;
const HIDDEN_POLL_MS = 10_000;
/** Список проектов читает индексы, поэтому без перемен обновляется реже, чем ход. */
const PROJECTS_EVERY = 5;
/** Дерево с диска перечитывается раз в несколько тиков: обход каталогов дешевле индексации, но не бесплатен. */
const TREE_EVERY = 1;
/** Состояния аудитов читаются раз в 15 тиков: вид идёт минуты, расписание — раз в минуту. */
const AUDITS_EVERY = 15;

/** Подпись входов дерева: по ней опрос понимает, что перестраивать нечего. */
function treeSignature(documents: { path: string; chunks: number; title: string }[], disk?: string): string {
  return JSON.stringify([disk, documents.map(({ path, chunks, title }) => [path, chunks, title])]);
}

function remembered(): string | null {
  try {
    return localStorage.getItem(PROJECT_KEY);
  } catch {
    return null;
  }
}

class App {
  #windowId = crypto.randomUUID();
  #watchTimer: ReturnType<typeof setInterval> | null = null;
  #watch = (): void => { void call("project_observe", { window: this.#windowId, project: this.current }).catch(() => {}); };
  #releaseWatch = (): void => { void call("project_observe", { window: this.#windowId, project: null }).catch(() => {}); };
  private currentView = $state<View>("main");
  settingsLeave: (() => boolean | Promise<boolean>) | null = null;
  get view(): View { return this.currentView; }
  set view(next: View) {
    if (this.currentView === "settings" && next !== "settings" && this.settingsLeave) {
      const decision = this.settingsLeave();
      if (typeof decision === "boolean") { if (decision) this.currentView = next; }
      else void decision.then(ok => { if (ok && this.currentView === "settings") this.currentView = next; });
    } else this.currentView = next;
  }
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
  searchUnsaved = $state<{ path: string; reason: string }[]>([]);
  #leavingProject = false;
  status = $state<StatusOutput | null>(null);

  docsRoot = $state("docs");
  tree = $state.raw<TreeNode[]>([]);
  selected = $state.raw<ReadonlySet<string>>(new Set());
  filter = $state("");

  query = $state("");
  textSearch = new TextSearchStore();
  retrievalMode = $state<RetrievalMode>('hybrid');
  includeRelated = $state(true);
  graphSearch = $state<GraphSearchOutput|null>(null);
  graphPreview = $state<GraphReply|null>(null);
  /** Вкладка «Граф» показывает анализ связей (пути, влияние, контекст), а не физическую карту. */
  graphAnalysis = $state(false);
  /** Узел, который полный граф выберет после открытия вкладки («В графе» из мини-графа). */
  graphFocus = $state<string | null>(null);
  /** Вкладка «Граф» открывается на виде запроса последнего поиска, даже если пользователь вышел из него в общий вид. */
  graphShowQuery = $state(false);
  graphPackage = $state<number | null>(null);
  /** Путь последнего активного документа (от корня проекта): центр локального графа мини-панели (Т-43, раунд 27). */
  activeDocument = $state<string | null>(null);
  /** Число результатов на этот запрос; `null` — по умолчанию из настроек. */
  count = $state<number | null>(null);
  /** Настройки поиска из settings.toml: умолчание «Результатов» и их предел; `null` — ещё не прочитаны. */
  searchSettings = $state<{ results: number; resultsReranked: number; maxResults: number; rerank: boolean } | null>(null);
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
  /** Аудиты знаний текущего проекта (Т-36): экран в правой панели, как «Новые темы». */
  audits = new AuditsStore((text, bad) => this.flash(text, bad), (project, text) => {
    this.#notifications.put({ id: `audits:${project}`, title: "Аудиты", text, project, target: "audits", active: false, bad: false, percent: null, updatedAt: new Date().toISOString() }, String(++this.#auditNoticeAttempt));
    this.publishNotifications();
  });
  auditsOpen = $state(false);
  auditsVersion = $state(0);
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
  #auditNoticeAttempt = 0;
  #refreshing = false;
  #refreshAgain = false;
  #refreshWaiters: Array<() => void> = [];
  /** Идёт ручная проверка («Проверить сейчас»): кнопки показывают «Проверяем…» до конца обновления. */
  checking = $state(false);
  #lastHiddenPoll = 0;
  #treeSignature = "";
  #treeSyncing = false;
  #treeRequest = 0;
  treeProblems = $state<{ path: string; reason: string }[]>([]);
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
    if (item.target === "audits") {
      if (!item.project || !this.projects.some(p => p.path === item.project)) { this.flash("Проект больше недоступен", true); return; }
      if (this.current !== item.project) await this.select(item.project);
      if (this.current !== item.project) return;
      this.openAudits();
      return;
    }
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

  /** Умолчание «Результатов» с rerank или без него (Т-18); до чтения настроек — 5. */
  get defaultCount(): number {
    const s = this.searchSettings;
    return s ? (s.rerank ? s.resultsReranked : s.results) : 5;
  }

  /** Верхний предел степпера — `search.max_results`; до чтения настроек — 20. */
  get maxCount(): number {
    return this.searchSettings?.maxResults ?? 20;
  }

  get shownCount(): number {
    return Math.min(this.count ?? this.results?.applied.results ?? this.defaultCount, this.maxCount);
  }

  applySearchSettings(doc: SettingsDoc): void {
    this.searchSettings = { results: doc.search.results, resultsReranked: doc.search.results_reranked, maxResults: doc.search.max_results, rerank: doc.rerank.enabled };
  }

  async loadSearchSettings(): Promise<void> {
    try {
      this.applySearchSettings((await call<SettingsOutput>("settings_get")).settings);
    } catch { /* остаются умолчания; настройки перечитаются при открытии экрана настроек */ }
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
    if (!this.#watchTimer) {
      this.#watch();
      this.#watchTimer = setInterval(this.#watch, 10_000);
      window.addEventListener("pagehide", this.#releaseWatch);
    }
    if (this.#timer || this.documentWindow) return;
    document.addEventListener("visibilitychange", this.#visible);
    void this.loadSearchSettings();
    void this.refresh(true);
    this.#timer = setInterval(() => void this.refresh(false), POLL_MS);
  }

  stop(): void {
    if (this.#watchTimer) {
      clearInterval(this.#watchTimer);
      this.#watchTimer = null;
      this.#releaseWatch();
    }
    window.removeEventListener("pagehide", this.#releaseWatch);
    if (this.#timer) clearInterval(this.#timer);
    this.#timer = null;
    document.removeEventListener("visibilitychange", this.#visible);
  }

  #visible = (): void => {
    this.#watch();
    if (!document.hidden) void this.refresh(true);
  };

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
    if (this.documentWindow) return;
    if (this.#refreshing) { this.#refreshAgain ||= force; return; }
    const hidden = document.hidden && !force;
    if (hidden && Date.now() - this.#lastHiddenPoll < HIDDEN_POLL_MS) return;
    if (hidden) this.#lastHiddenPoll = Date.now();
    this.#refreshing = true;
    this.#tick += 1;
    const epoch = this.#epoch;
    try {
      const activity = await call<ActivityOutput>("activity");
      if (epoch !== this.#epoch) return;
      const changed = JSON.stringify(activity.running?.project) !== JSON.stringify(this.activity?.running?.project)
        || activity.queue.length !== (this.activity?.queue.length ?? 0);
      this.activity = activity;
      let modelsChanged = false;
      try {
        const events = await call<OperationEventsOutput>("operation_events", { epoch: this.#notifications.epoch, after: this.#notifications.cursor });
        if (epoch !== this.#epoch) return;
        this.#notifications.events(events);
        modelsChanged = [...events.active, ...events.completed].some(op => op.kind === "download" || op.kind === "prepare");
        if (!hidden && (this.view === "indexing" || this.indexingOpen) && (this.indexingHistoryError || events.epoch !== this.indexingHistory?.epoch
          || events.completed.some(op => op.project === this.current))) void this.loadIndexingHistory();
        this.notificationError = null;
      } catch {
        this.notificationError = "Не удалось получить события. Сохранён последний снимок; исход операций пока неизвестен.";
      }
      // Отдельный сбой каталога не означает остановку загрузок или всего воркера.
      if (!hidden && (modelsChanged || (force && this.view === "settings"))) {
        try { await this.loadModels(); } catch { /* Сохраняем последний снимок. */ }
      }
      this.publishNotifications();
      this.#failures = 0;
      this.workerDown = null;
      this.#notifications.resolve("worker");
      if (hidden) return;
      if (force || changed || this.#tick % PROJECTS_EVERY === 0) await this.loadProjects();
      if (this.current) await this.loadStatus();
      if (this.current && (force || this.#tick % TREE_EVERY === 0) && !document.hidden) void this.syncTree();
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
      if (this.#refreshAgain) {
        this.#refreshAgain = false;
        const waiters = this.#refreshWaiters;
        this.#refreshWaiters = [];
        void this.refresh(true).finally(() => waiters.forEach(resolve => resolve()));
      }
    }
  }

  async loadProjects(): Promise<void> {
    if (this.documentWindow) return;
    const epoch = this.#epoch;
    const { projects } = await call<ProjectsOutput>("projects");
    if (epoch !== this.#epoch) return;
    this.projects = projects;
    this.projectsLoaded = true;
    if (!projects.some((project) => project.path === this.current)) {
      const next = projects[0]?.path ?? null;
      if (next !== this.current) await this.select(next);
    }
  }

  /** Привязывает «Новые темы» к выбранному проекту и раз в несколько тиков обновляет счётчик строки дерева. */
  private syncTopics(force: boolean): void {
    this.topics.bind(this.current);
    if (!this.current || this.project?.state === "unavailable") return;
    if (force || this.#tick % PROJECTS_EVERY === 0) void this.topics.load(true);
    // Аудиты обновляются реже: расписание срабатывает раз в минуту, новые находки — повод для уведомления (A-04).
    this.audits.bind(this.current);
    if (force || this.#tick % AUDITS_EVERY === 0) void this.audits.load(true);
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
      await this.readTree(path, epoch, true);
    } catch (error) {
      if (path === this.current && epoch === this.#epoch) this.treeProblems = [{ path: "", reason: String(error) }];
    }
  }

  /** Дерево идёт по диску (Т-33), сведения индекса подмешиваются по пути; без диска — только индекс. */
  private async readTree(path: string, epoch: number, force: boolean): Promise<void> {
    const request = ++this.#treeRequest;
    const inputs = await Promise.all([call<TreeOutput>("tree", { cwd: path, project: path }), treeRead(path)]).catch(error => {
      if (request !== this.#treeRequest) return null;
      throw error;
    });
    if (!inputs) return;
    const [output, disk] = inputs;
    if (request !== this.#treeRequest || path !== this.current || epoch !== this.#epoch) return;
    this.treeProblems = disk?.problems ?? [];
    const docsRoot = disk?.docs_root ?? output.docs_root;
    const signature = `${docsRoot}:${treeSignature(output.documents, disk?.revision)}`;
    if (!force && signature === this.#treeSignature) return;
    this.#treeSignature = signature;
    this.docsRoot = docsRoot;
    this.tree = buildTree(output.documents, docsRoot, disk ? { files: disk.files.map((file) => file.path), dirs: disk.dirs } : undefined);
  }

  /** Подхватывает изменения вне bars (Проводник, агент) и новые документы индекса: перестраивает дерево, только если оно изменилось. */
  async syncTree(): Promise<void> {
    const path = this.current;
    if (!path || this.#treeSyncing || this.project?.state === "unavailable") return;
    this.#treeSyncing = true;
    try {
      await this.readTree(path, this.#epoch, false);
    } catch (error) {
      // Сбой одного опроса оставляет прежнее дерево.
      if (path === this.current) this.treeProblems = [{ path: "", reason: String(error) }];
    } finally {
      this.#treeSyncing = false;
    }
  }

  async select(path: string | null): Promise<void> {
    if (this.#leavingProject) return;
    if (path !== this.current && this.current && editors.unsaved.some(b => b.project === this.current)) {
      this.#leavingProject = true;
      try { if (!await confirmProjectLeave(this.current)) return; }
      finally { this.#leavingProject = false; }
    }
    this.searchUnsaved = [];
    this.textSearch.reset();
    this.graphSearch=null;this.graphPreview=null;
    this.#selectionVersion++;
    this.#searchId++;
    this.searching = false;
    this.current = path;
    if (this.#watchTimer) this.#watch();
    this.tree = [];
    this.treeProblems = [];
    this.#treeSignature = "";
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
    this.audits.bind(path);
    this.auditsOpen = false;
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

  openSearchUnsaved(path: string): void {
    const buffer = this.current ? editors.get(this.current, path) : undefined;
    if (!buffer) { void this.openDocument({path, title:path, first_line:0, last_line:0}); return; }
    this.view = "main";
    this.viewer = {hit:{path,title:path,first_line:0,last_line:0},title:path,text:buffer.text,missing:false};
    this.activeDocument = path;
  }

  async search(): Promise<void> {
    const project = this.project;
    const query = this.query.trim();
    if (!project || !query) return;
    const searchId = ++this.#searchId;
    const requestId = newRequestId();
    const textParams = {project:project.path, query, ...this.scope, case_sensitive:this.textSearch.caseSensitive, regex:this.textSearch.regex, path_filter:this.textSearch.pathFilter, request_id:requestId};
    const semanticScope = this.scope;
    const mode=this.retrievalMode, includeRelated=this.includeRelated, limit=this.count, minScore=this.minScore;
    this.textSearch.reset();
    this.searching = true;
    this.searchUnsaved = [];
    this.searchError = null;
    this.results = null;
    this.graphSearch = null;
    this.graphPreview = null;
    this.displayed = null;
    this.emptyInfo = null;
    const started = performance.now();
    let textTask: Promise<void> | undefined;
    try {
      const unsaved = editors.unsaved.some(b => b.project === project.path) ? await editors.flush(project.path) : [];
      if (searchId !== this.#searchId || this.current !== project.path) return;
      this.searchUnsaved = unsaved.map(b => ({ path: b.path, reason: b.message || "Правки не записаны" }));
      this.searchedQuery = query;
      textTask = this.textSearch.search(textParams);
      if (project.state === "reindex_required") {
        // Поиск по смыслу ждёт согласия на переиндексацию (error-states.md): воркеру не звоним, «В тексте» читает файлы.
        this.#notifications.resolve(`search:${project.path}`);
        this.publishNotifications();
        return;
      }
      const { modules, files } = semanticScope;
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
      // `reindex_required` — гонка с обновлением списка проектов: баннер переиндексации появится сам, уведомление не нужно.
      if (this.searchError.kind !== "model_loading" && this.searchError.kind !== "reindex_required") {
        this.#notifications.error(`search:${project.path}`, `Ошибка поиска · ${project.name}`, this.searchError.message);
        this.publishNotifications();
      }
      if (this.searchError.kind === "model_loading") this.retrySearchLater(project.path, query);
    } finally {
      await textTask;
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

  /** Нажатие на строку «Аудиты»: открывается в панели просмотра, не меняя область поиска и запрос. */
  openAudits(): void {
    this.navigationVersion++;
    this.auditsVersion++;
    this.view = "main";
    this.closeViewer();
    this.auditsOpen = true;
  }

  closeAudits(): void {
    this.auditsVersion++;
    this.auditsOpen = false;
  }

  /** Отдельное окно документа (Т-40): проект задан окном; выбор главного окна не запоминается и не меняется. */
  documentWindow = $state(false);

  async bindDocumentWindow(project: string): Promise<void> {
    this.stop();
    this.#epoch += 1;
    this.documentWindow = true;
    this.current = project;
    this.topics.bind(project);
    await this.loadTree();
  }

  /** Открывает документ в отдельном окне bars; уже открытое окно выводится вперёд. */
  async openInNewWindow(path: string): Promise<void> {
    const project = this.current;
    if (!project) return;
    try {
      await documentWindowOpen(project, path);
    } catch (error) {
      this.flash(error instanceof Error ? error.message : String(error), true);
    }
  }

  async openDocument(hit: DocumentReference, missingIsDraft = false): Promise<void> {
    // Отдельное окно показывает один документ: ссылка из него открывается в основном окне.
    if (this.documentWindow) {
      if (this.current) await openInMainWindow(this.current, hit.path, hit.first_line, hit.last_line).catch((error) => this.flash(String(error), true));
      return;
    }
    const project = this.project;
    if (!project) return;
    const reading = ++this.#reading;
    this.topicsOpen = false;
    try {
      // Путь индекса уже от корня проекта: исключаем docs/docs/x.md при совпадении имён.
      const output = await call<ReadOutput>("read", { cwd: project.path, project: project.path, path: `./${hit.path}` });
      if (reading !== this.#reading || this.current !== project.path) return;
      if (hit.expectedRevision && hit.expectedRevision !== output.revision) {
        this.flash("Документ изменился после построения графа");
        hit = { ...hit, first_line: 0, last_line: 0, column: undefined, length: undefined };
      }
      this.view = "main";
      this.viewer = { hit, title: output.title, text: output.text, missing: false };
      this.activeDocument = hit.path;
    } catch (error) {
      if (reading !== this.#reading || this.current !== project.path) return;
      if (error instanceof BarsError && error.kind === "file_not_found") {
        if (hit.expectedRevision) { this.flash("Источник удалён", true); return; }
        this.view = "main";
          // Т-32: только явный клик по файлу папки разрешает пустой редактируемый документ.
          this.viewer = { hit, title: hit.title, text: "", missing: !missingIsDraft };
          if (missingIsDraft) this.activeDocument = hit.path;
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
      if (editors.unsaved.some(b => b.project === path) && !await confirmProjectLeave(path)) return;
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
      if (editors.unsaved.some(b => b.project === project.path) && !await confirmProjectLeave(project.path)) { this.deleteBusy = false; return; }
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
    if (this.checking) return;
    this.checking = true;
    try {
      await call("reindex", { cwd: path, project: path });
      await this.refresh(true);
    } catch (error) {
      this.flash(error instanceof Error ? error.message : String(error), true);
    } finally {
      this.checking = false;
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

  /** «Повторить» для документа с ошибкой в очереди документов (document-queue.md). */
  async retryDocument(project: string, path: string): Promise<void> {
    try {
      await call("index_retry", { project, path });
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
    if (this.checking) return;
    this.checking = true;
    try {
      await call("check_all");
      await this.refresh(true);
    } catch (error) {
      this.flash(error instanceof Error ? error.message : String(error), true);
    } finally {
      this.checking = false;
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

  /** Счётчик запросов открыть меню проектов в боковой панели (пункт «Проекты» трея, ISS-43). */
  projectsMenuRequest = $state(0);

  openProjectsMenu(): void {
    this.openSearch();
    this.projectsMenuRequest++;
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
