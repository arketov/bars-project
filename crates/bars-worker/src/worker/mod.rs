//! Воркер без окна: реестр проектов, очередь индексации, слежение за файлами и часовая проверка
//! (`docs/architecture/worker.md`, `docs/architecture/projects.md`, Т-5, Т-10, Т-15, Т-25).
//!
//! Обработчики протокола и CLI берут отсюда небольшой API [`Worker`]: регистрация, снятие,
//! выбор проекта, `status`, `reindex`, `session_start`, доступ к проекту для поиска и модели.
//! Поиск в очереди не стоит: он берёт [`Worker::gate`] на чтение и открывает индекс сам.

mod activity;
mod control;
mod deletion;
mod delta;
mod doc_queue;
mod graph;
mod job;
mod model_lifecycle;
pub mod models;
mod operations;
mod queue;
mod registry;
mod scan;
mod subprojects;
mod summary;
mod timefmt;
pub mod updates;
mod watcher;
mod worktrees;
mod wt_watch;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant, SystemTime};

use crate::autostart::{Autostart, NoAutostart};
use crate::clients::InstallEnv;
use bars_core::indexing::indexer::UpdateReport;
use bars_core::models::{ModelErrorKind, ModelFailure};
use bars_core::secrets::{OsSecrets, SecretStore};
use bars_core::settings::{DEFAULT_DOCS_ROOT, Settings, project_config_path};
use bars_protocol::ProjectState;
use tokio::runtime::Handle;
use tokio::sync::{Notify, RwLock};
use tokio::task::JoinHandle;

pub use model_lifecycle::ModelIdlePolicy;
pub use models::{
    BATCH_SIZE, BuiltinProvider, ConfiguredProvider, ModelProvider, ModelSlot, Models, Readiness,
    RerankSlot, SharedEmbedder, SharedReranker,
};
pub use queue::Priority;
pub use registry::{
    CwdTarget, Discovered, ProjectEntry, Registry, RegistryError, ResolveError, WorktreeRef,
};
pub use worktrees::{ViewSelector, WtSelector, WtSource};
pub use summary::{Health, Line, Summary};
pub use timefmt::{index_token, rfc3339};

use control::{JobCell, JobObserver, Pause};
pub use control::{Kind as JobKind, Progress as JobProgress};
use doc_queue::{DocQueue, DocTarget};
pub use doc_queue::{DocCounts, DocSource, DocState, DocView};
use job::{JobEnv, Outcome};
use queue::{Job, JobQueue};
use registry::{has_markdown, normalize};
use watcher::{DEBOUNCE, ProjectWatcher, Sink, WatchEvent};

/// Как часто воркер проверяет все проекты (Т-10). Слежение за файлами может молчать, поэтому
/// проверка не должна быть редкой: на неизменном проекте она дешёвая, эмбеддинг не запускается.
pub const PERIODIC_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Через сколько после пачки событий проект проверяется целиком: ловит хвост, который
/// `notify` потерял при переполнении буфера (ISS-06).
pub const TAIL_DELAY: Duration = Duration::from_secs(5);

/// Тишина после последней правки, после которой начинается автоматическая подготовка графа
/// (НФТ-7, ISS-104).
pub const GRAPH_QUIET: Duration = Duration::from_secs(90);

/// Не чаще одного автоматического прохода графа на проект за это время.
pub const GRAPH_MIN_INTERVAL: Duration = Duration::from_secs(300);

/// Безопасный текст ошибки скачивания по виду: ответ провайдера, URL и токены в окно не уходят.
fn download_error_text(kind: ModelErrorKind) -> &'static str {
    match kind {
        ModelErrorKind::Network => {
            "Не удалось скачать модель. Проверьте подключение и повторите в настройках моделей."
        }
        ModelErrorKind::FilesMissing => {
            "Файлы модели не найдены или повреждены. Скачайте их заново."
        }
        ModelErrorKind::OutOfMemory => {
            "Модели не хватило памяти. Закройте тяжёлые программы или выберите другое устройство."
        }
        ModelErrorKind::Runtime => {
            "Встроенная модель не отработала. Подробности — в журнале воркера."
        }
    }
}

/// Мьютекс, переживающий панику другого потока: состояние воркера не должно умирать от неё.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Ключ документа в очереди документов: путь от корня проекта через `/`, как в индексе.
/// `None` — файл не под корнем проекта.
fn doc_key(project: &Path, file: &Path) -> Option<String> {
    let relative = file.strip_prefix(project).ok()?;
    let parts: Option<Vec<&str>> = relative
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect();
    parts
        .filter(|parts| !parts.is_empty())
        .map(|parts| parts.join("/"))
}

#[derive(Clone)]
pub struct WorkerConfig {
    /// `~/.bars`: настройки, реестр, модели.
    pub bars_home: PathBuf,
    pub periodic_interval: Duration,
    /// Политика выгрузки встроенных сессий; не меняет индекс и настройки поиска.
    pub model_idle: ModelIdlePolicy,
    /// Сколько события слежения копятся до пачки.
    pub debounce: Duration,
    /// Через сколько после пачки событий проект проверяется целиком ([`TAIL_DELAY`]).
    pub tail_delay: Duration,
    /// Пауза после правки до embed документа; `None` — из настроек (`[indexing]
    /// edit_pause_seconds`, Т-41). Тесты задают короткую.
    pub edit_pause: Option<Duration>,
    /// Тишина без новых правок проекта, после которой начинается автоматическая подготовка графа.
    pub graph_quiet: Duration,
    /// Наименьший промежуток между началами автоматических проходов графа одного проекта.
    pub graph_min_interval: Duration,
    /// Куда установка и автообновление пишут записи клиентов; `None` — запись отключена.
    /// Приложение включает явно; тесты передают временный профиль.
    pub install_env: Option<InstallEnv>,
    /// Хранилище ключей провайдеров: сюда окно записывает ключ (`secret_set`).
    pub secrets: Arc<dyn SecretStore>,
    /// Запись автозапуска при входе в систему. Заводской воркер её не трогает: подставляет
    /// оболочка `bars` (`autostart::for_exe`), а тесты — подделку.
    pub autostart: Arc<dyn Autostart>,
}

impl std::fmt::Debug for WorkerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerConfig")
            .field("bars_home", &self.bars_home)
            .field("periodic_interval", &self.periodic_interval)
            .field("model_idle", &self.model_idle)
            .field("debounce", &self.debounce)
            .field("install_env", &self.install_env)
            .finish_non_exhaustive()
    }
}

impl WorkerConfig {
    pub fn new(bars_home: PathBuf) -> Self {
        Self {
            bars_home,
            periodic_interval: PERIODIC_INTERVAL,
            model_idle: ModelIdlePolicy::default(),
            debounce: DEBOUNCE,
            tail_delay: TAIL_DELAY,
            edit_pause: None,
            graph_quiet: GRAPH_QUIET,
            graph_min_interval: GRAPH_MIN_INTERVAL,
            install_env: None,
            secrets: Arc::new(OsSecrets::default()),
            autostart: Arc::new(NoAutostart),
        }
    }

    /// Граф начинается сразу после индексации, без паузы в правках и предела частоты.
    /// Для тестов и стендов, которые ждут автоматический граф; пауза проверяется отдельно.
    pub fn without_graph_pause(mut self) -> Self {
        self.graph_quiet = Duration::ZERO;
        self.graph_min_interval = Duration::ZERO;
        self
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    #[error("проект {0} не зарегистрирован")]
    NotRegistered(PathBuf),
    #[error("{0} не папка")]
    NotADirectory(PathBuf),
    /// Папки проекта нет на диске (`project_unavailable`).
    #[error("папки проекта {0} нет на диске")]
    Unavailable(PathBuf),
    /// Проект ждёт переиндексации после смены модели, а согласия нет (`reindex_needs_consent`).
    #[error("проект {0} ждёт переиндексации: нужно согласие пользователя")]
    ConsentRequired(PathBuf),
    #[error("ошибка файла: {0}")]
    Io(#[from] io::Error),
}

/// Состояние проекта: то, что возвращает `status` и показывает окно.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectStatus {
    pub state: ProjectState,
    /// Время последней завершённой проверки.
    pub updated_at: Option<SystemTime>,
    /// Сколько документов названо в ждущем задании. Полная проверка их не перечисляет, поэтому
    /// для неё `pending` равно нулю, а `queued` истинно.
    pub pending: usize,
    /// Проект стоит в очереди.
    pub queued: bool,
    /// Модель, которой построен индекс; `None`, пока индекс пуст или не открывался.
    pub model_index: Option<String>,
    /// Модель из настроек.
    pub model_settings: Option<String>,
    /// Последняя ошибка индексации или модели, без ключей.
    pub last_error: Option<String>,
    /// Работает ли слежение за файлами.
    pub watching: bool,
    /// Итог последней завершённой проверки.
    pub last_report: Option<UpdateReport>,
}

/// Проблемы проекта по итогам последней проверки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectIssues {
    pub rebuild_not_applied: bool,
    pub project: PathBuf,
    /// Файл и причина.
    pub files: Vec<bars_core::indexing::indexer::Problem>,
    pub last_error: Option<String>,
}

/// Ждущее задание для окна.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedJob {
    pub project: PathBuf,
    pub priority: Priority,
    pub kind: JobKind,
    pub files: usize,
}

/// Строка списка проектов.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectSnapshot {
    pub path: PathBuf,
    pub name: String,
    /// Имя совпадает с другим проектом: показывать с путём.
    pub duplicate_name: bool,
    pub registered_at: String,
    pub docs_root: String,
    pub state: ProjectState,
    /// Ближайший зарегистрированный предок (Т-49).
    pub parent: Option<PathBuf>,
    /// Составное имя `главный/путь`; только у проекта с предком.
    pub qualified_name: Option<String>,
    /// Найден автопоиском и не зарегистрирован человеком.
    pub discovered: bool,
    /// Индекса ещё не требовали (`state` — `Lazy`).
    pub lazy: bool,
    /// Обход автопоиска этого проекта оборван пределом (Т-49).
    pub discovery_truncated: bool,
    /// Живые worktree проекта-владельца (`worktrees.md`).
    pub worktrees: Vec<bars_core::worktree::LinkedWorktree>,
}

