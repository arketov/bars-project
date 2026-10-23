//! Воркер без окна: реестр проектов, очередь индексации, слежение за файлами и часовая проверка
//! (`docs/architecture/worker.md`, `docs/architecture/projects.md`, Т-5, Т-10, Т-15, Т-25).
//!
//! Обработчики протокола и CLI берут отсюда небольшой API [`Worker`]: регистрация, снятие,
//! выбор проекта, `status`, `reindex`, `session_start`, доступ к проекту для поиска и модели.
//! Поиск в очереди не стоит: он берёт [`Worker::gate`] на чтение и открывает индекс сам.

mod control;
mod deletion;
mod doc_queue;
mod graph;
mod job;
mod model_lifecycle;
pub mod models;
mod operations;
mod queue;
mod registry;
mod scan;
mod summary;
mod timefmt;
mod watcher;

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
pub use registry::{Discovered, ProjectEntry, Registry, RegistryError, ResolveError};
pub use summary::{Health, Line, Summary};
pub use timefmt::{index_token, rfc3339};

use control::{JobCell, JobObserver, Pause};
pub use control::{Kind as JobKind, Progress as JobProgress};
use doc_queue::DocQueue;
pub use doc_queue::{DocCounts, DocSource, DocState, DocView};
use job::{JobEnv, Outcome};
use queue::{Job, JobQueue};
use registry::{has_markdown, normalize, project_name};
use watcher::{DEBOUNCE, ProjectWatcher, Sink, WatchEvent};

/// Как часто воркер проверяет все проекты (Т-10). Слежение за файлами может молчать, поэтому
/// проверка не должна быть редкой: на неизменном проекте она дешёвая, эмбеддинг не запускается.
pub const PERIODIC_INTERVAL: Duration = Duration::from_secs(15 * 60);

/// Через сколько после пачки событий проект проверяется целиком: ловит хвост, который
/// `notify` потерял при переполнении буфера (ISS-06).
pub const TAIL_DELAY: Duration = Duration::from_secs(5);

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
            install_env: None,
            secrets: Arc::new(OsSecrets::default()),
            autostart: Arc::new(NoAutostart),
        }
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
    /// `notify` сообщил об ошибке или папка пропадала: слежение надо поднять заново.
    watch_failed: bool,
    /// Сколько сообщений слежения пришло с начала последней полной проверки. Проверка нашла
    /// изменения, а сообщений не было — слежение молчит, его надо перезапустить.
    watch_events: u32,
    /// Проверка «хвоста» уже назначена, новую не ставим.
    tail_armed: bool,
    gate: Arc<RwLock<()>>,
}

impl Runtime {
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
            watch_failed: false,
            watch_events: 0,
            tail_armed: false,
            gate: Arc::new(RwLock::new(())),
        }
    }
}

