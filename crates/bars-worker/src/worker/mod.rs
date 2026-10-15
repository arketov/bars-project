//! Воркер без окна: реестр проектов, очередь индексации, слежение за файлами и часовая проверка
//! (`docs/architecture/worker.md`, `docs/architecture/projects.md`, Т-5, Т-10, Т-15, Т-25).
//!
//! Обработчики протокола и CLI берут отсюда небольшой API [`Worker`]: регистрация, снятие,
//! выбор проекта, `status`, `reindex`, `session_start`, доступ к проекту для поиска и модели.
//! Поиск в очереди не стоит: он берёт [`Worker::gate`] на чтение и открывает индекс сам.

mod control;
mod job;
pub mod models;
mod queue;
mod registry;
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
use std::time::{Duration, SystemTime};

use crate::clients::InstallEnv;
use bars_core::indexing::indexer::UpdateReport;
use bars_core::secrets::{OsSecrets, SecretStore};
use bars_core::settings::{DEFAULT_DOCS_ROOT, Settings, project_config_path};
use bars_protocol::ProjectState;
use tokio::runtime::Handle;
use tokio::sync::{Notify, RwLock};
use tokio::task::JoinHandle;

pub use models::{
    BATCH_SIZE, BuiltinProvider, ConfiguredProvider, ModelProvider, ModelSlot, Models, RerankSlot,
    SharedEmbedder, SharedReranker,
};
pub use queue::Priority;
pub use registry::{Discovered, ProjectEntry, Registry, RegistryError, ResolveError};
pub use summary::{Health, Line, Summary};
pub use timefmt::rfc3339;

use control::{JobCell, JobObserver, Pause};
pub use control::{Kind as JobKind, Progress as JobProgress};
use job::{JobEnv, Outcome};
use queue::{Job, JobQueue};
use registry::{has_markdown, normalize, project_name};
use watcher::{DEBOUNCE, ProjectWatcher, Sink, WatchEvent};

/// Как часто воркер проверяет все проекты (Т-10).
pub const PERIODIC_INTERVAL: Duration = Duration::from_secs(3600);

/// Мьютекс, переживающий панику другого потока: состояние воркера не должно умирать от неё.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Clone)]
pub struct WorkerConfig {
    /// `~/.bars`: настройки, реестр, модели.
    pub bars_home: PathBuf,
    pub periodic_interval: Duration,
    /// Сколько события слежения копятся до пачки.
    pub debounce: Duration,
    /// Куда `install` и `uninstall` пишут записи клиентов; `None` — пути не определились.
    pub install_env: Option<InstallEnv>,
    /// Хранилище ключей провайдеров: сюда окно записывает ключ (`secret_set`).
    pub secrets: Arc<dyn SecretStore>,
}

impl std::fmt::Debug for WorkerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerConfig")
            .field("bars_home", &self.bars_home)
            .field("periodic_interval", &self.periodic_interval)
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
            debounce: DEBOUNCE,
            install_env: InstallEnv::detect(),
            secrets: Arc::new(OsSecrets::default()),
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
    pub project: PathBuf,
    /// Файл и причина.
    pub files: Vec<(String, String)>,
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
    status: RuntimeStatus,
    watcher: Option<ProjectWatcher>,
    /// `notify` сообщил об ошибке или папка пропадала: слежение надо поднять заново.
    watch_failed: bool,
    gate: Arc<RwLock<()>>,
}

impl Runtime {
    fn new() -> Self {
        Self {
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
            gate: Arc::new(RwLock::new(())),
        }
    }
}

struct Inner {
    config: WorkerConfig,
    registry: Mutex<Registry>,
    projects: Mutex<HashMap<PathBuf, Runtime>>,
    queue: Mutex<JobQueue>,
    /// Задание вынуто из очереди и ещё выполняется.
    busy: AtomicBool,
    wake: Notify,
    stop: AtomicBool,
    models: Models,
    handle: Handle,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    /// Пауза очереди (OQ-55).
    pause: Arc<Pause>,
    /// Идущее задание: его ход и флаг остановки.
    current: Mutex<Option<Arc<JobCell>>>,
    /// Модели, которые сейчас скачиваются по кнопке окна.
    downloads: Mutex<HashSet<String>>,
}

/// Воркер. Копии указывают на одно и то же состояние.
#[derive(Clone)]
pub struct Worker {
    inner: Arc<Inner>,
}