/// Итог регистрации.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registered {
    pub path: PathBuf,
    /// Проект уже был в реестре.
    pub already: bool,
}

#[derive(Debug)]
struct RuntimeStatus {
    state: ProjectState,
    updated_at: Option<SystemTime>,
    model_index: Option<String>,
    model_settings: Option<String>,
    last_error: Option<String>,
    last_report: Option<UpdateReport>,
}

struct Runtime {
    generation: u64,
    folder_revision: u64,
    deleting: bool,
    job_gate: Arc<tokio::sync::Mutex<()>>,
    status: RuntimeStatus,
    watcher: Option<ProjectWatcher>,
    mcp_until: Option<Instant>,
    windows: HashMap<String, Instant>,
    /// `notify` сообщил об ошибке или папка пропадала: слежение надо поднять заново.
    watch_failed: bool,
    /// Папки корня документации нет на диске: слежение не запускается и об этом уже записано в журнал.
    docs_root_missing: bool,
    /// Сколько сообщений слежения пришло с начала последней полной проверки. Проверка нашла
    /// изменения, а сообщений не было — слежение молчит, его надо перезапустить.
    watch_events: u32,
    /// Проверка «хвоста» уже назначена, новую не ставим.
    tail_armed: bool,
    gate: Arc<RwLock<()>>,
}

impl Runtime {
    /// Ленивая запись реестра: проект зарегистрирован, но ещё не проверялся (`Lazy`).
    fn new_lazy() -> Self {
        let mut runtime = Self::new();
        runtime.status.state = ProjectState::Lazy;
        runtime
    }

    fn new() -> Self {
        Self {
            generation: operations::generation(),
            folder_revision: 0,
            deleting: false,
            job_gate: Arc::new(tokio::sync::Mutex::new(())),
            status: RuntimeStatus {
                state: ProjectState::Ready,
                updated_at: None,
                model_index: None,
                model_settings: None,
                last_error: None,
                last_report: None,
            },
            watcher: None,
            mcp_until: None,
            windows: HashMap::new(),
            watch_failed: false,
            docs_root_missing: false,
            watch_events: 0,
            tail_armed: false,
            gate: Arc::new(RwLock::new(())),
        }
    }
}

struct Inner {
    updates: updates::Updates,
    requests: Arc<RwLock<()>>,
    config: WorkerConfig,
    registry: Mutex<Registry>,
    projects: Mutex<HashMap<PathBuf, Runtime>>,
    queue: Mutex<JobQueue>,
    /// Очередь документов: паузы после правок и состояния (`document-queue.md`).
    docs: Arc<Mutex<DocQueue>>,
    /// Очередь документов изменилась: таймер пересчитывает ближайшую паузу.
    doc_wake: Arc<Notify>,
    /// Задание вынуто из очереди и ещё выполняется.
    busy: AtomicBool,
    wake: Notify,
    stop: Arc<AtomicBool>,
    stop_wake: Notify,
    models: Models,
    handle: Handle,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    model_wake: Notify,
    /// Пауза очереди (OQ-55).
    pause: Arc<Pause>,
    /// Идущее задание: его ход и флаг остановки.
    current: Mutex<Option<Arc<JobCell>>>,
    /// Модели, которые сейчас скачиваются по кнопке окна.
    downloads: Mutex<HashSet<String>>,
    download_errors: Mutex<HashMap<String, ModelFailure>>,
    graph_jobs: Mutex<HashMap<PathBuf, graph::GraphJob>>,
    /// Начало последнего автоматического прохода графа проекта: предел частоты.
    graph_last_start: Mutex<HashMap<PathBuf, std::time::Instant>>,
    /// Сколько раз начинался новый проход графа (для проверок планировщика).
    #[cfg(test)]
    graph_passes: std::sync::atomic::AtomicUsize,
    graph_cache: Arc<crate::handler::graph_cache::GraphCache>,
    scans: Mutex<HashMap<PathBuf, scan::ScanJob>>,
    /// Автопоиск подпроектов: один проход на процесс и очередь запросов (`subprojects.rs`).
    discovery: subprojects::Gate,
    /// Живые worktree владельцев (`worktrees.rs`): только память, на диск попадает дельта.
    worktrees: Mutex<HashMap<PathBuf, Vec<bars_core::worktree::LinkedWorktree>>>,
    /// Состояния и очередь дельт worktree (`delta.rs`).
    deltas: Mutex<delta::Deltas>,
    /// В очереди дельт появилось задание или освободилась очередь проектов.
    delta_wake: Notify,
    /// Сверка дельты идёт: очередь проектов ждёт (индексация идёт по одному заданию).
    delta_busy: AtomicBool,
    /// Слежение и аренды worktree (`wt_watch.rs`): те же правила Т-10, что у проектов.
    wt_watch: Mutex<wt_watch::WtWatches>,
}

struct DownloadCleanup {
    inner: Arc<Inner>,
    id: String,
}
impl Drop for DownloadCleanup {
    fn drop(&mut self) {
        lock(&self.inner.downloads).remove(&self.id);
    }
}

/// Воркер. Копии указывают на одно и то же состояние.
#[derive(Clone)]
pub struct Worker {
    inner: Arc<Inner>,
}

impl Worker {
    pub(crate) fn is_stopping(&self) -> bool {
        self.inner.stop.load(Ordering::SeqCst)
    }

    pub(crate) async fn stopping(&self) {
        let notified = self.inner.stop_wake.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if !self.is_stopping() {
            notified.await;
        }
    }

    pub(crate) fn spawn_task(
        &self,
        future: impl std::future::Future<Output = ()> + Send + 'static,
    ) {
        let mut tasks = lock(&self.inner.tasks);
        if !self.is_stopping() {
            tasks.push(self.inner.handle.spawn(future));
        }
    }
    pub(crate) fn graph_cache(&self) -> Arc<crate::handler::graph_cache::GraphCache> {
        self.inner.graph_cache.clone()
    }
    pub fn model_runtime(&self) -> Option<bars_protocol::ModelsRuntime> {
        let user = bars_core::settings::UserSettings::load(self.bars_home())
            .ok()?
            .value;
        let settings = bars_core::settings::Settings::resolve(&user, &Default::default());
        self.inner.models.runtime(&settings)
    }
    /// Читает реестр, запускает очередь и часовой таймер, ставит все проекты на проверку при
    /// старте (Т-10). Вызывается внутри среды tokio. Битый `projects.toml` — ошибка: реестр
    /// не заменяется пустым.
    pub fn start(
        config: WorkerConfig,
        provider: Arc<dyn ModelProvider>,
    ) -> Result<Self, WorkerError> {
        let registry = Registry::load(&config.bars_home)?;
        let paths = registry.paths();
        let inner = Arc::new(Inner {
            updates: Default::default(),
            requests: Arc::new(RwLock::new(())),
            config,
            registry: Mutex::new(registry),
            projects: Mutex::new(HashMap::new()),
            queue: Mutex::new(JobQueue::new()),
            docs: Arc::new(Mutex::new(DocQueue::new())),
            doc_wake: Arc::new(Notify::new()),
            busy: AtomicBool::new(false),
            wake: Notify::new(),
            stop: Arc::new(AtomicBool::new(false)),
            models: Models::new(provider),
            handle: Handle::current(),
            tasks: Mutex::new(Vec::new()),
            model_wake: Notify::new(),
            stop_wake: Notify::new(),
            pause: Arc::new(Pause::default()),
            current: Mutex::new(None),
            downloads: Mutex::new(HashSet::new()),
            download_errors: Mutex::new(HashMap::new()),
            graph_jobs: Mutex::new(HashMap::new()),
            graph_last_start: Mutex::new(HashMap::new()),
            #[cfg(test)]
            graph_passes: std::sync::atomic::AtomicUsize::new(0),
            graph_cache: Default::default(),
            scans: Mutex::new(HashMap::new()),
            discovery: Default::default(),
            worktrees: Mutex::new(HashMap::new()),
            deltas: Mutex::new(delta::Deltas::default()),
            delta_wake: Notify::new(),
            delta_busy: AtomicBool::new(false),
            wt_watch: Mutex::new(HashMap::new()),
        });
        // Очередь ещё не идёт: никто не держит файлы индекса, восстановление безопасно.
        for path in &paths {
            recover_project_index(path);
        }
        // До первой проверки: внешние проекты исключают области вложенных и папки worktree.
        inner.discover_worktrees(None);
        inner.refresh_nested();
        let queue_task = tokio::spawn(Arc::clone(&inner).run_queue());
        let timer_task = tokio::spawn(Arc::clone(&inner).run_timer());
        let doc_task = tokio::spawn(Arc::clone(&inner).run_doc_timer());
        let activity_task = tokio::spawn(Arc::clone(&inner).run_activity_timer());
        let model_task = tokio::spawn(Arc::clone(&inner).run_model_maintenance());
        lock(&inner.tasks).extend([
            queue_task,
            timer_task,
            doc_task,
            model_task,
            activity_task,
        ]);
        for path in paths {
            inner.activate(&path, Priority::Periodic);
        }
        // После проверок файлов и без ожидания канала: обход подпроектов идёт в фоне.
        inner.schedule_discovery_all();
        // Очередь дельт worktree — после активации проектов: проверка старта видит их состояния.
        // Последней в списке: `shutdown` прерывает таймеры по номеру задания.
        let delta_task = tokio::spawn(Arc::clone(&inner).run_delta_queue());
        lock(&inner.tasks).push(delta_task);
        Ok(Self { inner })
    }

    /// Регистрирует проход в жизненном цикле воркера: выход из трея тоже ждёт его.
    pub(crate) fn refresh_clients(&self) {
        let Some(env) = self.install_env().cloned() else {
            return;
        };
        let mut tasks = lock(&self.inner.tasks);
        if self.inner.stop.load(Ordering::SeqCst) {
            return;
        }
        let projects = self.gate_projects(None);
        tasks.push(tokio::task::spawn_blocking(move || {
            for outcome in crate::clients::refresh(&env) {
                crate::clients::report(outcome.client, "обновлено", &outcome.result);
            }
            crate::clients::report_gates(&crate::clients::sync_project_gates(&env, &projects));
        }));
    }