struct Inner {
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
    stop: AtomicBool,
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
    download_errors: Mutex<HashMap<String, String>>,
    graph_jobs: Mutex<HashMap<PathBuf, graph::GraphJob>>,
    scans: Mutex<HashMap<PathBuf, scan::ScanJob>>,
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
            requests: Arc::new(RwLock::new(())),
            config,
            registry: Mutex::new(registry),
            projects: Mutex::new(HashMap::new()),
            queue: Mutex::new(JobQueue::new()),
            docs: Arc::new(Mutex::new(DocQueue::new())),
            doc_wake: Arc::new(Notify::new()),
            busy: AtomicBool::new(false),
            wake: Notify::new(),
            stop: AtomicBool::new(false),
            models: Models::new(provider),
            handle: Handle::current(),
            tasks: Mutex::new(Vec::new()),
            model_wake: Notify::new(),
            pause: Arc::new(Pause::default()),
            current: Mutex::new(None),
            downloads: Mutex::new(HashSet::new()),
            download_errors: Mutex::new(HashMap::new()),
            graph_jobs: Mutex::new(HashMap::new()),
            scans: Mutex::new(HashMap::new()),
        });
        let queue_task = tokio::spawn(Arc::clone(&inner).run_queue());
        let timer_task = tokio::spawn(Arc::clone(&inner).run_timer());
        let doc_task = tokio::spawn(Arc::clone(&inner).run_doc_timer());
        let model_task = tokio::spawn(Arc::clone(&inner).run_model_maintenance());
        lock(&inner.tasks).extend([queue_task, timer_task, doc_task, model_task]);
        for path in paths {
            inner.activate(&path, Priority::Periodic);
        }
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

    /// Перестаёт брать задания, дожидается текущего, снимает слежение.
    pub async fn shutdown(&self) {
        self.inner.stop.store(true, Ordering::SeqCst);
        self.inner.model_wake.notify_one();
        // Задание на паузе иначе не закончится никогда: его останавливают, остальные дописываются.
        if self.inner.pause.is_paused() {
            self.inner.cancel_current();
            self.inner.pause.set(false);
        }
        self.inner.wake.notify_one();
        let tasks: Vec<_> = lock(&self.inner.tasks).drain(..).collect();
        // Первая — очередь: она дописывает текущее задание сама. Вторая — таймер, он ждёт часами;
        // третья — таймер пауз документов, он тоже не нужен после остановки.
        for (position, task) in tasks.into_iter().enumerate() {
            if position == 1 || position == 2 {
                task.abort();
            }
            let _ = task.await;
        }
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
    /// ставит на проверку ещё раз.
    pub fn register(&self, path: &Path) -> Result<Registered, WorkerError> {
        let _request =
            self.inner.requests.try_read().map_err(|_| {
                io::Error::other("идёт удаление проекта; повторите регистрацию позже")
            })?;
        let path = normalize(path)?;
        if !path.is_dir() {
            return Err(WorkerError::NotADirectory(path));
        }
        ensure_project_config(&path)?;
        let (canonical, added) = {
            let mut registry = lock(&self.inner.registry);
            let added = registry.add(&path)?;
            let canonical = registry
                .find(&path)
                .map_or_else(|| path.clone(), |entry| entry.path.clone());
            (canonical, added)
        };
        self.inner.activate(&canonical, Priority::Explicit);
        Ok(Registered {
            path: canonical,
            already: !added,
        })
    }

    /// Снимает проект с регистрации: убирает из `projects.toml`, останавливает слежение, снимает
    /// с очереди. `.bars/` и документы проекта остаются (Т-5).
    pub fn unregister(&self, selector: &str) -> Result<PathBuf, WorkerError> {
        let _request = self.inner.requests.try_read().map_err(|_| {
            io::Error::other("идёт удаление проекта; повторите снятие регистрации позже")
        })?;
        let path = self.resolve(selector)?;
        lock(&self.inner.registry).remove(&path)?;
        let runtime = lock(&self.inner.projects).remove(&path);
        self.inner.cancel_queued(&path);
        self.inner.cancel_scan(&path);
        // Слежение останавливается вне блокировок: поток `notify` может ждать их.
        drop(runtime);
        Ok(path)
    }

    /// Проект по пути или по имени папки; одинаковое имя у нескольких проектов — ошибка.
    pub fn resolve(&self, selector: &str) -> Result<PathBuf, WorkerError> {
        Ok(lock(&self.inner.registry).resolve(selector)?.path.clone())
    }

    /// Ближайший зарегистрированный проект от `cwd` вверх по дереву (Т-15).
    pub fn project_for_cwd(&self, cwd: &Path) -> Option<PathBuf> {
        lock(&self.inner.registry)
            .project_for_cwd(cwd)
            .map(|entry| entry.path.clone())
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

    /// Все проекты реестра с состоянием, в порядке регистрации.
    pub fn projects(&self) -> Vec<ProjectSnapshot> {
        let entries: Vec<(ProjectEntry, bool)> = {
            let registry = lock(&self.inner.registry);
            registry
                .entries()
                .iter()
                .map(|entry| (entry.clone(), registry.has_duplicate_name(&entry.path)))
                .collect()
        };
        entries
            .into_iter()
            .map(|(entry, duplicate_name)| {
                let state = lock(&self.inner.projects).get(&entry.path).map_or(
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
                );
                ProjectSnapshot {
                    name: project_name(&entry.path),
                    duplicate_name,
                    docs_root: self.inner.docs_root_of(&entry.path),
                    registered_at: entry.registered_at,
                    state,
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
        let discovered = lock(&self.inner.registry).discover_root(&cwd);
        if discovered.registered {
            self.inner.activate(&discovered.root, Priority::Explicit);
            self.sync_project_gates(Some(&discovered.root));
            return Ok(Some(discovered.root));
        }
        if !discovered.root.is_dir()
            || !has_markdown(&discovered.root, &self.inner.docs_root_of(&discovered.root))
        {
            return Ok(None);
        }
        let root = self.register(&discovered.root)?.path;
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
        let state = lock(&self.inner.projects)
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
        let projects = lock(&self.inner.projects);
        self.inner
            .enqueue_receipt(&path, job, &projects)
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

    pub fn download_errors(&self) -> HashMap<String, String> {
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
            let _download = DownloadCleanup { inner: Arc::clone(&inner), id: id.clone() };
            log::info!("Начато скачивание модели {id}");
            let success = inner.models.download_operation(&settings, reranker, Some(operation)).is_ok();
            if !success {
                log::error!("Не удалось скачать модель {id}; подробности — в состоянии модели");
                // В UI не передаём произвольный ответ провайдера, URL или токены.
                lock(&inner.download_errors).insert(
                    id.clone(),
                    "Не удалось скачать модель. Проверьте подключение и повторите в настройках моделей.".into(),
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
        let projects = lock(&self.projects);
        self.enqueue_for(project, job, &projects);
    }

    fn enqueue_for(&self, project: &Path, job: Job, projects: &HashMap<PathBuf, Runtime>) {
        self.enqueue_receipt(project, job, projects);
    }

    fn enqueue_receipt(
        &self,
        project: &Path,
        mut job: Job,
        projects: &HashMap<PathBuf, Runtime>,
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
                let settings = Settings::load(&self.config.bars_home, project)
                    .ok()
                    .map(|s| s.value);
                journal.begin(
                    "check",
                    "explicit",
                    Some(project.display().to_string()),
                    projects[project].generation,
                    None,
                    None,
                    false,
                    settings.as_ref().map(operations::settings_key).unwrap_or(0),
                )
            });
        let id = job.operation_id;
        queue.push(project, job);
        let merged = queue.get(project).expect("задание поставлено");
        journal.edit(id, |op| {
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

    /// Вводит проект в работу: папка на месте, слежение запущено, проверка в очереди. Папки нет —
    /// проект `unavailable`.
    fn activate(self: &Arc<Self>, path: &Path, priority: Priority) {
        let registry = lock(&self.registry);
        if !registry.contains(path) {
            return;
        }
        if lock(&self.projects)
            .get(path)
            .is_some_and(|runtime| runtime.deleting)
        {
            return;
        }
        if !path.is_dir() {
            self.mark_unavailable(path);
            return;
        }
        remove_stale_rebuild(path);
        {
            let mut projects = lock(&self.projects);
            let runtime = projects
                .entry(path.to_path_buf())
                .or_insert_with(Runtime::new);
            if runtime.status.state == ProjectState::Unavailable {
                runtime.status.state = ProjectState::Ready;
            }
        }
        drop(registry);
        self.ensure_watching(path);
        self.enqueue(path, Job::check(priority));
    }

    fn check_all(self: &Arc<Self>) {
        let paths = lock(&self.registry).paths();
        for path in paths {
            self.activate(&path, Priority::Periodic);
        }
    }

    /// Поднимает слежение, если его нет или оно сломалось.
    fn ensure_watching(self: &Arc<Self>, path: &Path) {
        let generation = lock(&self.projects)
            .get(path)
            .filter(|runtime| {
                !runtime.deleting && (runtime.watcher.is_none() || runtime.watch_failed)
            })
            .map(|runtime| Arc::clone(&runtime.job_gate));
        let Some(generation) = generation else {
            return;
        };
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
                .filter(|runtime| !runtime.deleting && Arc::ptr_eq(&runtime.job_gate, &generation))
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
            .is_some_and(|runtime| !runtime.deleting)
        {
            return;
        }
        if self.stop.load(Ordering::SeqCst) {
            return;
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
            WatchEvent::Files(files) => self.touch_files(project, files, DocSource::Watch),
            WatchEvent::Failed(message) => {
                if let Some(runtime) = lock(&self.projects).get_mut(project) {
                    runtime.watch_failed = true;
                    runtime.status.last_error = Some(format!("слежение за файлами: {message}"));
                }
                // События могли потеряться: проверка целиком.
                self.enqueue(project, Job::check(Priority::Watch));
            }
            WatchEvent::Rescan => self.enqueue(project, Job::check(Priority::Watch)),
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
                self.submit_docs(&item.project, item.paths, item.source);
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
            | Outcome::ReindexRequired(_) => {
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
            if runtime.tail_armed || runtime.deleting {
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
            if let Some(runtime) = lock(&inner.projects)
                .get_mut(&project)
                .filter(|r| !r.deleting && Arc::ptr_eq(&r.job_gate, &generation))
            {
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
            inner.enqueue(&project, Job::check(Priority::Watch));
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
            let silent = runtime.watch_events == 0;
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
            let next = {
                let mut queue = lock(&self.queue);
                let next = queue.pop();
                if next.is_some() {
                    self.busy.store(true, Ordering::SeqCst);
                }
                next
            };
            match next {
                Some((path, job)) => {
                    self.process(path, job).await;
                    self.busy.store(false, Ordering::SeqCst);
                }
                None => self.wake.notified().await,
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
            self.check_all();
        }
    }

    async fn process(self: &Arc<Self>, path: PathBuf, job: Job) {
        let operation_id = job.operation_id;
        let _pending = operations::PendingResult(Arc::clone(&self.models.operations), operation_id);
        let rebuild = job.rebuild.is_some();
        let Some(job_gate) = lock(&self.projects)
            .get(&path)
            .map(|r| Arc::clone(&r.job_gate))
        else {
            return;
        };
        let _job = Arc::clone(&job_gate).lock_owned().await;
        let full_check = job.rebuild.is_none() && job.scope == queue::Scope::Full;
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
            lock(&self.docs).begin(&path, explicit)
        };
        let worker = Arc::clone(self);
        let project = path.clone();
        lock(&self.models.operations).edit(operation_id, |op| op.state = "running".into());
        let taken = Arc::new(Mutex::new(taken.into_iter().collect()));
        let source = match job.priority {
            Priority::Explicit => DocSource::Explicit,
            Priority::Edit => DocSource::Edit,
            Priority::Watch => DocSource::Watch,
            Priority::Periodic => DocSource::Periodic,
        };
        let mut observer = JobObserver::new(Arc::clone(&self.pause), Arc::clone(&cell))
            .with_documents(Arc::new(doc_queue::Observer {
                docs: Arc::clone(&self.docs),
                project: path.clone(),
                source,
                taken: Arc::clone(&taken),
                wake: Arc::clone(&self.doc_wake),
                rebuild,
            }));
        if !rebuild {
            let docs = Arc::clone(&self.docs);
            let owner = path.clone();
            observer = observer.with_defer(Arc::new(move |document: &str, full_check: bool| {
                let docs = lock(&docs);
                (!full_check && docs.is_waiting(&owner, document)) || docs.is_error(&owner, document)
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
            Outcome::ReindexRequired(_) => log::info!("Нужна переиндексация: {}", path.display()),
        }
        let unfinished: Vec<_> = lock(&taken).iter().cloned().collect();
        self.finish_docs(&path, &unfinished, &result.outcome);
        let outcome = result.outcome.clone();
        // Публикация итога только после применения состояния проекта.
        self.apply(&path, previous, &job_gate, result);
        let mut journal = lock(&self.models.operations);
        journal.edit(operation_id, |op| {
            let p = cell.progress();
            op.docs_done = Some(p.docs_done);
            op.docs_total = cell.total_known().then_some(p.docs_total);
        });
        journal.index_result(operation_id, &outcome, rebuild);
        drop(journal);
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
                    status.last_error = if cancelled { None } else { Some(message) };
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
                Outcome::ReindexRequired(_) => {
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

/// Создаёт `.bars/config.toml`, если его нет; существующий файл не трогает (Т-5).
fn ensure_project_config(project: &Path) -> io::Result<()> {
    let file = project_config_path(project);
    if file.exists() {
        return Ok(());
    }
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        file,
        "# Настройки проекта bars. Что можно задать — docs/storage/settings.md.\n\
         # docs_root = \"docs\"\n",
    )
}

/// Недописанная переиндексация прошлого запуска оставляет `index.sqlite.new`: он не нужен
/// (`docs/architecture/worker.md`, «Завершение воркера»).
fn remove_stale_rebuild(project: &Path) {
    let index = bars_core::storage::index_path(project);
    let mut name = index.file_name().unwrap_or_default().to_os_string();
    name.push(".new");
    let new = index.with_file_name(&name);
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut file = new.clone().into_os_string();
        file.push(suffix);
        let _ = fs::remove_file(PathBuf::from(file));
    }
}