impl Worker {
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
            config,
            registry: Mutex::new(registry),
            projects: Mutex::new(HashMap::new()),
            queue: Mutex::new(JobQueue::new()),
            busy: AtomicBool::new(false),
            wake: Notify::new(),
            stop: AtomicBool::new(false),
            models: Models::new(provider),
            handle: Handle::current(),
            tasks: Mutex::new(Vec::new()),
            pause: Arc::new(Pause::default()),
            current: Mutex::new(None),
            downloads: Mutex::new(HashSet::new()),
        });
        let queue_task = tokio::spawn(Arc::clone(&inner).run_queue());
        let timer_task = tokio::spawn(Arc::clone(&inner).run_timer());
        lock(&inner.tasks).extend([queue_task, timer_task]);
        for path in paths {
            inner.activate(&path, Priority::Periodic);
        }
        Ok(Self { inner })
    }

    /// Перестаёт брать задания, дожидается текущего, снимает слежение.
    pub async fn shutdown(&self) {
        self.inner.stop.store(true, Ordering::SeqCst);
        // Задание на паузе иначе не закончится никогда: его останавливают, остальные дописываются.
        if self.inner.pause.is_paused() {
            self.inner.cancel_current();
            self.inner.pause.set(false);
        }
        self.inner.wake.notify_one();
        let tasks: Vec<_> = lock(&self.inner.tasks).drain(..).collect();
        // Первая — очередь: она дописывает текущее задание сама. Вторая — таймер, он ждёт часами.
        for (position, task) in tasks.into_iter().enumerate() {
            if position == 1 {
                task.abort();
            }
            let _ = task.await;
        }
        let watchers: Vec<_> = lock(&self.inner.projects)
            .values_mut()
            .filter_map(|runtime| runtime.watcher.take())
            .collect();
        drop(watchers);
    }

    // --- реестр ---

    /// Регистрирует папку как проект: ровно ту, что указана (Т-6). Создаёт `.bars/config.toml`,
    /// если его нет, запускает слежение и ставит проект на проверку. Повторная регистрация
    /// ставит на проверку ещё раз.
    pub fn register(&self, path: &Path) -> Result<Registered, WorkerError> {
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
        let path = self.resolve(selector)?;
        lock(&self.inner.registry).remove(&path)?;
        let runtime = lock(&self.inner.projects).remove(&path);
        lock(&self.inner.queue).remove(&path);
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
                let state = lock(&self.inner.projects)
                    .get(&entry.path)
                    .map_or(ProjectState::Unavailable, |runtime| runtime.status.state);
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
            return Ok(Some(discovered.root));
        }
        if !discovered.root.is_dir()
            || !has_markdown(&discovered.root, &self.inner.docs_root_of(&discovered.root))
        {
            return Ok(None);
        }
        Ok(Some(self.register(&discovered.root)?.path))
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
        let status = &runtime.status;
        Ok(ProjectStatus {
            state: status.state,
            updated_at: status.updated_at,
            pending: queued.map_or(0, Job::listed_files),
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
        self.inner.enqueue(&path, job);
        Ok(())
    }

    // --- пауза, остановка, ход ---

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
        let running = {
            let current = lock(&self.inner.current);
            current
                .as_ref()
                .filter(|cell| cell.project == path)
                .map(|cell| cell.cancel())
                .is_some()
        };
        let queued = lock(&self.inner.queue).remove(&path);
        Ok(running || queued)
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

    /// Проблемы проектов по итогам последней проверки: файлы с ошибками и `last_error`.
    pub fn problems(&self) -> Vec<ProjectIssues> {
        let projects = lock(&self.inner.projects);
        let mut out: Vec<_> = projects
            .iter()
            .filter_map(|(path, runtime)| {
                let files: Vec<(String, String)> = runtime
                    .status
                    .last_report
                    .as_ref()
                    .map(|report| {
                        report
                            .problems
                            .iter()
                            .map(|problem| (problem.path.clone(), problem.reason.clone()))
                            .collect()
                    })
                    .unwrap_or_default();
                let error = runtime.status.last_error.clone();
                (!files.is_empty() || error.is_some()).then(|| ProjectIssues {
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
            .and_then(|runtime| runtime.watcher.take());
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
    pub fn secrets(&self) -> &Arc<dyn SecretStore> {
        &self.inner.config.secrets
    }

    /// Папка моделей (`~/.bars/models`).
    pub fn models_dir(&self) -> PathBuf {
        self.inner.config.bars_home.join("models")
    }

    /// Модели, которые скачиваются прямо сейчас.
    pub fn downloading(&self) -> HashSet<String> {
        lock(&self.inner.downloads).clone()
    }

    /// Скачивает встроенную модель в фоне. `false`, если она уже скачивается. Загрузка идёт в
    /// блокирующей задаче: окно узнаёт о конце по `models_info`.
    pub fn start_download(&self, id: &str, reranker: bool) -> bool {
        if !lock(&self.inner.downloads).insert(id.to_string()) {
            return false;
        }
        let inner = Arc::clone(&self.inner);
        let id = id.to_string();
        let mut user = bars_core::settings::UserSettings::default();
        if reranker {
            user.rerank.model.clone_from(&id);
        } else {
            user.embed.model.clone_from(&id);
        }
        let settings = Settings::resolve(&user, &bars_core::settings::ProjectConfig::default());
        self.inner.handle.spawn_blocking(move || {
            // Ошибка скачивания видна по тому, что модель так и не стала `downloaded`; текст в лог.
            if let Err(error) = inner.models.download(&settings, reranker) {
                eprintln!("bars: не удалось скачать модель {id}: {error}");
            }
            lock(&inner.downloads).remove(&id);
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

    /// Очередь пуста и ничего не выполняется.
    pub fn is_idle(&self) -> bool {
        let queue = lock(&self.inner.queue);
        queue.is_empty() && !self.inner.busy.load(Ordering::SeqCst)
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

    fn enqueue(&self, project: &Path, job: Job) {
        lock(&self.queue).push(project, job);
        self.wake.notify_one();
    }

    /// Вводит проект в работу: папка на месте, слежение запущено, проверка в очереди. Папки нет —
    /// проект `unavailable`.
    fn activate(self: &Arc<Self>, path: &Path, priority: Priority) {
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
        let needed = lock(&self.projects)
            .get(path)
            .is_some_and(|runtime| runtime.watcher.is_none() || runtime.watch_failed);
        if !needed {
            return;
        }
        let weak: Weak<Inner> = Arc::downgrade(self);
        let key = path.to_path_buf();
        let sink: Sink = Arc::new(move |event| {
            if let Some(inner) = weak.upgrade() {
                inner.on_watch_event(&key, event);
            }
        });
        let started =
            ProjectWatcher::start(path, &self.docs_root_of(path), self.config.debounce, sink).ok();
        let replaced = lock(&self.projects).get_mut(path).map(|runtime| {
            runtime.watch_failed = started.is_none();
            std::mem::replace(&mut runtime.watcher, started)
        });
        // Прежнее слежение останавливается вне блокировки: его поток может ждать её.
        drop(replaced);
    }

    /// Вызывается в потоке `notify`: останавливать слежение отсюда нельзя.
    fn on_watch_event(self: &Arc<Self>, project: &Path, event: WatchEvent) {
        if self.stop.load(Ordering::SeqCst) {
            return;
        }
        if !project.is_dir() {
            if let Some(watcher) = self.mark_unavailable(project) {
                std::thread::spawn(move || drop(watcher));
            }
            return;
        }
        match event {
            WatchEvent::Files(files) => self.enqueue(project, Job::files(Priority::Watch, files)),
            WatchEvent::Failed(message) => {
                if let Some(runtime) = lock(&self.projects).get_mut(project) {
                    runtime.watch_failed = true;
                    runtime.status.last_error = Some(format!("слежение за файлами: {message}"));
                }
                // События могли потеряться: проверка целиком.
                self.enqueue(project, Job::check(Priority::Watch));
            }
        }
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
        lock(&self.queue).remove(path);
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
        let (gate, previous) = {
            let mut projects = lock(&self.projects);
            let Some(runtime) = projects.get_mut(&path) else {
                return;
            };
            let previous = runtime.status.state;
            if previous == ProjectState::Unavailable {
                return;
            }
            if previous != ProjectState::ReindexRequired {
                runtime.status.state = ProjectState::Indexing;
            }
            (Arc::clone(&runtime.gate), previous)
        };

        let worker = Arc::clone(self);
        let project = path.clone();
        let cell = JobCell::new(&path, &job);
        *lock(&self.current) = Some(Arc::clone(&cell));
        let observer = JobObserver::new(Arc::clone(&self.pause), cell);
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
        *lock(&self.current) = None;

        let result = result.unwrap_or_else(|error| job::JobResult {
            outcome: Outcome::Failed(format!("внутренняя ошибка воркера: {error}")),
            index_model: None,
            settings_model: None,
        });
        self.apply(&path, previous, result);
    }

    fn apply(&self, path: &Path, previous: ProjectState, result: job::JobResult) {
        let mut stopped = None;
        {
            let mut projects = lock(&self.projects);
            let Some(runtime) = projects.get_mut(path) else {
                return;
            };
            let status = &mut runtime.status;
            if let Some(model) = result.index_model {
                status.model_index = Some(model);
            }
            if let Some(model) = result.settings_model {
                status.model_settings = Some(model);
            }
            match result.outcome {
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