    /// Ставит гейт чтения документации в проект (или во все проекты), которые знает Claude Code.
    pub(crate) fn sync_project_gates(&self, only: Option<&Path>) {
        let Some(env) = self.install_env().cloned() else {
            return;
        };
        let projects = self.gate_projects(only);
        let mut tasks = lock(&self.inner.tasks);
        if projects.is_empty() || self.inner.stop.load(Ordering::SeqCst) {
            return;
        }
        tasks.push(tokio::task::spawn_blocking(move || {
            crate::clients::report_gates(&crate::clients::sync_project_gates(&env, &projects));
        }));
    }

    fn gate_projects(&self, only: Option<&Path>) -> Vec<crate::clients::GateProject> {
        self.projects()
            .into_iter()
            .filter(|snapshot| only.is_none_or(|path| snapshot.path == path))
            .map(|snapshot| crate::clients::GateProject {
                path: snapshot.path,
                docs_root: snapshot.docs_root,
            })
            .collect()
    }

    /// Останавливает индексацию на границе документа, снимает слежение и выгружает модели.
    pub async fn shutdown(&self) {
        self.inner.stop.store(true, Ordering::SeqCst);
        self.inner.stop_wake.notify_waiters();
        self.inner.graph_cache.clear();
        self.inner.model_wake.notify_one();
        self.inner.cancel_current();
        self.inner.pause.set(false);
        self.inner.wake.notify_one();
        self.inner.delta_wake.notify_one();
        let tasks: Vec<_> = lock(&self.inner.tasks).drain(..).collect();
        // Первая — очередь: она дописывает текущий документ. Вторая — таймер, он ждёт часами;
        // третья — таймер пауз документов, он тоже не нужен после остановки.
        for (position, task) in tasks.into_iter().enumerate() {
            if position == 1 || position == 2 {
                task.abort();
            }
            let _ = task.await;
        }
        // Завершающаяся публикация могла заполнить кэш после первого clear.
        self.inner.graph_cache.clear();
        let watchers: Vec<_> = lock(&self.inner.projects)
            .values_mut()
            .filter_map(|runtime| runtime.watcher.take())
            .collect();
        drop(watchers);
        let inner = Arc::clone(&self.inner);
        if tokio::task::spawn_blocking(move || inner.models.clear())
            .await
            .is_err()
        {
            log::error!("Не удалось выгрузить модели при завершении");
        }
        log::info!("Воркер остановлен");
    }

    // --- реестр ---

    /// Регистрирует папку как проект: ровно ту, что указана (Т-6). Создаёт `.bars/config.toml`,
    /// если его нет, запускает слежение и ставит проект на проверку. Повторная регистрация
    /// ставит на проверку ещё раз. Найденный автопоиском подпроект становится самостоятельным:
    /// запись одна, индекс один (Т-49).
    pub fn register(&self, path: &Path) -> Result<Registered, WorkerError> {
        let _request =
            self.inner.requests.try_read().map_err(|_| {
                io::Error::other("идёт удаление проекта; повторите регистрацию позже")
            })?;
        let path = normalize(path)?;
        let _publication = self.inner.updates.after(&path, updates::ALL);
        if !path.is_dir() {
            return Err(WorkerError::NotADirectory(path));
        }
        // Корень документации нового проекта: умолчание Т-7, если клиент ничего не записал.
        bars_protocol::prepare_project(&path, None)?;
        let (canonical, added) = {
            let mut registry = lock(&self.inner.registry);
            let added = registry.add(&path)?;
            let canonical = registry
                .find(&path)
                .map_or_else(|| path.clone(), |entry| entry.path.clone());
            if !added {
                registry.set_standalone(&canonical)?;
            }
            (canonical, added)
        };
        // Новый вложенный проект уходит из индекса внешнего, внешний новый — исключает вложенные.
        self.inner.refresh_nested();
        // Worktree нового проекта — до первой проверки: их папки не должны попасть в индекс.
        self.rediscover_worktrees(&canonical);
        // У нового самостоятельного проекта лениво хранится только то, что нашёл автопоиск: после
        // `set_standalone` состояние `Lazy` в `Runtime` снимает `activate`.
        self.inner.activate(&canonical, Priority::Explicit);
        // Новый главный сразу получает свои подпроекты.
        self.inner.schedule_discovery(&canonical, false);
        Ok(Registered {
            path: canonical,
            already: !added,
        })
    }

    /// Меняет отображаемое имя, сохраняя путь и состояние проекта.
    pub fn rename_project(&self, selector: &str, name: String) -> Result<(), WorkerError> {
        let mut registry = lock(&self.inner.registry);
        let path = registry.resolve(selector)?.path.clone();
        registry.rename(&path, name)?;
        Ok(())
    }

    /// Снимает проект с регистрации, сохраняя `.bars/` и документы (Т-5). Найденные записи без
    /// предка убираются; бывший предок обходится заново: подпроект, отвечающий правилу, вернётся
    /// найденным ленивым (Т-49).
    pub fn unregister(&self, selector: &str) -> Result<PathBuf, WorkerError> {
        let _request = self.inner.requests.try_read().map_err(|_| {
            io::Error::other("идёт удаление проекта; повторите снятие регистрации позже")
        })?;
        let path = self.resolve_known(selector)?;
        let former_parent = lock(&self.inner.registry)
            .parent_of(&path)
            .map(|parent| parent.path.clone());
        self.inner.remove_entry(&path)?;
        self.inner.remove_orphans();
        // Снятый вложенный проект возвращается во внешний: тот получает проверку.
        self.inner.refresh_nested();
        if let Some(parent) = former_parent {
            self.inner.schedule_discovery(&parent, false);
        }
        Ok(path)
    }

    /// Проект по пути, имени или составному имени `главный/путь`; одинаковое имя у нескольких
    /// проектов — ошибка. Неизвестный подпроект, который отвечает правилу автопоиска, регистрируется
    /// сразу найденным ленивым: одна проверка путей без обхода дерева (Т-49).
    pub fn resolve(&self, selector: &str) -> Result<PathBuf, WorkerError> {
        let error = match self.resolve_known(selector) {
            Ok(path) => return Ok(path),
            Err(WorkerError::Resolve(error)) => error,
            Err(other) => return Err(other),
        };
        if matches!(
            error,
            ResolveError::NotFound(_) | ResolveError::UnknownSubproject { .. }
        ) && self.register_for_selector(selector).is_some()
            && let Ok(path) = self.resolve_known(selector)
        {
            return Ok(path);
        }
        Err(self.not_a_subproject(selector, error).into())
    }

    /// То же, что [`Worker::resolve`], но реестр не пополняется: для снятия и удаления.
    pub(crate) fn resolve_known(&self, selector: &str) -> Result<PathBuf, WorkerError> {
        Ok(lock(&self.inner.registry).resolve(selector)?.path.clone())
    }

    /// Ближайший зарегистрированный проект от `cwd` вверх по дереву (Т-15); для `cwd` в живом
    /// worktree — его владелец.
    pub fn project_for_cwd(&self, cwd: &Path) -> Option<PathBuf> {
        self.resolve_cwd(cwd).map(|(path, _)| path)
    }

    /// Проект `cwd` и, если `cwd` лежит в корне вида живого worktree, этот worktree
    /// ([`Registry::resolve_cwd`]).
    pub fn resolve_cwd(&self, cwd: &Path) -> Option<(PathBuf, Option<WorktreeRef>)> {
        lock(&self.inner.registry)
            .resolve_cwd(cwd)
            .map(|target| (target.project.path.clone(), target.worktree))
    }

    /// Привязка меню папки: новая регистрация или смена docs_root инвалидирует цель.
    pub fn folder_identity(&self, path: &Path) -> Result<String, WorkerError> {
        let path = self.registered(path)?;
        let projects = lock(&self.inner.projects);
        let runtime = projects
            .get(&path)
            .filter(|r| !r.deleting)
            .ok_or_else(|| WorkerError::NotRegistered(path.clone()))?;
        Ok(format!(
            "{}:{}:{}",
            runtime.generation,
            runtime.folder_revision,
            self.inner.docs_root_of(&path)
        ))
    }

    /// Корень документации зарегистрированного проекта относительно его корня; окно проверяет им пути записи.
    pub fn docs_root(&self, path: &Path) -> Result<String, WorkerError> {
        let path = self.registered(path)?;
        Ok(self.inner.docs_root_of(&path))
    }

    /// Короткое действие окна не должно пересечь удаление/смену настроек проекта.
    pub async fn folder_action_guard(&self) -> tokio::sync::OwnedRwLockWriteGuard<()> {
        Arc::clone(&self.inner.requests).write_owned().await
    }

    /// Имя и docs_root только одного канонического проекта, без обхода чужих настроек.
    pub(crate) fn project_metadata(&self, path: &Path) -> Option<(String, String)> {
        let name = lock(&self.inner.registry)
            .entries()
            .iter()
            .find(|entry| entry.path == path)?
            .display_name();
        Some((name, self.inner.docs_root_of(path)))
    }

    /// Вложенные проекты, чья область исключена из индекса `project` (Т-7). `count` добавляет число
    /// `.md` в области по диску: для предупреждения в окне; иначе поле 0.
    pub fn nested_projects(
        &self,
        project: &Path,
        count: bool,
    ) -> Vec<bars_protocol::NestedProjectInfo> {
        bars_core::indexing::nested::areas(project)
            .into_iter()
            // Папки worktree — не вложенные проекты: поиск по ним идёт параметром `wt`.
            .filter(|area| area.worktree.is_none())
            .map(|area| {
                // Вложенный проект — подпроект: агент выбирает его составным именем (Т-49).
                let registry = lock(&self.inner.registry);
                let known = registry.qualified_name(&area.project).or_else(|| {
                    registry
                        .find(&area.project)
                        .map(ProjectEntry::display_name)
                });
                drop(registry);
                let name = known.unwrap_or_else(|| {
                    area.project
                        .file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
                });
                let documents = if count {
                    let docs_root = self.inner.docs_root_of(&area.project);
                    bars_core::indexing::walk::walk_docs(&area.project, Path::new(&docs_root))
                        .map_or(0, |walk| {
                            u32::try_from(walk.files.len()).unwrap_or(u32::MAX)
                        })
                } else {
                    0
                };
                bars_protocol::NestedProjectInfo {
                    name,
                    path: area.project.display().to_string(),
                    dir: area.project_dir,
                    area: area.dir,
                    documents,
                }
            })
            .collect()
    }

