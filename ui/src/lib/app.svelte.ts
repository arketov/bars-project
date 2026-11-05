// Состояние окна: проекты, дерево, поиск, просмотр документа, ход индексации.
// Всё, что знает воркер, спрашивается у него; окно хранит только выбор пользователя.

import { call, documentWindowOpen, joinPath, openInMainWindow, openPath, pickFolder, projectRootPrepare, projectRootSuggest, type RootChoice, type RootSuggestion } from "./api";
import type { DiskTreeOutput } from "./api";
import { ProjectUpdates, type Snapshot } from "./window-updates";
import { editors } from './editors.svelte';
import { confirmProjectLeave } from './unsaved.svelte';
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

export type DocumentReference = Pick<SearchHit, "path" | "title" | "first_line" | "last_line"> & { column?: number; length?: number; expectedRevision?: string; /** Открыт из выдачи или «В тексте»: только такой вкладке уместно «Обновить выдачу». */ fromSearch?: boolean; /** Слова и фразы запроса: «Открыть раздел» подсвечивает их в тексте раздела. */ terms?: { words: string[]; phrases: string[] } };

const PROJECT_KEY = "bars-project";
const SCORES_KEY = "bars-search-scores";
/** Через сколько поиск считается ждущим модель и показывает баннер с кнопкой «по словам». */
const MODEL_WAIT_BANNER_MS = 2500;
function readFlag(key: string): boolean {
  try { return localStorage.getItem(key) === "1"; } catch { return false; }
}
const POLL_MS = 1000;
const HIDDEN_POLL_MS = 10_000;
/** Список проектов читает индексы, поэтому без перемен обновляется реже, чем ход. */
const PROJECTS_EVERY = 5;
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
  #nativeVisible = $state(true);
  get windowVisible(): boolean { return this.#nativeVisible && !document.hidden; }
  #windowId = crypto.randomUUID();
  #watchTimer: ReturnType<typeof setInterval> | null = null;
  #watch = (): void => {
    const project = this.current;
    void this.#updates.lease(async () => {
      const result = await call<{ snapshot_observed?: boolean }>("project_observe", { window: this.#windowId, project: this.windowVisible ? project : null });
      if (result.snapshot_observed === false && project === this.current && this.windowVisible && this.#watchTimer) void this.#updates.reconnect();
    }).catch(() => {});
  };
  #releaseWatch = (): void => { void this.#updates.lease(async () => { await call("project_observe", { window: this.#windowId, project: null }); }).catch(() => {}); };
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
  /** Баллы по сигналам в выдаче: переключатель «показать баллы» в параметрах поиска, выключен по умолчанию и запоминается. */
  showScores = $state(readFlag(SCORES_KEY));
  /** Поиск ждёт загрузку модели дольше обычного: баннер с кнопкой «Искать по словам сейчас». */
  waitingModel = $state(false);
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
  displayed = $state<{ requestId: string; project: string; query: string; modules: string[]; files: string[]; limit: number | null } | null>(null);
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
  #updates = new ProjectUpdates(snapshot => this.applySnapshot(snapshot), error => { this.treeProblems = [{ path: "", reason: `Сохранён последний снимок: ${error}` }]; }, undefined, () => this.windowVisible);
  #disk: DiskTreeOutput | null = null;
  #index: TreeOutput | null = null;
  #snapshotKey = "";
  #snapshotEpochKey = "";
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
      document.addEventListener("visibilitychange", this.#visible);
      void this.#updates.ensure(this.current, this.documentWindow);
    }
    if (this.#timer || this.documentWindow) return;
    void this.loadSearchSettings();
    void this.refresh(this.windowVisible);
    this.#timer = setInterval(() => void this.refresh(false), POLL_MS);
  }

  stop(): void {
    this.#updates.stop();
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
    if (!this.windowVisible) this.#updates.stop();
    else void this.#updates.bind(this.current, this.documentWindow);
    if (this.windowVisible) void this.refresh(true);
  };

  windowVisibility(visible: boolean): void {
    if (this.#nativeVisible === visible) return;
    this.#nativeVisible = visible;
    if (this.#watchTimer) this.#visible();
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
    if (this.documentWindow) return;
    if (this.#refreshing) {
      this.#refreshAgain ||= force;
      if (force) return new Promise(resolve => this.#refreshWaiters.push(resolve));
      return;
    }
    const hidden = !this.windowVisible && !force;
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
      if (force && this.current) await this.#updates.request(this.current, this.documentWindow, true);
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
    await this.#updates.request(this.current, this.documentWindow, true);
  }

  async loadTree(): Promise<void> {
    if (!this.current) {
      this.tree = [];
      return;
    }
    await this.#updates.request(this.current, this.documentWindow, true);
  }

  /** Дерево идёт по диску (Т-33), сведения индекса подмешиваются по пути; без диска — только индекс. */
  private applySnapshot(snapshot: Snapshot): void {
    if (snapshot.project !== this.current) return;
    const key = `${snapshot.project}:${snapshot.subscription}:${snapshot.epoch}`;
    if (key !== this.#snapshotKey) { this.#snapshotKey = key; this.#treeSignature = ""; }
    const epochKey = `${snapshot.project}:${snapshot.epoch}`;
    if (epochKey !== this.#snapshotEpochKey) { this.#snapshotEpochKey = epochKey; this.#disk = null; this.#index = null; }
    const { disk: diskPart, index: indexPart, status: statusPart } = snapshot.parts;
    if (diskPart && !diskPart.unchanged && diskPart.value) this.#disk = diskPart.value as DiskTreeOutput;
    if (indexPart && !indexPart.unchanged && indexPart.value) this.#index = indexPart.value as TreeOutput;
    if (statusPart && !statusPart.unchanged && statusPart.value) this.status = statusPart.value as StatusOutput;
    const errors = Object.values(snapshot.parts).flatMap(part => part.error ? [{ path: "", reason: `Сохранён последний снимок: ${part.error}` }] : []);
    const disk = this.#disk, output = this.#index;
    this.treeProblems = [...(disk?.problems ?? []), ...errors];
    // При новой идентичности показываем прежнее дерево целиком с ошибкой, не смешивая части.
    if ((!disk && diskPart?.error) || (!output && indexPart?.error)) return;
    if (!output) return;
    const docsRoot = disk?.docs_root ?? output.docs_root;
    const signature = `${docsRoot}:${treeSignature(output.documents, disk?.revision)}`;
    if (signature === this.#treeSignature) return;
    this.#treeSignature = signature;
    this.docsRoot = docsRoot;
    this.tree = buildTree(output.documents, docsRoot, disk ? { files: disk.files.map((file) => file.path), dirs: disk.dirs, nested: disk.nested } : undefined);
  }

  /** Подхватывает изменения вне bars (Проводник, агент) и новые документы индекса: перестраивает дерево, только если оно изменилось. */
  async syncTree(): Promise<void> {
    await this.loadTree();
  }

  async select(path: string | null): Promise<void> {
    if (this.#leavingProject) return;
    if (path !== this.current && this.current && editors.unsaved.some(b => b.project === this.current)) {
      this.#leavingProject = true;
      try { if (!await confirmProjectLeave(this.current)) return; }
      finally { this.#leavingProject = false; }
    }
    this.searchUnsaved = [];
    this.#selectionVersion++;
    this.#searchId++;
    this.searching = false;
    this.current = path;
    if (this.#watchTimer) this.#watch();
    this.tree = [];
    this.treeProblems = [];
    this.#treeSignature = "";
    this.#disk = null; this.#index = null;
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
    this.waitingModel = false;
    this.searchError = null;
    this.displayed = null;
    this.markedRequest = null;
    this.closeViewer();
    this.topicsOpen = false;
    this.status = null;
    this.topics.bind(path);
    this.audits.bind(path);
    this.auditsOpen = false;
    if (path) void this.topics.load(true);
    await this.#updates.bind(path, this.documentWindow);
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

  /** Поиск. `wordsOnly` — «Искать по словам сейчас»: воркер не ждёт загрузку модели и ищет по словам запроса. */
  async search(options: { wordsOnly?: boolean } = {}): Promise<void> {
    const project = this.project;
    const query = this.query.trim();
    if (!project || !query) return;
    const searchId = ++this.#searchId;
    const requestId = newRequestId();
    const scope = this.scope;
    const limit = this.count;
    this.searching = true;
    this.waitingModel = false;
    this.searchUnsaved = [];
    this.searchError = null;
    this.results = null;
    this.displayed = null;
    const started = performance.now();
    // Загрузка выгруженной модели занимает секунды: после паузы выдача показывает ожидание и кнопку «по словам».
    const waitTimer = setTimeout(() => { if (searchId === this.#searchId) this.waitingModel = true; }, MODEL_WAIT_BANNER_MS);
    try {
      const unsaved = editors.unsaved.some(b => b.project === project.path) ? await editors.flush(project.path) : [];
      if (searchId !== this.#searchId || this.current !== project.path) return;
      this.searchUnsaved = unsaved.map(b => ({ path: b.path, reason: b.message || "Правки не записаны" }));
      this.searchedQuery = query;
      if (project.state === "reindex_required") {
        // Поиск ждёт согласия на переиндексацию (error-states.md): воркеру не звоним.
        this.#notifications.resolve(`search:${project.path}`);
        this.publishNotifications();
        return;
      }
      const { modules, files } = scope;
      const output = await call<SearchOutput>("search", {
        cwd: project.path,
        project: project.path,
        query,
        modules,
        files,
        ...(limit === null ? {} : { limit }),
        ...(options.wordsOnly ? { wait_models: false } : {}),
        // Служебное поле окна: ID отображаемого обращения. Воркер считает по нему спрос на тему, ручная отметка опирается на него же.
        _window: { request_id: requestId },
      });
      if (searchId !== this.#searchId || this.current !== project.path) return;
      this.results = output;
      this.displayed = { requestId, project: project.path, query, modules, files, limit };
      this.markedRequest = null;
      this.#notifications.resolve(`search:${project.path}`);
      if (output.rerank_error) this.#notifications.error(`rerank:${project.path}`, `Ошибка оценки моделью · ${project.name}`, "Поиск выполнен без оценки моделью. Проверьте модель и подключение.", "models");
      else this.#notifications.resolve(`rerank:${project.path}`);
      this.publishNotifications();
      this.searchedQuery = query;
    } catch (error) {
      if (searchId !== this.#searchId || this.current !== project.path) return;
      this.results = null;
      this.displayed = null;
      this.searchError = error instanceof BarsError ? error : new BarsError(String(error));
      // `reindex_required` — гонка с обновлением списка проектов: баннер переиндексации появится сам, уведомление не нужно.
      if (this.searchError.kind !== "model_loading" && this.searchError.kind !== "reindex_required") {
        this.#notifications.error(`search:${project.path}`, `Ошибка поиска · ${project.name}`, this.searchError.message);
        this.publishNotifications();
      }
      if (this.searchError.kind === "model_loading") this.retrySearchLater(project.path, query);
    } finally {
      clearTimeout(waitTimer);
      if (searchId === this.#searchId) {
        this.waitingModel = false;
        this.elapsedMs = Math.round(performance.now() - started);
        this.searching = false;
      }
    }
  }

  /** «Искать по словам сейчас»: ожидающий поиск бросается, тот же запрос идёт без ожидания модели. */
  async searchWords(): Promise<void> {
    await this.search({ wordsOnly: true });
  }

  /** Пока воркер отвечает `model_loading`, тот же поиск повторяется раз в две секунды (error-states.md). */
  private retrySearchLater(path: string, query: string): void {
    setTimeout(() => {
      const same = this.project?.path === path && this.query.trim() === query;
      if (same && !this.searching && this.searchError?.kind === "model_loading") void this.search();
    }, 2000);
  }

  /** «Повторить» на экране ошибки модели: воркер снимает паузу перед повтором загрузки (ISS-71), затем поиск. */
  async retryModelSearch(): Promise<void> {
    try {
      await call("model_retry", {});
    } catch {
      // Без сброса поиск всё равно повторяется: воркер сам вернёт ошибку.
    }
    await this.search();
  }

  /** «Весь проект» из пустой выдачи. */
  async searchWhole(): Promise<void> {
    this.clearSelection();
    await this.search();
  }

  /** «Без кавычек», «Без исключения» и чип «×»: запрос заменяется и ищется заново. */
  async searchAs(query: string): Promise<void> {
    this.query = query;
    await this.search();
  }

  /** Переключатель «показать баллы»: выбор запоминается. */
  setShowScores(on: boolean): void {
    this.showScores = on;
    try { localStorage.setItem(SCORES_KEY, on ? "1" : "0"); } catch { /* хранилище недоступно: выбор живёт до закрытия окна */ }
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
    this.#disk = null; this.#index = null; this.#treeSignature = "";
    this.topics.bind(project);
    await this.#updates.bind(project, true);
    this.start();
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

  renamingProject = $state<ProjectInfo | null>(null);
  renameBusy = $state(false);
  renameError = $state<string | null>(null);

  requestRenameProject(path: string): void {
    if (this.renameBusy) return;
    const project = this.projects.find(p => p.path === path);
    if (!project) return;
    this.renameError = null;
    this.renamingProject = { ...project };
  }

  cancelRenameProject(): void {
    if (this.renameBusy) return;
    this.renamingProject = null;
    this.renameError = null;
  }

  async renameProject(name: string): Promise<void> {
    const project = this.renamingProject;
    if (!project || this.renameBusy) return;
    name = name.trim();
    if (!name || /[\x00-\x1f\x7f-\x9f]/.test(name)) {
      this.renameError = "Введите имя проекта без управляющих символов";
      return;
    }
    this.renameBusy = true;
    this.renameError = null;
    try {
      await call("project_rename", { project: project.path, name });
      this.projects = this.projects.map(p => p.path === project.path ? { ...p, name } : p);
      this.renamingProject = null;
      this.flash(`Проект переименован в «${name}»`);
    } catch (error) {
      this.renameError = error instanceof Error ? error.message : String(error);
    } finally {
      this.renameBusy = false;
    }
  }

  /** Выбор корня документации новой папки (Т-7); `null` — диалог закрыт. */
  rootChoice = $state<{ path: string; suggested: RootChoice; hasDocs: boolean; moved: ProjectInfo | null; inside: RootSuggestion["inside"]; nested: NonNullable<RootSuggestion["nested"]> } | null>(null);
  rootBusy = $state(false);

  async addProject(): Promise<void> {
    await this.askRoot(null);
  }

  /** Диалог «Что считать документацией?» для выбранной папки; `moved` — проект, который переезжает сюда. */
  private async askRoot(moved: ProjectInfo | null): Promise<void> {
    const path = await pickFolder();
    if (!path) return;
    try {
      const { has_docs, suggested, inside, nested } = await projectRootSuggest(path);
      this.rootChoice = { path, suggested, hasDocs: has_docs, moved, inside: inside ?? null, nested: nested ?? [] };
    } catch (error) {
      this.flash(error instanceof Error ? error.message : String(error), true);
    }
  }

  cancelRoot(): void {
    if (this.rootBusy) return;
    this.rootChoice = null;
  }

  /** Записывает выбранный корень, регистрирует папку и открывает проект. */
  async confirmRoot(root: RootChoice): Promise<void> {
    const pending = this.rootChoice;
    if (!pending || this.rootBusy) return;
    this.rootBusy = true;
    try {
      await projectRootPrepare(pending.path, root);
      await call("register", { path: pending.path });
      if (pending.moved) await call("unregister", { cwd: pending.moved.path, project: pending.moved.path });
      this.rootChoice = null;
      await this.loadProjects();
      await this.select(pending.path);
    } catch (error) {
      this.rootChoice = null;
      this.flash(error instanceof Error ? error.message : String(error), true);
    } finally {
      this.rootBusy = false;
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
    await this.askRoot(project);
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
