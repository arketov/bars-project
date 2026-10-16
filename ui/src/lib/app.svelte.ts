// Состояние окна: проекты, дерево, поиск, просмотр документа, ход индексации.
// Всё, что знает воркер, спрашивается у него; окно хранит только выбор пользователя.

import { call, joinPath, openPath, pickFolder } from "./api";
import { documentsIn } from "./format";
import { buildTree, searchScope, toggle, type SearchScope, type TreeNode } from "./tree";
import {
  BarsError,
  type ActivityOutput,
  type ProjectInfo,
  type ProjectsOutput,
  type ReadOutput,
  type SearchHit,
  type SearchOutput,
  type StatusOutput,
  type TreeOutput,
} from "./types";

export type View = "main" | "settings" | "indexing";

export interface ViewedDocument {
  hit: SearchHit;
  title: string;
  text: string;
  /** Документ пропал между выдачей и открытием. */
  missing: boolean;
}

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
  consentOpen = $state(false);
  /** Сообщение внизу окна: результат действия или ошибка. */
  toast = $state<{ text: string; bad: boolean } | null>(null);

  #timer: ReturnType<typeof setInterval> | null = null;
  #tick = 0;
  #failures = 0;

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
    this.toast = { text, bad };
    setTimeout(() => {
      if (this.toast?.text === text) this.toast = null;
    }, 5000);
  }

  async refresh(force: boolean): Promise<void> {
    this.#tick += 1;
    try {
      const activity = await call<ActivityOutput>("activity");
      const changed = JSON.stringify(activity.running?.project) !== JSON.stringify(this.activity?.running?.project)
        || activity.queue.length !== (this.activity?.queue.length ?? 0);
      this.activity = activity;
      this.#failures = 0;
      this.workerDown = null;
      if (force || changed || this.#tick % PROJECTS_EVERY === 0) await this.loadProjects();
      if (this.current) await this.loadStatus();
    } catch (error) {
      this.#failures += 1;
      // Одна осечка — не повод пугать пользователя.
      if (this.#failures >= 2) this.workerDown = error instanceof Error ? error.message : String(error);
    }
  }

  async loadProjects(): Promise<void> {
    const { projects } = await call<ProjectsOutput>("projects");
    this.projects = projects;
    this.projectsLoaded = true;
    if (!projects.some((project) => project.path === this.current)) {
      const next = projects[0]?.path ?? null;
      if (next !== this.current) await this.select(next);
    } else if (this.tree.length === 0 && this.current) {
      await this.loadTree();
    }
  }

  async loadStatus(): Promise<void> {
    if (!this.current) return;
    try {
      this.status = await call<StatusOutput>("status", { cwd: this.current, project: this.current });
    } catch {
      this.status = null;
    }
  }

  async loadTree(): Promise<void> {
    if (!this.current) {
      this.tree = [];
      return;
    }
    try {
      const output = await call<TreeOutput>("tree", { cwd: this.current, project: this.current });
      this.docsRoot = output.docs_root;
      this.tree = buildTree(output.documents, output.docs_root);
    } catch {
      this.tree = [];
    }
  }

  async select(path: string | null): Promise<void> {
    this.current = path;
    try {
      if (path) localStorage.setItem(PROJECT_KEY, path);
      else localStorage.removeItem(PROJECT_KEY);
    } catch {
      // Выбор не запомнится — окно откроется на первом проекте.
    }
    this.selected = new Set();
    this.filter = "";
    this.results = null;
    this.searchError = null;
    this.viewer = null;
    this.minScore = null;
    this.status = null;
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
    this.searching = true;
    this.searchError = null;
    const started = performance.now();
    try {
      const { modules, files } = this.scope;
      const output = await call<SearchOutput>("search", {
        cwd: project.path,
        project: project.path,
        query,
        modules,
        files,
        ...(this.count === null ? {} : { limit: this.count }),
        ...(this.minScore === null ? {} : { min_score: this.minScore }),
      });
      this.results = output;
      this.searchedQuery = query;
      this.emptyInfo = output.results.length === 0 ? await this.diagnoseEmpty(project.path, modules, files) : null;
    } catch (error) {
      this.results = null;
      this.searchError = error instanceof BarsError ? error : new BarsError(String(error));
      if (this.searchError.kind === "model_loading") this.retrySearchLater(project.path, query);
    } finally {
      this.elapsedMs = Math.round(performance.now() - started);
      this.searching = false;
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
  async diagnoseEmpty(path: string, modules: string[], files: string[]): Promise<{ best: number | null; lowest: number | null }> {
    try {
      const probe = await call<SearchOutput>("search", {
        cwd: path,
        project: path,
        query: this.query.trim(),
        modules,
        files,
        limit: 1,
        min_score: 0,
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

  async openDocument(hit: SearchHit): Promise<void> {
    const project = this.project;
    if (!project) return;
    try {
      const output = await call<ReadOutput>("read", { cwd: project.path, project: project.path, path: hit.path });
      this.viewer = { hit, title: output.title, text: output.text, missing: false };
    } catch (error) {
      if (error instanceof BarsError && error.kind === "file_not_found") {
        this.viewer = { hit, title: hit.title, text: "", missing: true };
      } else {
        this.flash(error instanceof Error ? error.message : String(error), true);
      }
    }
  }

  closeViewer(): void {
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

  async reindexCheck(path: string): Promise<void> {
    try {
      await call("reindex", { cwd: path, project: path });
      await this.refresh(true);
    } catch (error) {
      this.flash(error instanceof Error ? error.message : String(error), true);
    }
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
    this.view = "indexing";
  }

  openSettings(tab = "models"): void {
    this.settingsTab = tab;
    this.view = "settings";
  }
}

export const app = new App();