    /// Все проекты реестра с состоянием, в порядке регистрации.
    pub fn projects(&self) -> Vec<ProjectSnapshot> {
        // Предок и составное имя считаются по всему реестру под одной блокировкой.
        let entries: Vec<(ProjectEntry, bool, Option<PathBuf>, Option<String>)> = {
            let registry = lock(&self.inner.registry);
            registry
                .entries()
                .iter()
                .map(|entry| {
                    (
                        entry.clone(),
                        registry.has_duplicate_name(&entry.path),
                        registry
                            .parent_of(&entry.path)
                            .map(|parent| parent.path.clone()),
                        registry.qualified_name(&entry.path),
                    )
                })
                .collect()
        };
        entries
            .into_iter()
            .map(|(entry, duplicate_name, parent, qualified_name)| {
                // Ленивая запись ещё не проверялась: состояние берётся из реестра, не из `Runtime`.
                let state = if entry.lazy {
                    ProjectState::Lazy
                } else {
                    lock(&self.inner.projects).get(&entry.path).map_or(
                        ProjectState::Unavailable,
                        |runtime| {
                            if runtime.status.state == ProjectState::Ready
                                && lock(&self.inner.scans).contains_key(&entry.path)
                            {
                                ProjectState::Indexing
                            } else {
                                runtime.status.state
                            }
                        },
                    )
                };
                ProjectSnapshot {
                    name: entry.display_name(),
                    duplicate_name,
                    docs_root: self.inner.docs_root_of(&entry.path),
                    registered_at: entry.registered_at,
                    state,
                    parent,
                    qualified_name,
                    discovered: entry.discovered,
                    lazy: entry.lazy,
                    discovery_truncated: self.inner.discovery_truncated(&entry.path),
                    worktrees: self.inner.linked_of(&entry.path),
                    path: entry.path,
                }
            })
            .collect()
    }

    /// Хук начала сессии: находит корень проекта по `cwd` ([`Registry::discover_root`]), при
    /// необходимости регистрирует его и ставит на проверку. `None` — `cwd` не в проекте и
    /// документации для регистрации нет: реестр не засоряется.
    pub fn session_start(&self, cwd: &Path) -> Result<Option<PathBuf>, WorkerError> {
        let cwd = normalize(cwd)?;
        // Точечная проверка папок от `cwd` вверх до проекта: подпроект, о котором реестр ещё не
        // знает, регистрируется найденным ленивым, и проектом сессии становится он (Т-49).
        self.register_for_cwd(&cwd);
        // Рабочая копия живого worktree без зарегистрированного владельца — `None`: хук не
        // регистрирует ни копию, ни основной checkout (OQ-307).
        let Some(discovered) = lock(&self.inner.registry).discover_root(&cwd) else {
            return Ok(None);
        };
        if discovered.registered {
            self.observe_mcp(&discovered.root)?;
            let _ = self.scan_receipt(&discovered.root);
            // Worktree владельца и вложенных проектов перечитываются: только что созданный
            // `git worktree add` виден сразу (первое обращение к виду — шаг 6.3.2).
            self.rediscover_worktrees(&discovered.root);
            // Сессия в W — первое обращение к виду: дельта строится лениво (Explicit).
            if let Some((owner, Some(reference))) = self.resolve_cwd(&cwd) {
                self.ensure_worktree_delta(&owner, &reference.name);
            }
            self.sync_project_gates(Some(&discovered.root));
            return Ok(Some(discovered.root));
        }
        // Хук сам регистрирует только папку с настроенным проектом (`.bars/config.toml`) или с
        // `docs/`; папку без них регистрирует лишь явный выбор человека (Т-6, Т-7, OQ-253).
        let docs_root = if project_config_path(&discovered.root).exists() {
            self.inner.docs_root_of(&discovered.root)
        } else if discovered.root.join(DEFAULT_DOCS_ROOT).is_dir() {
            DEFAULT_DOCS_ROOT.to_string()
        } else {
            return Ok(None);
        };
        if !discovered.root.is_dir() || !has_markdown(&discovered.root, &docs_root) {
            return Ok(None);
        }
        let root = self.register(&discovered.root)?.path;
        self.observe_mcp(&root)?;
        self.sync_project_gates(Some(&root));
        Ok(Some(root))
    }

    // --- состояние и переиндексация ---

    /// Состояние зарегистрированного проекта.
    pub fn status(&self, path: &Path) -> Result<ProjectStatus, WorkerError> {
        let path = self.registered(path)?;
        let projects = lock(&self.inner.projects);
        let runtime = projects
            .get(&path)
            .ok_or_else(|| WorkerError::NotRegistered(path.clone()))?;
        let queue = lock(&self.inner.queue);
        let queued = queue.get(&path);
        let counts = lock(&self.inner.docs).counts(&path);
        let live_docs = counts.waiting + counts.queued + counts.indexing;
        let status = &runtime.status;
        let scanning = lock(&self.inner.scans).contains_key(&path);
        Ok(ProjectStatus {
            state: if scanning && status.state == ProjectState::Ready {
                ProjectState::Indexing
            } else {
                status.state
            },
            updated_at: status.updated_at,
            // Записи очереди документов — это и есть ожидающие документы; без них — документы,
            // названные в ждущем задании.
            pending: if live_docs > 0 {
                live_docs
            } else {
                queued.map_or(0, Job::listed_files)
            },
            queued: queued.is_some(),
            model_index: status.model_index.clone(),
            model_settings: status.model_settings.clone(),
            last_error: status.last_error.clone(),
            watching: runtime.watcher.is_some()
                && !runtime.watch_failed
                && status.state != ProjectState::Unavailable,
            last_report: status.last_report.clone(),
        })
    }

    /// Явная переиндексация (Т-25). `full` строит индекс заново; `accept_model_change` — согласие
    /// человека после смены модели. Проект в состоянии `reindex_required` без согласия
    /// не переиндексируется ни при каком `full`: [`WorkerError::ConsentRequired`].
    pub fn reindex(
        &self,
        path: &Path,
        full: bool,
        accept_model_change: bool,
    ) -> Result<(), WorkerError> {
        self.reindex_receipt(path, full, accept_model_change)
            .map(|_| ())
    }

    /// Проверка с ID принятой/объединённой операции для уведомления окна.
    pub fn reindex_receipt(
        &self,
        path: &Path,
        full: bool,
        accept_model_change: bool,
    ) -> Result<(u64, bool, JobKind), WorkerError> {
        let path = self.registered(path)?;
        // Явная проверка — обращение к проекту, в том числе из CLI: ленивый проект просыпается.
        self.inner.wake_lazy(&path);
        let _publication = self.inner.updates.after(&path, updates::STATUS);
        let key = self.inner.operation_settings_key(&path);
        let projects = lock(&self.inner.projects);
        let state = projects
            .get(&path)
            .map(|runtime| runtime.status.state)
            .ok_or_else(|| WorkerError::NotRegistered(path.clone()))?;
        match state {
            ProjectState::Unavailable => return Err(WorkerError::Unavailable(path)),
            ProjectState::ReindexRequired if !accept_model_change => {
                return Err(WorkerError::ConsentRequired(path));
            }
            _ => {}
        }
        let job = if full || state == ProjectState::ReindexRequired {
            Job::rebuild(accept_model_change)
        } else {
            Job::check(Priority::Explicit)
        };
        self.inner
            .enqueue_receipt(&path, job, &projects, key)
            .ok_or(WorkerError::NotRegistered(path))
    }

    // --- пауза, остановка, ход ---

    /// Явная проверка файлов из меню документа; смену модели не подтверждает.
    pub fn check_files(&self, path: &Path, files: Vec<PathBuf>) -> Result<(), WorkerError> {
        let path = self.registered(path)?;
        match self.status(&path)?.state {
            ProjectState::Unavailable => return Err(WorkerError::Unavailable(path)),
            ProjectState::ReindexRequired => return Err(WorkerError::ConsentRequired(path)),
            _ => {}
        }
        if !files.is_empty() {
            self.inner
                .enqueue(&path, Job::files(Priority::Explicit, files));
        }
        Ok(())
    }

    /// Приостанавливает очередь целиком: идущее задание замирает между документами, новые не
    /// берутся (OQ-55).
    pub fn pause(&self) {
        self.inner.pause.set(true);
    }

    /// Снимает паузу.
    pub fn resume(&self) {
        self.inner.pause.set(false);
        self.inner.wake.notify_one();
    }

    pub fn is_paused(&self) -> bool {
        self.inner.pause.is_paused()
    }

    /// Останавливает индексацию проекта: идущее задание прерывается между документами, ждущее
    /// снимается с очереди. Старый индекс остаётся, проект в `reindex_required` остаётся в нём
    /// (OQ-55). `true`, если было что останавливать.
    pub fn cancel(&self, path: &Path) -> Result<bool, WorkerError> {
        let path = self.registered(path)?;
        // Публикация scan и постановка обновления используют тот же projects-lock.
        let _projects = lock(&self.inner.projects);
        let running = {
            let current = lock(&self.inner.current);
            current
                .as_ref()
                .filter(|cell| cell.project == path)
                .map(|cell| cell.cancel())
                .is_some()
        };
        let scanning = self.inner.cancel_scan(&path);
        let queued = self.inner.cancel_queued(&path);
        // Остановка проекта останавливает и сверки дельт его worktree, и слежение за W: состояния
        // вернутся проверкой старта или следующим обращением.
        self.inner.cancel_deltas(&path);
        Ok(running || queued || scanning)
    }

    /// Ход идущего задания.
    pub fn progress(&self) -> Option<JobProgress> {
        lock(&self.inner.current)
            .as_ref()
            .map(|cell| cell.progress())
    }

    /// Ждущие задания в порядке выполнения.
    pub fn queued(&self) -> Vec<QueuedJob> {
        lock(&self.inner.queue)
            .snapshot()
            .into_iter()
            .map(|(project, job)| QueuedJob {
                project,
                priority: job.priority,
                kind: JobKind::of(&job),
                files: job.listed_files(),
            })
            .collect()
    }

    /// Правка документов в окне bars: они ждут паузы Т-41 и идут в очередь с приоритетом правки
    /// (Т-42). Файл, который уже виден слежению, остаётся одной записью.
    pub fn documents_edited(&self, path: &Path, files: Vec<PathBuf>) -> Result<(), WorkerError> {
        let path = self.registered(path)?;
        if !files.is_empty() && self.status(&path)?.state != ProjectState::Unavailable {
            self.inner.touch_files(&path, files, DocSource::Edit);
        }
        Ok(())
    }

    /// Очередь документов проекта для страницы индексации: до `limit` записей в порядке
    /// выполнения и счётчики по состояниям.
    pub fn doc_queue(
        &self,
        path: &Path,
        limit: usize,
    ) -> Result<(Vec<DocView>, DocCounts), WorkerError> {
        let path = self.registered(path)?;
        let docs = lock(&self.inner.docs);
        Ok((
            docs.snapshot(&path, Instant::now(), limit),
            docs.counts(&path),
        ))
    }

    /// «Повторить» для документа с ошибкой: он снова готов к индексации. `false` — записи нет
    /// или она не в ошибке.
    pub fn doc_retry(&self, path: &Path, document: &str) -> Result<bool, WorkerError> {
        let path = self.registered(path)?;
        let retried = lock(&self.inner.docs).retry(&path, document);
        if retried {
            let ready = lock(&self.inner.docs).ready_of(&path);
            if let Some(ready) = ready {
                self.inner.submit_docs(&path, ready.paths, ready.source);
            }
        }
        Ok(retried)
    }

    /// Проблемы проектов по итогам последней проверки: файлы с ошибками и `last_error`.
    pub fn problems(&self) -> Vec<ProjectIssues> {
        let projects = lock(&self.inner.projects);
        let mut out: Vec<_> = projects
            .iter()
            .filter_map(|(path, runtime)| {
                let mut files = runtime
                    .status
                    .last_report
                    .as_ref()
                    .map(|report| report.problems.clone())
                    .unwrap_or_default();
                if let Some(current) = lock(&self.inner.current)
                    .as_ref()
                    .filter(|cell| cell.project == *path)
                {
                    for problem in current.report_snapshot().problems {
                        files.retain(|old| old.path != problem.path);
                        files.push(problem);
                    }
                }
                let error = runtime.status.last_error.clone();
                (!files.is_empty() || error.is_some()).then(|| ProjectIssues {
                    rebuild_not_applied: runtime
                        .status
                        .last_report
                        .as_ref()
                        .is_some_and(|r| r.rebuild_not_applied),
                    project: path.clone(),
                    files,
                    last_error: error,
                })
            })
            .collect();
        out.sort_by(|a, b| a.project.cmp(&b.project));
        out
    }

    /// Меняет корень документации проекта: запись в `.bars/config.toml`, слежение переезжает,
    /// проект ставится на полную проверку (нарезка и корень в индексе записаны, воркер увидит
    /// расхождение). Согласие не нужно: модель та же.
    pub fn set_docs_root(&self, path: &Path, docs_root: &str) -> Result<(), WorkerError> {
        let _publication = self.inner.updates.after(path, updates::ALL);
        let _request = self.inner.requests.try_read().map_err(|_| {
            io::Error::other("идёт удаление проекта; повторите изменение настроек позже")
        })?;
        let path = self.registered(path)?;
        if !path.is_dir() {
            return Err(WorkerError::Unavailable(path));
        }
        crate::settings_io::save_docs_root(&path, docs_root).map_err(|error| {
            WorkerError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                error.to_string(),
            ))
        })?;
        let old = lock(&self.inner.projects)
            .get_mut(&path)
            .and_then(|runtime| {
                runtime.folder_revision += 1;
                runtime.watcher.take()
            });
        // Слежение останавливается вне блокировки: его поток может ждать её.
        drop(old);
        self.inner.ensure_watching(&path);
        self.inner.enqueue(&path, Job::check(Priority::Explicit));
        // Новая область вложенного проекта меняет исключения внешних.
        self.inner.refresh_nested();
        // Смена корня документации меняет «вне корня документации»: подпроекты ищутся заново.
        self.inner.schedule_discovery(&path, false);
        Ok(())
    }

    /// Ставит на проверку все проекты: то же, что делает часовой таймер. Пропавшие проекты
    /// становятся `unavailable`, вернувшиеся — снова в работу.
    pub fn check_all(&self) {
        self.inner.check_all();
    }

    // --- для поиска ---

    /// Доступ к проекту. Поиск держит его на чтение, пока открывает индекс; подмена файла при
    /// полной переиндексации берёт на запись: новые поиски ждут, текущие дочитывают.
    pub fn gate(&self, path: &Path) -> Result<Arc<RwLock<()>>, WorkerError> {
        let path = self.registered(path)?;
        lock(&self.inner.projects)
            .get(&path)
            .map(|runtime| Arc::clone(&runtime.gate))
            .ok_or(WorkerError::NotRegistered(path))
    }

    /// Менеджер моделей: embed-модель для эмбеддинга запроса поиска.
    pub fn models(&self) -> &Models {
        &self.inner.models
    }

    /// Хранилище ключей провайдеров.
    pub fn autostart(&self) -> &Arc<dyn Autostart> {
        &self.inner.config.autostart
    }

    pub fn secrets(&self) -> &Arc<dyn SecretStore> {
        &self.inner.config.secrets
    }

    /// Папка моделей (`~/.bars/models`).
    pub fn models_dir(&self) -> PathBuf {
        self.inner.config.bars_home.join("models")
    }

    /// Модели, которые скачиваются или грузятся прямо сейчас: по кнопке «Скачать» и для поиска.
    pub fn downloading(&self) -> HashSet<String> {
        let mut ids = lock(&self.inner.downloads).clone();
        ids.extend(self.inner.models.loading_ids());
        ids
    }

    pub fn download_errors(&self) -> HashMap<String, ModelFailure> {
        lock(&self.inner.download_errors).clone()
    }

    /// Скачивает встроенную модель в фоне. `false`, если она уже скачивается. Загрузка идёт в
    /// блокирующей задаче: окно узнаёт о конце по `models_info`.
    pub fn start_download(&self, id: &str, reranker: bool) -> bool {
        if !lock(&self.inner.downloads).insert(id.to_string()) {
            return false;
        }
        lock(&self.inner.download_errors).remove(id);
        let inner = Arc::clone(&self.inner);
        let id = id.to_string();
        let mut user = bars_core::settings::UserSettings::load(self.bars_home())
            .map(|s| s.value)
            .unwrap_or_default();
        if reranker {
            user.rerank.source = bars_core::settings::ModelSource::Builtin;
            user.rerank.model.clone_from(&id);
        } else {
            user.embed.source = bars_core::settings::ModelSource::Builtin;
            user.embed.model.clone_from(&id);
        }
        let settings = Settings::resolve(&user, &bars_core::settings::ProjectConfig::default());
        let operation = models::begin_model(
            &self.inner.models.operations,
            &settings,
            reranker,
            "download",
        );
        self.inner.handle.spawn_blocking(move || {
            let _attempt = models::ModelAttempt(Arc::clone(&inner.models.operations), operation);
            let _download = DownloadCleanup {
                inner: Arc::clone(&inner),
                id: id.clone(),
            };
            log::info!("Начато скачивание модели {id}");
            let outcome = inner
                .models
                .download_operation(&settings, reranker, Some(operation));
            let success = outcome.is_ok();
            if let Err(failure) = outcome {
                log::error!(
                    "Не удалось скачать модель {id} ({}); подробности — в состоянии модели",
                    failure.kind.as_str()
                );
                // В UI не передаём произвольный ответ провайдера, URL или токены: только вид ошибки
                // и фиксированный текст по нему.
                lock(&inner.download_errors).insert(
                    id.clone(),
                    ModelFailure::new(failure.kind, download_error_text(failure.kind)),
                );
            } else {
                log::info!("Скачивание модели {id} завершено");
            }
            lock(&inner.downloads).remove(&id);
            models::finish_model(&inner.models.operations, operation, success);
        });
        true
    }

    /// Пути установки в клиенты (`install`, `uninstall`).
    pub fn install_env(&self) -> Option<&InstallEnv> {
        self.inner.config.install_env.as_ref()
    }

    pub fn bars_home(&self) -> &Path {
        &self.inner.config.bars_home
    }

    /// Очередь пуста, ничего не выполняется и ни один документ не ждёт паузы или индексации.
    pub fn is_idle(&self) -> bool {
        let queue = lock(&self.inner.queue);
        queue.is_empty()
            && !lock(&self.inner.docs).has_pending()
            && !self.inner.busy.load(Ordering::SeqCst)
            && lock(&self.inner.scans).is_empty()
            && lock(&self.inner.deltas).idle()
            && lock(&self.inner.graph_jobs)
                .values()
                .all(graph::GraphJob::idle)
    }

    fn registered(&self, path: &Path) -> Result<PathBuf, WorkerError> {
        lock(&self.inner.registry)
            .find(path)
            .map(|entry| entry.path.clone())
            .ok_or_else(|| WorkerError::NotRegistered(path.to_path_buf()))
    }
}

impl Inner {
    fn docs_root_of(&self, project: &Path) -> String {
        Settings::load(&self.config.bars_home, project).map_or_else(
            |_| DEFAULT_DOCS_ROOT.to_string(),
            |loaded| loaded.value.docs_root,
        )
    }

    fn cancel_current(&self) {
        if let Some(cell) = lock(&self.current).as_ref() {
            cell.cancel();
        }
    }

    fn cancel_queued(&self, path: &Path) -> bool {
        let dropped = lock(&self.docs).remove_project(path) > 0;
        let mut queue = lock(&self.queue);
        let id = queue.get(path).map(|job| job.operation_id);
        let removed = queue.remove(path) || dropped;
        if let Some(id) = id {
            lock(&self.models.operations).finish(id, "cancelled", None, None);
        }
        removed
    }

    fn enqueue(&self, project: &Path, job: Job) {
        let _publication = self.updates.after(project, updates::STATUS);
        let key = self.operation_settings_key(project);
        let projects = lock(&self.projects);
        self.enqueue_for(project, job, &projects, key);
    }

    fn operation_settings_key(&self, project: &Path) -> u64 {
        Settings::load(&self.config.bars_home, project)
            .ok()
            .map(|settings| operations::settings_key(&settings.value))
            .unwrap_or(0)
    }

    fn enqueue_for(
        &self,
        project: &Path,
        job: Job,
        projects: &HashMap<PathBuf, Runtime>,
        key: u64,
    ) {
        self.enqueue_receipt(project, job, projects, key);
    }

    fn enqueue_receipt(
        &self,
        project: &Path,
        mut job: Job,
        projects: &HashMap<PathBuf, Runtime>,
        key: u64,
    ) -> Option<(u64, bool, JobKind)> {
        if !projects
            .get(project)
            .is_some_and(|runtime| !runtime.deleting)
        {
            return None;
        }
        let mut queue = lock(&self.queue);
        let mut journal = lock(&self.models.operations);
        let was_merged = queue.get(project).is_some();
        job.operation_id = queue
            .get(project)
            .map(|job| job.operation_id)
            .unwrap_or_else(|| {
                journal.begin(
                    "check",
                    "explicit",
                    Some(project.display().to_string()),
                    projects[project].generation,
                    None,
                    None,
                    false,
                    key,
                )
            });
        let id = job.operation_id;
        queue.push(project, job);
        let merged = queue.get(project).expect("задание поставлено");
        journal.edit(id, |op| {
            op.docs_total = match &merged.scope {
                queue::Scope::Files(files)
                    if files.iter().all(|path| {
                        path.extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
                    }) =>
                {
                    Some(files.len())
                }
                _ => None,
            };
            op.kind = match JobKind::of(merged) {
                JobKind::Rebuild => "rebuild",
                JobKind::Check => "check",
                JobKind::Files => "files",
            }
            .into();
            op.source = match merged.priority {
                Priority::Explicit => "explicit",
                Priority::Edit => "edit",
                Priority::Watch => "watch",
                Priority::Periodic => "periodic",
            }
            .into();
        });
        self.wake.notify_one();
        Some((id, was_merged, JobKind::of(merged)))
    }

    /// Проверяет папку и запускает проверку; слежение — только при активности проекта.
    /// Папки нет — проект `unavailable`; регистрация сразу ставит начальное обновление.
    /// Ленивый найденный подпроект на старте и в часовой проверке остаётся ленивым; любое другое
    /// обращение — первое: `lazy` снимается и проект идёт обычным путём (Т-49).
    fn activate(self: &Arc<Self>, path: &Path, priority: Priority) {
        if priority != Priority::Periodic {
            self.wake_lazy(path);
        }
        let registry = lock(&self.registry);
        let Some(entry) = registry.find(path) else {
            return;
        };
        if lock(&self.projects)
            .get(path)
            .is_some_and(|runtime| runtime.deleting)
        {
            return;
        }
        if entry.lazy {
            // Ни проверки, ни слежения, ни задания: ПК не нагружается (НФТ-7).
            lock(&self.projects)
                .entry(path.to_path_buf())
                .or_insert_with(Runtime::new_lazy);
            return;
        }
        if !path.is_dir() {
            drop(registry);
            self.mark_unavailable(path);
            return;
        }
        {
            let mut projects = lock(&self.projects);
            let runtime = projects
                .entry(path.to_path_buf())
                .or_insert_with(Runtime::new);
            if matches!(
                runtime.status.state,
                ProjectState::Unavailable | ProjectState::Lazy
            ) {
                runtime.status.state = ProjectState::Ready;
            }
        }
        drop(registry);
        self.ensure_watching(path);
        if priority == Priority::Explicit {
            self.enqueue(path, Job::check(priority));
            return;
        }
        let worker = Worker {
            inner: self.clone(),
        };
        if let Err(error) = worker.scan_with_priority(path, priority)
            && !matches!(error, WorkerError::ConsentRequired(_))
            && let Some(runtime) = lock(&self.projects).get_mut(path)
        {
            runtime.status.last_error = Some(error.to_string());
        }
    }

    /// Пересчитывает области вложенных проектов по реестру и `docs_root` каждого проекта и ставит
    /// проверку проектам, у которых список исключений изменился: ушедшие в область вложенного
    /// файлы удаляются из индекса по путям, вернувшиеся добавляются (Т-7, Т-22). Пропавший
    /// проект остаётся исключённым, пока запись в реестре есть.
    fn refresh_nested(&self) {
        let paths = lock(&self.registry).paths();
        let projects: Vec<(PathBuf, String)> = paths
            .into_iter()
            .map(|path| {
                let docs_root = self.docs_root_of(&path);
                (path, docs_root)
            })
            .collect();
        for project in bars_core::indexing::nested::publish(&projects) {
            // Ленивый проект не индексируется: новые исключения учтёт его первая проверка.
            if self.is_lazy(&project) {
                continue;
            }
            self.enqueue(&project, Job::check(Priority::Explicit));
        }
    }

    fn check_all(self: &Arc<Self>) {
        let paths = lock(&self.registry).paths();
        for path in paths {
            self.activate(&path, Priority::Periodic);
        }
    }

    /// Запоминает, есть ли у проекта папка корня документации; возвращает её наличие. Запись в
    /// журнал — один раз при исчезновении папки, возврат папки сбрасывает пометку.
    fn note_docs_root(&self, path: &Path, present: bool) -> bool {
        let first_missing = {
            let mut projects = lock(&self.projects);
            let Some(runtime) = projects.get_mut(path) else {
                return present;
            };
            let first_missing = !present && !runtime.docs_root_missing;
            runtime.docs_root_missing = !present;
            first_missing
        };
        if first_missing {
            log::warn!(
                "Нет папки корня документации, слежение не запускается, пока она не появится: {}",
                path.display()
            );
        }
        present
    }

    /// Поднимает слежение, если его нет или оно сломалось.
    fn ensure_watching(self: &Arc<Self>, path: &Path) {
        let generation = lock(&self.projects)
            .get(path)
            .filter(|runtime| {
                !runtime.deleting
                    && runtime.observed(Instant::now())
                    && (runtime.watcher.is_none() || runtime.watch_failed)
            })
            .map(|runtime| Arc::clone(&runtime.job_gate));
        let Some(generation) = generation else {
            return;
        };
        // Без папки корня документации запускать нечего: одна проверка существования, журнал
        // пишется только при переходе в это состояние (ISS-106).
        let root_present = path.join(self.docs_root_of(path)).is_dir();
        if !self.note_docs_root(path, root_present) {
            return;
        }
        let _publication = self.updates.after(path, updates::STATUS);
        let weak: Weak<Inner> = Arc::downgrade(self);
        let key = path.to_path_buf();
        let sink: Sink = Arc::new(move |event| {
            if let Some(inner) = weak.upgrade() {
                inner.on_watch_event(&key, event);
            }
        });
        let mut started =
            ProjectWatcher::start(path, &self.docs_root_of(path), self.config.debounce, sink).ok();
        if started.is_none() {
            log::warn!(
                "Не удалось запустить слежение за документацией: {}",
                path.display()
            );
        }
        let replaced = {
            let mut projects = lock(&self.projects);
            projects
                .get_mut(path)
                .filter(|runtime| {
                    !runtime.deleting
                        && runtime.observed(Instant::now())
                        && Arc::ptr_eq(&runtime.job_gate, &generation)
                })
                .and_then(|runtime| {
                    runtime.watch_failed = started.is_none();
                    std::mem::replace(&mut runtime.watcher, started.take())
                })
        };
        // Прежнее слежение останавливается вне блокировки: его поток может ждать её.
        drop(replaced);
        drop(started);
    }

    /// Вызывается в потоке `notify`: останавливать слежение отсюда нельзя.
    fn on_watch_event(self: &Arc<Self>, project: &Path, event: WatchEvent) {
        if !lock(&self.projects)
            .get(project)
            .is_some_and(|runtime| !runtime.deleting && runtime.observed(Instant::now()))
        {
            return;
        }
        if self.stop.load(Ordering::SeqCst) {
            return;
        }
        if matches!(event, WatchEvent::Tree) {
            self.updates.changed(project, updates::DISK, false);
            return;
        }
        let _publication = self.updates.after(project, updates::STATUS);
        if matches!(event, WatchEvent::Failed(_) | WatchEvent::Rescan) {
            self.updates.changed(project, updates::ALL, true);
        }
        if !project.is_dir() {
            if let Some(watcher) = self.mark_unavailable(project) {
                std::thread::spawn(move || drop(watcher));
            }
            return;
        }
        if let Some(runtime) = lock(&self.projects).get_mut(project) {
            runtime.watch_events = runtime.watch_events.saturating_add(1);
        }
        match event {
            WatchEvent::Tree => unreachable!(),
            WatchEvent::Files(files) => {
                // Событие внутри области вложенного проекта не ставит задание внешнему.
                let excluded = bars_core::indexing::nested::excluded(project);
                let files: Vec<PathBuf> = files
                    .into_iter()
                    .filter(|file| {
                        !file.strip_prefix(project).is_ok_and(|relative| {
                            let relative = relative.to_string_lossy().replace('\\', "/");
                            bars_core::indexing::nested::is_excluded(&relative, &excluded)
                        })
                    })
                    .collect();
                if files.is_empty() {
                    return;
                }
                self.touch_files(project, files, DocSource::Watch)
            }
            WatchEvent::Failed(message) => {
                if let Some(runtime) = lock(&self.projects).get_mut(project) {
                    runtime.watch_failed = true;
                    runtime.status.last_error = Some(format!("слежение за файлами: {message}"));
                }
                // События могли потеряться: проверка целиком.
                let worker = Worker {
                    inner: self.clone(),
                };
                let _ = worker.scan_with_priority(project, Priority::Watch);
            }
            WatchEvent::Rescan => {
                let worker = Worker {
                    inner: self.clone(),
                };
                let _ = worker.scan_with_priority(project, Priority::Watch);
            }
        }
        // Хвост пачки мог потеряться: проверка целиком через несколько секунд.
        self.arm_tail(project);
    }

    /// Пауза после правки до embed документа проекта (Т-41): значение конфигурации, иначе
    /// `[indexing] edit_pause_seconds`; ошибка настроек не мешает — берётся умолчание.
    fn edit_pause(&self, project: &Path) -> Duration {
        self.config.edit_pause.unwrap_or_else(|| {
            let seconds = Settings::load(&self.config.bars_home, project)
                .map(|loaded| loaded.value.indexing.edit_pause_seconds)
                .unwrap_or(bars_core::settings::DEFAULT_EDIT_PAUSE_SECONDS);
            Duration::from_secs(u64::from(seconds))
        })
    }

    /// Файлы изменились в окне или на диске. Существующие `.md` ждут паузы и идут в очередь
    /// документов (Т-41, Т-42); пропавшие файлы, папки и проект, ждущий переиндексации, ставятся
    /// заданием сразу: удалению пауза не нужна, а в `reindex_required` записи не создаются.
    fn touch_files(&self, project: &Path, files: Vec<PathBuf>, source: DocSource) {
        let _publication = self.updates.after(project, updates::DISK | updates::STATUS);
        let reindex = lock(&self.projects)
            .get(project)
            .is_some_and(|runtime| runtime.status.state == ProjectState::ReindexRequired);
        let mut direct = Vec::new();
        let mut keys = Vec::new();
        for file in files {
            match doc_key(project, &file) {
                Some(key) if !reindex && file.is_file() => keys.push(key),
                _ => direct.push(file),
            }
        }
        if !keys.is_empty() {
            let pause = self.edit_pause(project);
            let now = Instant::now();
            let ready = {
                let mut docs = lock(&self.docs);
                for key in &keys {
                    docs.touch(project, key, source, pause, now);
                }
                docs.ready_of(project)
            };
            if let Some(ready) = ready {
                self.submit_docs(project, ready.paths, ready.source);
            }
            self.doc_wake.notify_one();
        }
        if !direct.is_empty() {
            self.enqueue(project, Job::files(source.priority(), direct));
        }
    }

    /// Отдаёт готовые документы заданию проекта: одна пачка на проект.
    fn submit_docs(
        &self,
        project: &Path,
        paths: std::collections::BTreeSet<String>,
        source: DocSource,
    ) {
        let files = paths.into_iter().map(|path| project.join(path));
        self.enqueue(project, Job::files(source.priority(), files));
    }

    /// Ждёт ближайшую паузу и отдаёт документы, у которых она истекла.
    async fn run_doc_timer(self: Arc<Self>) {
        loop {
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            let next = lock(&self.docs).next_due();
            match next {
                Some(due) => {
                    let due = tokio::time::Instant::from_std(due);
                    tokio::select! {
                        () = tokio::time::sleep_until(due) => {}
                        () = self.doc_wake.notified() => continue,
                    }
                }
                None => {
                    self.doc_wake.notified().await;
                    continue;
                }
            }
            let ready = lock(&self.docs).promote_due(Instant::now());
            for item in ready {
                match &item.target {
                    DocTarget::Project(project) => {
                        self.submit_docs(project, item.paths, item.source);
                    }
                    DocTarget::Worktree { owner, name } => self.submit_worktree_docs(owner, name),
                }
            }
        }
    }

    /// Итог задания для записей очереди документов. Успех снимает записи; документ с ошибкой
    /// эмбеддинга остаётся с причиной; сбой задания возвращает остальные в очередь; остановка,
    /// недоступная папка и ожидание переиндексации снимают все записи проекта.
    fn finish_docs(&self, project: &Path, taken: &[String], outcome: &Outcome) {
        use bars_core::indexing::indexer::UpdateReport;
        let failures = |report: &UpdateReport| -> std::collections::BTreeMap<String, String> {
            report
                .problems
                .iter()
                .filter(|problem| problem.stage == "embedding")
                .map(|problem| (problem.path.clone(), problem.reason.clone()))
                .collect()
        };
        let mut docs = lock(&self.docs);
        match outcome {
            Outcome::Updated(report) => docs.finish(project, taken, &failures(report), false),
            Outcome::Interrupted {
                report,
                cancelled: false,
                ..
            } => docs.finish(project, taken, &failures(report), true),
            Outcome::Failed(_) => docs.finish(project, taken, &Default::default(), true),
            Outcome::Interrupted { .. }
            | Outcome::Cancelled
            | Outcome::Unavailable
            | Outcome::ReindexRequired => {
                docs.remove_project(project);
            }
        }
        drop(docs);
        self.doc_wake.notify_one();
    }

    /// Назначает полную проверку проекта через `tail_delay` (ISS-06): на Windows `notify` теряет
    /// события при переполнении буфера, а о потере не сообщает. Пока проверка назначена, новых
    /// не ставится. Заодно поднимает слежение, если оно сломалось: отсюда, не из потока `notify`.
    fn arm_tail(self: &Arc<Self>, project: &Path) {
        let generation = {
            let mut projects = lock(&self.projects);
            let Some(runtime) = projects.get_mut(project) else {
                return;
            };
            if runtime.tail_armed || runtime.deleting || !runtime.observed(Instant::now()) {
                return;
            }
            runtime.tail_armed = true;
            Arc::clone(&runtime.job_gate)
        };
        let weak: Weak<Inner> = Arc::downgrade(self);
        let project = project.to_path_buf();
        let delay = self.config.tail_delay;
        self.handle.spawn(async move {
            tokio::time::sleep(delay).await;
            let Some(inner) = weak.upgrade() else {
                return;
            };
            if inner.stop.load(Ordering::SeqCst) {
                return;
            }
            if let Some(runtime) = lock(&inner.projects).get_mut(&project).filter(|r| {
                !r.deleting && r.observed(Instant::now()) && Arc::ptr_eq(&r.job_gate, &generation)
            }) {
                runtime.tail_armed = false;
            } else {
                return;
            }
            if !project.is_dir() {
                return;
            }
            let restart = Arc::clone(&inner);
            let path = project.clone();
            let _ = tokio::task::spawn_blocking(move || restart.ensure_watching(&path)).await;
            let worker = Worker { inner };
            let _ = worker.scan_with_priority(&project, Priority::Watch);
        });
    }

    /// Итог полной проверки: нашла изменения, а слежение молчало. Значит, события потерялись:
    /// проверяем ещё раз, пока не будет тихо. Если это была часовая проверка, слежение,
    /// скорее всего, умерло молча, и его перезапускают.
    fn after_full_check(self: &Arc<Self>, path: &Path, periodic: bool) {
        let silent = {
            let mut projects = lock(&self.projects);
            let Some(runtime) = projects.get_mut(path) else {
                return;
            };
            // Ненаблюдаемому проекту слежение не нужно: тревога только у наблюдаемого (ISS-106).
            let silent = runtime.watch_events == 0 && runtime.observed(Instant::now());
            if silent && periodic && runtime.watcher.is_some() {
                runtime.watch_failed = true;
            }
            silent
        };
        if !silent {
            return;
        }
        if periodic {
            log::warn!(
                "bars: проверка нашла изменения в {}, а слежение молчало: перезапускаю слежение",
                path.display()
            );
            self.ensure_watching(path);
        }
        self.arm_tail(path);
    }

    /// Папки проекта нет: проект `unavailable`, с очереди он снимается. Возвращает слежение,
    /// которое надо остановить вне блокировок.
    fn mark_unavailable(&self, path: &Path) -> Option<ProjectWatcher> {
        let _publication = self.updates.after(path, updates::ALL);
        let watcher = {
            let mut projects = lock(&self.projects);
            let runtime = projects
                .entry(path.to_path_buf())
                .or_insert_with(Runtime::new);
            runtime.status.state = ProjectState::Unavailable;
            runtime.watch_failed = true;
            runtime.watcher.take()
        };
        self.cancel_queued(path);
        watcher
    }

    async fn run_queue(self: Arc<Self>) {
        loop {
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            if self.pause.is_paused() {
                // Пауза замораживает очередь целиком; `resume` и `shutdown` будят цикл.
                self.wake.notified().await;
                continue;
            }
            if self.delta_busy.load(Ordering::SeqCst) {
                // Сверка дельты worktree идёт: индексация по одному заданию, проекты ждут её конца.
                tokio::select! {
                    () = self.wake.notified() => {},
                    () = tokio::time::sleep(Duration::from_millis(50)) => {},
                }
                continue;
            }
            let next = {
                let projects = lock(&self.projects);
                let mut queue = lock(&self.queue);
                let next = queue.pop_available(|path| {
                    let runtime = projects.get(path).filter(|runtime| !runtime.deleting)?;
                    let job_gate = Arc::clone(&runtime.job_gate);
                    let guard = Arc::clone(&job_gate).try_lock_owned().ok()?;
                    Some((job_gate, guard))
                });
                if next.is_some() {
                    self.busy.store(true, Ordering::SeqCst);
                }
                next
            };
            match next {
                Some((path, job, (job_gate, guard))) => {
                    self.process(path, job, job_gate, guard).await;
                    self.busy.store(false, Ordering::SeqCst);
                    // Очередь проектов свободна: дельты worktree могут идти.
                    self.delta_wake.notify_one();
                }
                None => {
                    // Чужой держатель gate может не сигналить очередь; проверяем только готовность.
                    let blocked = !lock(&self.queue).is_empty();
                    if blocked {
                        tokio::select! {
                            () = self.wake.notified() => {},
                            () = tokio::time::sleep(Duration::from_millis(50)) => {},
                        }
                    } else {
                        self.wake.notified().await;
                    }
                }
            }
        }
    }

    async fn run_timer(self: Arc<Self>) {
        let period = self.config.periodic_interval;
        let mut ticker = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
        loop {
            ticker.tick().await;
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            // Сначала автопоиск подпроектов, затем проверка файлов (ленивые в ней пропускаются).
            let inner = Arc::clone(&self);
            let _ = tokio::task::spawn_blocking(move || {
                inner.discover_all();
                // Затем worktree всех владельцев, ленивых тоже; пропавшие уходят из списка.
                inner.discover_worktrees(None);
                // Дельты: уборка пропавших и несовместимых, актуальность живых.
                inner.check_deltas();
            })
            .await;
            self.check_all();
        }
    }

    async fn process(
        self: &Arc<Self>,
        path: PathBuf,
        job: Job,
        job_gate: Arc<tokio::sync::Mutex<()>>,
        _project_lease: tokio::sync::OwnedMutexGuard<()>,
    ) {
        let operation_id = job.operation_id;
        let _pending = operations::PendingResult(Arc::clone(&self.models.operations), operation_id);
        let rebuild = job.rebuild.is_some();
        let full_check = job.rebuild.is_none() && job.reconciled;
        let periodic = job.priority == Priority::Periodic;
        let cell = JobCell::new(&path, &job);
        let (gate, previous) = {
            let mut projects = lock(&self.projects);
            let Some(runtime) = projects.get_mut(&path) else {
                return;
            };
            if runtime.deleting || !Arc::ptr_eq(&runtime.job_gate, &job_gate) {
                return;
            }
            let previous = runtime.status.state;
            if previous == ProjectState::Unavailable {
                return;
            }
            if previous != ProjectState::ReindexRequired {
                runtime.status.state = ProjectState::Indexing;
            }
            if full_check {
                runtime.watch_events = 0;
            }
            *lock(&self.current) = Some(Arc::clone(&cell));
            (Arc::clone(&runtime.gate), previous)
        };
        self.updates.changed(&path, updates::STATUS, false);

        // Документы очереди, которые задание разберёт, и те, что остаются ждать паузы (Т-41).
        let taken = {
            let listed = match &job.scope {
                queue::Scope::Full => None,
                queue::Scope::Files(files) => Some(
                    files
                        .iter()
                        .filter_map(|file| doc_key(&path, file))
                        .collect::<std::collections::BTreeSet<_>>(),
                ),
            };
            let explicit = if rebuild {
                Some(None)
            } else if job.priority == Priority::Explicit {
                Some(listed.as_ref())
            } else {
                None
            };
            lock(&self.docs).begin(&path, explicit, listed.as_ref())
        };
        let worker = Arc::clone(self);
        let project = path.clone();
        lock(&self.models.operations).edit(operation_id, |op| op.state = "running".into());
        let taken = Arc::new(Mutex::new(taken.into_iter().collect()));
        // Документы, которые задание запишет в индекс или удалит: их увидят дельты worktree.
        let written = Arc::new(Mutex::new(std::collections::BTreeSet::new()));
        let source = match job.priority {
            Priority::Explicit => DocSource::Explicit,
            Priority::Edit => DocSource::Edit,
            Priority::Watch => DocSource::Watch,
            Priority::Periodic => DocSource::Periodic,
        };
        let mut observer = JobObserver::new(
            Arc::clone(&self.pause),
            Arc::clone(&cell),
            Arc::clone(&self.stop),
        )
        .with_documents(Arc::new(doc_queue::Observer {
            updates: self.updates.clone(),
            docs: Arc::clone(&self.docs),
            project: path.clone(),
            source,
            taken: Arc::clone(&taken),
            written: Arc::clone(&written),
            wake: Arc::clone(&self.doc_wake),
            rebuild,
        }));
        if !rebuild {
            let docs = Arc::clone(&self.docs);
            let owner = path.clone();
            observer = observer.with_defer(Arc::new(move |document: &str, full_check: bool| {
                let docs = lock(&docs);
                (!full_check && docs.is_waiting(&owner, document))
                    || docs.is_error(&owner, document)
            }));
        }
        log::info!("Начата индексация: {}", path.display());
        let result = tokio::task::spawn_blocking(move || {
            let env = JobEnv {
                bars_home: &worker.config.bars_home,
                models: &worker.models,
                gate,
                handle: worker.handle.clone(),
                observer: &observer,
            };
            job::run(&env, &project, &job)
        })
        .await;

        let result = result.unwrap_or_else(|error| job::JobResult {
            outcome: Outcome::Failed(format!("внутренняя ошибка воркера: {error}")),
            index_model: None,
            settings_model: None,
        });
        let found_changes = full_check
            && matches!(
                &result.outcome,
                Outcome::Updated(report)
                    if report.added + report.removed > 0
                        || (!report.full_check && report.changed > 0)
            );
        let report = match &result.outcome {
            Outcome::Updated(report) | Outcome::Interrupted { report, .. } => Some(report),
            _ => None,
        };
        if let Some(report) = report {
            for problem in &report.problems {
                // reason может содержать ответ внешнего провайдера. Пишем только метаданные.
                log::warn!(
                    "Проблема индексации: проект {}, файл {}, этап {}, попыток {}, прежняя версия {}",
                    path.display(),
                    problem.path,
                    problem.stage,
                    problem.attempts,
                    problem.previous_version
                );
            }
        }
        match &result.outcome {
            Outcome::Failed(_) => log::error!(
                "Индексация завершилась ошибкой: {}; подробности — в отчёте задания",
                path.display()
            ),
            Outcome::Interrupted { .. } => log::warn!(
                "Индексация прервана: {}; подробности — в отчёте задания",
                path.display()
            ),
            Outcome::Updated(report) if !report.problems.is_empty() => log::warn!(
                "Индексация завершена с проблемами: {}; файлов с ошибками: {}",
                path.display(),
                report.problems.len()
            ),
            Outcome::Updated(report) => log::info!(
                "Индексация завершена: {}; добавлено {}, изменено {}, удалено {}",
                path.display(),
                report.added,
                report.changed,
                report.removed
            ),
            Outcome::Unavailable => log::warn!("Папка проекта недоступна: {}", path.display()),
            Outcome::Cancelled => log::info!("Индексация отменена: {}", path.display()),
            Outcome::ReindexRequired => log::info!("Нужна переиндексация: {}", path.display()),
        }
        let unfinished: Vec<_> = lock(&taken).iter().cloned().collect();
        self.finish_docs(&path, &unfinished, &result.outcome);
        let outcome = result.outcome.clone();
        // Готовность текста публикуется уже с зарегистрированным заданием графа.
        if matches!(outcome, Outcome::Updated(_) | Outcome::Interrupted { .. }) {
            let worker = Worker {
                inner: self.clone(),
            };
            // Индексация без добавленных, изменённых и удалённых документов граф не трогает.
            let changed = match &outcome {
                Outcome::Updated(report) => report.added + report.changed + report.removed > 0,
                _ => true,
            };
            worker.graph_signal(&path, changed).await;
        }
        // Публикация итога только после применения состояния проекта.
        self.apply(&path, previous, &job_gate, result);
        // Индекс владельца изменился: живые дельты его worktree сверяют затронутые пути. После
        // полной переиндексации или смены нарезки — весь вид; прерванная переиндексация индекс
        // не подменила.
        let changed_paths = match &outcome {
            Outcome::Updated(report) if rebuild || report.full_check => Some(None),
            Outcome::Updated(_) | Outcome::Interrupted { .. } if !rebuild => {
                let written = std::mem::take(&mut *lock(&written));
                (!written.is_empty()).then_some(Some(written))
            }
            _ => None,
        };
        if let Some(paths) = changed_paths {
            self.owner_changed(&path, paths);
        }
        {
            let mut journal = lock(&self.models.operations);
            journal.edit(operation_id, |op| {
                let p = cell.progress();
                op.docs_done = Some(p.docs_done);
                op.docs_total = cell.total_known().then_some(p.docs_total);
            });
            journal.index_result(operation_id, &outcome, rebuild);
        }
        *lock(&self.current) = None;
        if found_changes {
            self.after_full_check(&path, periodic);
        }
    }

    fn apply(
        &self,
        path: &Path,
        previous: ProjectState,
        generation_gate: &Arc<tokio::sync::Mutex<()>>,
        result: job::JobResult,
    ) {
        let _publication = self.updates.after(
            path,
            updates::STATUS
                | if matches!(&result.outcome, Outcome::Updated(_)) {
                    updates::INDEX
                } else {
                    0
                },
        );
        let mut stopped = None;
        {
            let mut projects = lock(&self.projects);
            let Some(runtime) = projects.get_mut(path) else {
                return;
            };
            if !Arc::ptr_eq(&runtime.job_gate, generation_gate) {
                return;
            }
            let status = &mut runtime.status;
            if let Some(model) = result.index_model {
                status.model_index = Some(model);
            }
            if let Some(model) = result.settings_model {
                status.model_settings = Some(model);
            }
            match result.outcome {
                Outcome::Interrupted {
                    report,
                    message,
                    cancelled,
                } => {
                    status.state = if previous == ProjectState::ReindexRequired {
                        ProjectState::ReindexRequired
                    } else {
                        ProjectState::Ready
                    };
                    status.last_report = Some(report);
                    if !cancelled {
                        status.last_error = Some(message);
                    }
                }
                Outcome::Unavailable => {
                    status.state = ProjectState::Unavailable;
                    runtime.watch_failed = true;
                    stopped = runtime.watcher.take();
                }
                _ if status.state == ProjectState::Unavailable => {}
                Outcome::Updated(report) => {
                    status.state = ProjectState::Ready;
                    status.updated_at = Some(SystemTime::now());
                    status.last_error = None;
                    status.last_report = Some(report);
                }
                Outcome::ReindexRequired => {
                    status.state = ProjectState::ReindexRequired;
                    status.last_error = None;
                }
                Outcome::Cancelled => {
                    // Остановка оставляет проект как был: старый индекс цел (OQ-55).
                    status.state = if previous == ProjectState::ReindexRequired {
                        ProjectState::ReindexRequired
                    } else {
                        ProjectState::Ready
                    };
                }
                Outcome::Failed(message) => {
                    status.state = if previous == ProjectState::ReindexRequired {
                        ProjectState::ReindexRequired
                    } else {
                        ProjectState::Ready
                    };
                    status.last_error = Some(message);
                }
            }
        }
        drop(stopped);
    }
}

/// Недописанная подмена прошлого запуска оставляет `index.sqlite.new` или `index.sqlite.old`;
/// правила — `docs/architecture/worker.md`, «Восстановление файлов индекса при старте».
fn recover_project_index(project: &Path) {
    if !project.is_dir() {
        return;
    }
    if let Err(error) =
        bars_core::indexing::indexer::recover_index_files(&bars_core::storage::index_path(project))
    {
        log::error!(
            "Не удалось восстановить файлы индекса {}: {error}",
            project.display()
        );
    }
}
