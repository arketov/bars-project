//! Тесты воркера на временных каталогах и подделке модели.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use bars_core::storage::{Index, Journal, index_path};
use bars_protocol::ProjectState;
use tempfile::TempDir;

use super::models::fake::FakeProvider;
use super::registry::Registry;
use super::*;

mod activity;
mod deletion;
mod doc_queue;
mod folders;
mod graph;
mod operations;
mod shutdown;

struct Env {
    home: TempDir,
    projects: TempDir,
    provider: Arc<FakeProvider>,
}

impl Env {
    fn new() -> Self {
        Self {
            home: TempDir::new().unwrap(),
            projects: TempDir::new().unwrap(),
            provider: FakeProvider::new("m1"),
        }
    }

    fn config(&self) -> WorkerConfig {
        let mut config = WorkerConfig::new(self.home.path().to_path_buf());
        config.debounce = Duration::from_millis(100);
        config.edit_pause = Some(Duration::ZERO);
        config.graph_quiet = Duration::ZERO;
        config.graph_min_interval = Duration::ZERO;
        config
    }

    fn start(&self) -> Worker {
        Worker::start(self.config(), self.provider.clone()).unwrap()
    }

    /// Проект с документами: `(путь внутри docs, текст)`.
    fn project(&self, name: &str, docs: &[(&str, &str)]) -> PathBuf {
        let root = self.projects.path().join(name);
        fs::create_dir_all(root.join("docs")).unwrap();
        for (file, text) in docs {
            let path = root.join("docs").join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        root
    }
}

const DOC: &str = "# Заголовок\n\nТекст раздела про индексацию.\n";

#[tokio::test(flavor = "multi_thread")]
async fn notification_download_failure_is_retained_and_cleared_on_retry() {
    let failure =
        failed_download(|| bars_core::models::EmbedError::Load("backend test failure".into()))
            .await;
    assert_eq!(failure.kind.as_str(), "runtime");
    assert!(failure.message.contains("журнале воркера"));
}

#[tokio::test(flavor = "multi_thread")]
async fn download_failure_kind_follows_the_error_type_and_text_stays_safe() {
    let network = failed_download(|| {
        bars_core::models::EmbedError::Network(
            "https://huggingface.co/x?token=secret: timeout".into(),
        )
    })
    .await;
    assert_eq!(network.kind.as_str(), "network");
    assert!(network.message.contains("Проверьте подключение"));
    assert!(!network.message.contains("secret"));
    let files =
        failed_download(|| bars_core::models::EmbedError::Files("SHA-256 не совпал".into())).await;
    assert_eq!(files.kind.as_str(), "files_missing");
    assert!(files.message.contains("повреждены"));
    // Слова про файлы и сеть в тексте библиотеки вид не меняют; нехватка памяти — по списку признаков.
    let words = failed_download(|| {
        bars_core::models::EmbedError::Load("файл не найден, нет сети huggingface.co".into())
    })
    .await;
    assert_eq!(words.kind.as_str(), "runtime");
    let memory = failed_download(|| {
        bars_core::models::EmbedError::Load("DirectML: 0x8007000E bad allocation".into())
    })
    .await;
    assert_eq!(memory.kind.as_str(), "out_of_memory");
    assert!(memory.message.contains("не хватило памяти"));
}

/// Скачивание, которое всегда падает с заданной ошибкой: проверяет хранение ошибки, повтор и
/// очистку при новом запуске; возвращает сохранённую ошибку.
async fn failed_download(
    make: fn() -> bars_core::models::EmbedError,
) -> bars_core::models::ModelFailure {
    let env = Env::new();
    struct FailedDownload(fn() -> bars_core::models::EmbedError);
    impl ModelProvider for FailedDownload {
        fn fingerprint(
            &self,
            settings: &Settings,
        ) -> Result<bars_core::models::ModelFingerprint, bars_core::models::EmbedError> {
            FakeProvider::new("m1").fingerprint(settings)
        }
        fn load(
            &self,
            settings: &Settings,
        ) -> Result<Box<dyn bars_core::models::Embedder + Send>, bars_core::models::EmbedError>
        {
            assert_eq!(
                settings.embed.source,
                bars_core::settings::ModelSource::Builtin
            );
            assert_eq!(
                settings.embed.accelerator,
                bars_core::models::Accelerator::Cpu
            );
            std::thread::sleep(Duration::from_millis(150));
            Err((self.0)())
        }
    }
    fs::write(env.home.path().join("settings.toml"), "[embed]\nsource='remote'\naccelerator='cpu'\nmodel='test'\nbase_url='https://example.invalid/v1'\napi_key_name='test'\n").unwrap();
    bars_core::settings::UserSettings::load(env.home.path()).unwrap();
    let worker = Worker::start(env.config(), Arc::new(FailedDownload(make))).unwrap();
    assert!(worker.start_download("BAAI/bge-m3", false));
    assert!(!worker.start_download("BAAI/bge-m3", false));
    wait_until("ошибка скачивания", || {
        worker.downloading().is_empty()
    })
    .await;
    let errors = worker.download_errors();
    let failure = errors["BAAI/bge-m3"].clone();
    assert_eq!(
        worker.download_errors(),
        errors,
        "опрос не поглощает ошибку"
    );
    assert!(worker.start_download("BAAI/bge-m3", false));
    assert!(worker.download_errors().is_empty());
    wait_until("повтор скачивания", || {
        worker.downloading().is_empty()
    })
    .await;
    worker.shutdown().await;
    failure
}

#[tokio::test(flavor = "multi_thread")]
async fn document_menu_check_queues_explicit_file_and_updates_index() {
    let env = Env::new();
    let project = env.project("проект с пробелами", &[("Тема.md", DOC)]);
    let mut config = env.config();
    config.install_env = None;
    let worker = Worker::start(config, env.provider.clone()).unwrap();
    worker.register(&project).unwrap();
    idle(&worker).await;
    worker.pause();
    let file = project.join("docs/Тема.md");
    fs::write(&file, "---\ntitle: Новый заголовок\n---\n\n# Новый заголовок\n\nИзменённое содержимое документа.\n").unwrap();
    worker.check_files(&project, vec![file]).unwrap();
    let queue = worker.queued();
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].kind, JobKind::Files);
    assert_eq!(queue[0].priority, Priority::Explicit);
    assert_eq!(queue[0].files, 1);
    worker.resume();
    idle(&worker).await;
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    let title: String = index
        .conn()
        .query_row("SELECT title FROM documents", [], |row| row.get(0))
        .unwrap();
    assert_eq!(title, "Новый заголовок");
    drop(index);
    fs::remove_file(project.join("docs/Тема.md")).unwrap();
    worker
        .check_files(&project, vec![project.join("docs/Тема.md")])
        .unwrap();
    idle(&worker).await;
    assert_eq!(documents(&project), 0);
    env.provider.set_model("m2");
    worker
        .check_files(&project, vec![project.join("docs/Тема.md")])
        .unwrap();
    idle(&worker).await;
    assert_eq!(
        worker.status(&project).unwrap().state,
        ProjectState::ReindexRequired
    );
    assert!(matches!(
        worker.check_files(&project, vec![project.join("docs/Тема.md")]),
        Err(WorkerError::ConsentRequired(_))
    ));
    worker.shutdown().await;
}

async fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !condition() {
        assert!(Instant::now() < deadline, "не дождались: {what}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn idle(worker: &Worker) {
    wait_until("очередь опустеет", || worker.is_idle()).await;
}

fn documents(project: &Path) -> usize {
    let index = Index::open(&index_path(project), Journal::Wal).unwrap();
    let count = index.document_count().unwrap();
    index.close().unwrap();
    count
}

fn state(worker: &Worker, project: &Path) -> ProjectState {
    worker.status(project).unwrap().state
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registered_project_is_indexed_without_permanent_watching() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC), ("mod/b.md", DOC)]);
    let worker = env.start();

    let registered = worker.register(&project).unwrap();
    assert!(!registered.already);
    idle(&worker).await;

    let status = worker.status(&project).unwrap();
    assert_eq!(status.state, ProjectState::Ready);
    assert!(status.updated_at.is_some());
    assert!(!status.watching);
    assert_eq!(status.last_error, None);
    assert_eq!(status.model_index, status.model_settings);
    assert_eq!(status.model_index.as_deref(), Some("builtin:m1"));
    // A watcher follow-up can already replace the initial report with an unchanged check.
    let report = status.last_report.unwrap();
    assert_eq!(report.added + report.changed + report.unchanged, 2);
    assert_eq!(documents(&project), 2);
    assert!(project.join(".bars/config.toml").is_file());
    assert_eq!(
        Registry::load(env.home.path()).unwrap().paths(),
        vec![project]
    );
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn register_twice_keeps_one_entry() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC)]);
    let worker = env.start();
    worker.register(&project).unwrap();
    assert!(worker.register(&project).unwrap().already);
    idle(&worker).await;
    assert_eq!(worker.projects().len(), 1);
    assert!(matches!(
        worker.register(&env.projects.path().join("nope")),
        Err(WorkerError::NotADirectory(_))
    ));
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn existing_project_config_is_kept() {
    let env = Env::new();
    let project = env.project("alpha", &[]);
    fs::create_dir_all(project.join("handbook")).unwrap();
    fs::write(project.join("handbook/x.md"), DOC).unwrap();
    fs::create_dir_all(project.join(".bars")).unwrap();
    fs::write(
        project.join(".bars/config.toml"),
        "docs_root = \"handbook\"\n",
    )
    .unwrap();
    let worker = env.start();

    worker.register(&project).unwrap();
    idle(&worker).await;

    assert_eq!(
        fs::read_to_string(project.join(".bars/config.toml")).unwrap(),
        "docs_root = \"handbook\"\n"
    );
    assert_eq!(documents(&project), 1);
    assert_eq!(worker.projects()[0].docs_root, "handbook");
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn all_registered_projects_are_checked_at_start() {
    let env = Env::new();
    let one = env.project("one", &[("a.md", DOC)]);
    let two = env.project("two", &[("b.md", DOC), ("c.md", DOC)]);
    {
        let mut registry = Registry::load(env.home.path()).unwrap();
        registry.add(&one).unwrap();
        registry.add(&two).unwrap();
    }
    let worker = env.start();
    idle(&worker).await;

    assert_eq!(documents(&one), 1);
    assert_eq!(documents(&two), 2);
    assert!(
        worker
            .projects()
            .iter()
            .all(|p| p.state == ProjectState::Ready)
    );
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_change_reaches_the_index_without_manual_actions() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC)]);
    let worker = env.start();
    worker.register(&project).unwrap();
    worker.observe_mcp(&project).unwrap();
    idle(&worker).await;
    assert_eq!(documents(&project), 1);

    fs::write(project.join("docs/new.md"), DOC).unwrap();
    wait_until("новый документ в индексе", || {
        worker.is_idle() && documents(&project) == 2
    })
    .await;

    fs::remove_file(project.join("docs/a.md")).unwrap();
    wait_until(
        "удалённый документ ушёл из индекса",
        || worker.is_idle() && documents(&project) == 1,
    )
    .await;
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hourly_check_finds_changes_the_watcher_missed() {
    let env = Env::new();
    // Корень документации `docs/` выбран, но папки ещё нет: слежение не стартует, первая
    // проверка кончается ошибкой. Без выбора корнем стала бы вся папка (Т-7).
    let root = env.projects.path().join("late");
    fs::create_dir_all(root.join(".bars")).unwrap();
    fs::write(root.join(".bars/config.toml"), "docs_root = \"docs\"\n").unwrap();
    let mut config = env.config();
    config.periodic_interval = Duration::from_millis(200);
    let worker = Worker::start(config, env.provider.clone()).unwrap();
    worker.register(&root).unwrap();
    worker.observe_mcp(&root).unwrap();
    idle(&worker).await;
    let status = worker.status(&root).unwrap();
    assert!(!status.watching);
    assert!(status.last_error.is_some());

    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(root.join("docs/a.md"), DOC).unwrap();
    wait_until(
        "таймер проиндексировал документ",
        || worker.is_idle() && worker.status(&root).is_ok_and(|s| s.last_error.is_none()),
    )
    .await;
    assert_eq!(documents(&root), 1);
    assert!(
        worker.status(&root).unwrap().watching,
        "таймер поднял слежение"
    );
    worker.shutdown().await;
}

/// ISS-06: агент переложил сотни файлов разом; индекс должен догнать их без часовой проверки.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hundreds_of_files_at_once_reach_the_index() {
    let env = Env::new();
    let project = env.project("bulk", &[("a.md", DOC)]);
    let mut config = env.config();
    config.tail_delay = Duration::from_millis(300);
    let worker = Worker::start(config, env.provider.clone()).unwrap();
    worker.register(&project).unwrap();
    worker.observe_mcp(&project).unwrap();
    idle(&worker).await;

    const FILES: usize = 400;
    for number in 0..FILES {
        let folder = project.join("docs").join(format!("module-{}", number % 20));
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(format!("doc-{number}.md")), DOC).unwrap();
    }
    wait_until("все файлы в индексе", || {
        worker.is_idle() && documents(&project) == FILES + 1
    })
    .await;
    worker.shutdown().await;
}

/// ISS-06: слежение, которое замолчало, перезапускается: часовая проверка нашла файл,
/// а событий не было, значит, дальше правки должны снова доходить сами.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deaf_watcher_is_restarted_by_the_periodic_check() {
    let env = Env::new();
    let project = env.project("deaf", &[("a.md", DOC)]);
    let mut config = env.config();
    config.tail_delay = Duration::from_millis(150);
    let worker = Worker::start(config, env.provider.clone()).unwrap();
    let path = worker.register(&project).unwrap().path;
    worker.observe_mcp(&path).unwrap();
    idle(&worker).await;
    assert_eq!(documents(&project), 1);

    // Слежение «оглохло»: оно живо, но событий не передаёт.
    let deaf = ProjectWatcher::start(
        &project,
        "docs",
        Duration::from_millis(100),
        Arc::new(|_| {}),
    )
    .unwrap();
    let old = lock(&worker.inner.projects)
        .get_mut(&path)
        .and_then(|runtime| runtime.watcher.replace(deaf));
    drop(old);
    assert!(worker.status(&path).unwrap().watching);

    fs::write(project.join("docs/b.md"), DOC).unwrap();
    worker.check_all();
    wait_until("часовая проверка нашла b.md", || {
        worker.is_idle() && documents(&project) == 2
    })
    .await;

    fs::write(project.join("docs/c.md"), DOC).unwrap();
    wait_until(
        "слежение снова видит правки",
        || worker.is_idle() && documents(&project) == 3,
    )
    .await;
    worker.shutdown().await;
}

/// ISS-106: без папки корня документации слежение не запускается и не повторяет попыток,
/// а когда папка вернулась, поднимается при ближайшем подтверждении.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_docs_root_does_not_retry_watching_and_recovers() {
    let env = Env::new();
    let project = env.project("noroot", &[("a.md", DOC)]);
    let worker = Worker::start(env.config(), env.provider.clone()).unwrap();
    let path = worker.register(&project).unwrap().path;
    idle(&worker).await;

    fs::remove_dir_all(project.join("docs")).unwrap();
    worker.observe_mcp(&path).unwrap();
    worker.observe_mcp(&path).unwrap();
    {
        let projects = lock(&worker.inner.projects);
        let runtime = projects.get(&path).unwrap();
        assert!(runtime.docs_root_missing);
        assert!(runtime.watcher.is_none());
        assert!(!runtime.watch_failed);
    }

    fs::create_dir_all(project.join("docs")).unwrap();
    fs::write(project.join("docs/a.md"), DOC).unwrap();
    worker.observe_mcp(&path).unwrap();
    assert!(worker.status(&path).unwrap().watching);
    assert!(
        !lock(&worker.inner.projects)
            .get(&path)
            .unwrap()
            .docs_root_missing
    );
    worker.shutdown().await;
}

/// ISS-106: у ненаблюдаемого проекта периодическая проверка не объявляет слежение умершим.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn silent_watcher_of_unobserved_project_is_left_alone() {
    let env = Env::new();
    let project = env.project("unseen", &[("a.md", DOC)]);
    let worker = Worker::start(env.config(), env.provider.clone()).unwrap();
    let path = worker.register(&project).unwrap().path;
    idle(&worker).await;

    let stray = ProjectWatcher::start(
        &project,
        "docs",
        Duration::from_millis(100),
        Arc::new(|_| {}),
    )
    .unwrap();
    lock(&worker.inner.projects).get_mut(&path).unwrap().watcher = Some(stray);
    worker.inner.after_full_check(&path, true);
    assert!(
        !lock(&worker.inner.projects)
            .get(&path)
            .unwrap()
            .watch_failed
    );
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn model_change_requires_reindex_and_waits_for_consent() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC), ("b.md", DOC)]);
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    let embedded = env.provider.counters.texts.load(Ordering::SeqCst);
    assert!(embedded > 0);

    // В настройках другая модель: проверка не запускает эмбеддинг и не переиндексирует сама.
    env.provider.set_model("m2");
    worker.reindex(&project, false, false).unwrap();
    idle(&worker).await;
    let status = worker.status(&project).unwrap();
    assert_eq!(status.state, ProjectState::ReindexRequired);
    assert_eq!(status.model_index.as_deref(), Some("builtin:m1"));
    assert_eq!(status.model_settings.as_deref(), Some("builtin:m2"));
    assert_eq!(env.provider.counters.texts.load(Ordering::SeqCst), embedded);

    // Часовая проверка и хук состояния не меняют (Т-25).
    worker.check_all();
    idle(&worker).await;
    assert_eq!(state(&worker, &project), ProjectState::ReindexRequired);
    assert_eq!(env.provider.counters.texts.load(Ordering::SeqCst), embedded);

    // Без согласия переиндексация не принимается, даже полная.
    for full in [false, true] {
        assert!(matches!(
            worker.reindex(&project, full, false),
            Err(WorkerError::ConsentRequired(_))
        ));
    }
    idle(&worker).await;
    assert_eq!(state(&worker, &project), ProjectState::ReindexRequired);

    // С согласием индекс строится заново новой моделью.
    worker.reindex(&project, false, true).unwrap();
    idle(&worker).await;
    let status = worker.status(&project).unwrap();
    assert_eq!(status.state, ProjectState::Ready);
    assert_eq!(status.model_index.as_deref(), Some("builtin:m2"));
    assert!(env.provider.counters.texts.load(Ordering::SeqCst) > embedded);
    assert_eq!(documents(&project), 2);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_reindex_with_stale_state_still_asks_for_consent() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC)]);
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;

    // Состояние ещё `ready`, но модель в настройках уже другая: задание само сверяет отпечатки.
    env.provider.set_model("m2");
    worker.reindex(&project, true, false).unwrap();
    idle(&worker).await;
    let status = worker.status(&project).unwrap();
    assert_eq!(status.state, ProjectState::ReindexRequired);
    assert_eq!(status.model_index.as_deref(), Some("builtin:m1"));
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_reindex_waits_for_searches_before_swapping_the_file() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC)]);
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;

    // Поиск, который ещё читает индекс.
    let reader = worker.gate(&project).unwrap().read_owned().await;
    worker.reindex(&project, true, false).unwrap();
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(!worker.is_idle(), "подмена ждёт читающего");
    assert_eq!(state(&worker, &project), ProjectState::Indexing);

    drop(reader);
    idle(&worker).await;
    assert_eq!(state(&worker, &project), ProjectState::Ready);
    assert_eq!(documents(&project), 1);
    assert!(!index_path(&project).with_extension("sqlite.new").exists());
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_project_is_unavailable_and_returns_when_the_folder_does() {
    let env = Env::new();
    let gone = env.projects.path().join("gone");
    {
        let mut registry = Registry::load(env.home.path()).unwrap();
        registry.add(&gone).unwrap();
    }
    let worker = env.start();
    idle(&worker).await;

    let status = worker.status(&gone).unwrap();
    assert_eq!(status.state, ProjectState::Unavailable);
    assert!(!status.watching);
    assert!(matches!(
        worker.reindex(&gone, false, false),
        Err(WorkerError::Unavailable(_))
    ));
    // Из реестра проект не исчезает: папка может быть на отключённом диске.
    assert_eq!(worker.projects().len(), 1);
    assert_eq!(worker.projects()[0].state, ProjectState::Unavailable);

    fs::create_dir_all(gone.join("docs")).unwrap();
    fs::write(gone.join("docs/a.md"), DOC).unwrap();
    worker.check_all();
    idle(&worker).await;
    let status = worker.status(&gone).unwrap();
    assert_eq!(status.state, ProjectState::Ready);
    assert!(!status.watching);
    assert_eq!(documents(&gone), 1);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn moved_project_is_registered_anew_and_reuses_its_index() {
    let env = Env::new();
    let old = env.project("old", &[("a.md", DOC)]);
    let worker = env.start();
    worker.register(&old).unwrap();
    idle(&worker).await;
    let embedded = env.provider.counters.texts.load(Ordering::SeqCst);
    worker.shutdown().await;

    // Папку вместе с `.bars/` перенесли; в реестре осталась старая запись.
    let new = env.projects.path().join("new");
    fs::rename(&old, &new).unwrap();
    let worker = env.start();
    worker.register(&new).unwrap();
    idle(&worker).await;

    assert_eq!(state(&worker, &old), ProjectState::Unavailable);
    assert_eq!(state(&worker, &new), ProjectState::Ready);
    assert_eq!(
        env.provider.counters.texts.load(Ordering::SeqCst),
        embedded,
        "тот же индекс подхвачен без переэмбеддинга (Т-6)"
    );
    assert_eq!(documents(&new), 1);

    // Старую запись убирает пользователь.
    worker.unregister(&old.to_string_lossy()).unwrap();
    assert_eq!(worker.projects().len(), 1);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unregister_stops_everything_and_keeps_project_files() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC)]);
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;

    assert_eq!(worker.unregister("alpha").unwrap(), project);
    assert!(
        Registry::load(env.home.path())
            .unwrap()
            .entries()
            .is_empty()
    );
    assert!(matches!(
        worker.status(&project),
        Err(WorkerError::NotRegistered(_))
    ));
    assert!(project.join(".bars/index.sqlite").is_file());
    assert!(project.join("docs/a.md").is_file());

    // Правка после снятия в индекс не попадает.
    fs::write(project.join("docs/b.md"), DOC).unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(worker.is_idle());
    assert_eq!(documents(&project), 1);

    // Повторная регистрация подхватывает `.bars/` как есть.
    worker.register(&project).unwrap();
    idle(&worker).await;
    assert_eq!(documents(&project), 2);
    assert!(matches!(
        worker.unregister("nope"),
        Err(WorkerError::Resolve(ResolveError::NotFound(_)))
    ));
    worker.shutdown().await;
}

/// Приёмка Т-7: область вложенного проекта уходит из индекса внешнего без пересборки и
/// возвращается после снятия вложенного; `bars_search` (occurrences) и чтение по диску не ограничены.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nested_project_area_leaves_the_outer_index_and_returns_when_it_is_unregistered() {
    let env = Env::new();
    let outer = env.project("outer", &[("a.md", DOC)]);
    let inner = outer.join("docs/inner");
    fs::create_dir_all(&inner).unwrap();
    fs::write(inner.join("x.md"), DOC).unwrap();
    let worker = env.start();
    worker.register(&outer).unwrap();
    idle(&worker).await;
    assert_eq!(documents(&outer), 2);

    worker.register(&inner).unwrap();
    idle(&worker).await;
    assert_eq!(documents(&outer), 1);
    assert_eq!(documents(&inner), 1);
    let nested = worker.nested_projects(&outer, true);
    assert_eq!(nested.len(), 1);
    assert_eq!(nested[0].dir, "docs/inner");
    assert_eq!(nested[0].documents, 1);
    assert!(inner.join("x.md").is_file());

    fs::write(inner.join("y.md"), DOC).unwrap();
    worker.unregister(&inner.display().to_string()).unwrap();
    idle(&worker).await;
    assert_eq!(documents(&outer), 3);
    assert!(worker.nested_projects(&outer, false).is_empty());
    worker.shutdown().await;
}

/// Приёмка Т-7: пропавший вложенный проект остаётся исключённым, пока есть запись в реестре.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unavailable_nested_project_stays_excluded_until_it_is_removed() {
    let env = Env::new();
    let outer = env.project("outer", &[("a.md", DOC)]);
    let inner = outer.join("docs/inner");
    fs::create_dir_all(&inner).unwrap();
    fs::write(inner.join("x.md"), DOC).unwrap();
    let worker = env.start();
    worker.register(&outer).unwrap();
    worker.register(&inner).unwrap();
    idle(&worker).await;
    assert_eq!(documents(&outer), 1);

    fs::remove_dir_all(&inner).unwrap();
    worker.check_all();
    idle(&worker).await;
    assert_eq!(documents(&outer), 1);
    assert_eq!(worker.nested_projects(&outer, false).len(), 1);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_start_finds_the_root_and_registers_only_real_documentation() {
    let env = Env::new();
    let worker = env.start();

    // cwd в подпапке git-репозитория с документацией: регистрируется корень репозитория.
    let repo = env.project("repo", &[("a.md", DOC)]);
    fs::write(repo.join(".git"), "gitdir: x").unwrap();
    let cwd = repo.join("src/deep");
    fs::create_dir_all(&cwd).unwrap();
    assert_eq!(worker.session_start(&cwd).unwrap(), Some(repo.clone()));
    idle(&worker).await;
    assert_eq!(documents(&repo), 1);

    // Вторая сессия — уже зарегистрированный проект, новой записи нет.
    assert_eq!(worker.session_start(&cwd).unwrap(), Some(repo.clone()));
    assert_eq!(worker.projects().len(), 1);
    assert_eq!(worker.project_for_cwd(&cwd), Some(repo));

    // Папка без документации реестр не засоряет.
    let lone = env.projects.path().join("lone");
    fs::create_dir_all(&lone).unwrap();
    assert_eq!(worker.session_start(&lone).unwrap(), None);
    assert_eq!(worker.projects().len(), 1);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_start_skips_a_folder_without_docs_or_config() {
    let env = Env::new();
    let worker = env.start();

    // Папка заметок без `docs/` и `.bars/`: хук её не регистрирует (Т-6, OQ-253).
    let notes = env.projects.path().join("notes");
    fs::create_dir_all(notes.join("clients")).unwrap();
    fs::write(notes.join("clients/script.md"), DOC).unwrap();
    assert_eq!(worker.session_start(&notes).unwrap(), None);
    assert!(worker.projects().is_empty());

    // Явный выбор человека (регистрация) работает: корень по умолчанию — вся папка.
    worker.register(&notes).unwrap();
    let config = fs::read_to_string(notes.join(".bars/config.toml")).unwrap();
    assert!(config.contains("docs_root = \".\""), "{config}");

    // Проект с `.bars/config.toml` и своим корнем хук регистрирует.
    let manual = env.projects.path().join("manual");
    fs::create_dir_all(manual.join(".bars")).unwrap();
    fs::write(manual.join(".bars/config.toml"), "docs_root = \".\"\n").unwrap();
    fs::write(manual.join("guide.md"), DOC).unwrap();
    assert_eq!(worker.session_start(&manual).unwrap(), Some(manual.clone()));
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn broken_settings_leave_an_error_and_do_not_stop_the_worker() {
    let env = Env::new();
    let bad = env.project("bad", &[("a.md", DOC)]);
    let good = env.project("good", &[("a.md", DOC)]);
    fs::create_dir_all(bad.join(".bars")).unwrap();
    fs::write(bad.join(".bars/config.toml"), "docs_root = [").unwrap();
    let worker = env.start();

    worker.register(&bad).unwrap();
    worker.register(&good).unwrap();
    idle(&worker).await;

    let status = worker.status(&bad).unwrap();
    assert_eq!(status.state, ProjectState::Ready);
    assert!(status.last_error.unwrap().contains("config.toml"));
    assert_eq!(documents(&good), 1);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn model_that_cannot_load_is_an_error_not_a_crash() {
    let env = Env::new();
    let provider = Arc::new(FakeProvider {
        fail_load: true,
        ..FakeProvider::plain("m1")
    });
    let project = env.project("alpha", &[("a.md", DOC)]);
    let worker = Worker::start(env.config(), provider).unwrap();
    worker.register(&project).unwrap();
    idle(&worker).await;

    let status = worker.status(&project).unwrap();
    assert_eq!(status.state, ProjectState::Ready);
    assert!(status.last_error.unwrap().contains("модель не загрузилась"));
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn broken_registry_stops_the_start_instead_of_being_replaced() {
    let env = Env::new();
    fs::write(env.home.path().join("projects.toml"), "[[project").unwrap();
    assert!(matches!(
        Worker::start(env.config(), env.provider.clone()),
        Err(WorkerError::Registry(_))
    ));
    assert_eq!(
        fs::read_to_string(env.home.path().join("projects.toml")).unwrap(),
        "[[project"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stale_rebuild_file_is_removed_at_start() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC)]);
    fs::create_dir_all(project.join(".bars")).unwrap();
    let stale = project.join(".bars/index.sqlite.new");
    fs::write(&stale, "обрывок").unwrap();
    {
        let mut registry = Registry::load(env.home.path()).unwrap();
        registry.add(&project).unwrap();
    }
    let worker = env.start();
    idle(&worker).await;
    assert!(!stale.exists());
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interrupted_swap_returns_the_old_index_at_start() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC)]);
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    worker.shutdown().await;
    // Подмена оборвалась: нового файла на месте нет, прежний лежит как `.old`.
    let index = index_path(&project);
    let old = project.join(".bars/index.sqlite.old");
    fs::rename(&index, &old).unwrap();
    let partial = project.join(".bars/index.sqlite.new");
    fs::write(&partial, "обрывок").unwrap();

    let worker = env.start();
    idle(&worker).await;
    assert!(index.exists() && !old.exists() && !partial.exists());
    assert_eq!(documents(&project), 1);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn job_restores_the_old_index_left_by_a_failed_rollback() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC)]);
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    let index = index_path(&project);
    let old = project.join(".bars/index.sqlite.old");
    fs::rename(&index, &old).unwrap();

    worker
        .check_files(&project, vec![project.join("docs/a.md")])
        .unwrap();
    idle(&worker).await;
    assert!(index.exists() && !old.exists());
    assert_eq!(documents(&project), 1);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn periodic_check_does_not_touch_the_index_files() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC)]);
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    worker.pause();
    // Файл идущей полной переиндексации: проверка из таймера его не трогает (ISS-67).
    let building = project.join(".bars/index.sqlite.new");
    fs::write(&building, "строится").unwrap();
    worker.check_all();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(building.exists());
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pause_freezes_the_queue_until_resume() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC)]);
    let worker = env.start();
    worker.pause();
    assert!(worker.is_paused());

    worker.register(&project).unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(!worker.is_idle(), "на паузе проверка не берётся");
    assert_eq!(worker.queued().len(), 1);
    assert!(worker.progress().is_none());
    assert_eq!(documents_or_zero(&project), 0);

    worker.resume();
    idle(&worker).await;
    assert!(!worker.is_paused());
    assert_eq!(documents(&project), 1);
    assert!(worker.queued().is_empty());
    worker.shutdown().await;
}

fn documents_or_zero(project: &Path) -> usize {
    if index_path(project).exists() {
        documents(project)
    } else {
        0
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_takes_a_waiting_project_off_the_queue() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC)]);
    let worker = env.start();
    worker.pause();
    worker.register(&project).unwrap();

    assert!(worker.cancel(&project).unwrap());
    assert!(
        !worker.cancel(&project).unwrap(),
        "второй раз снимать нечего"
    );
    assert!(worker.queued().is_empty());
    assert_eq!(state(&worker, &project), ProjectState::Ready);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_jobs_report_kind_and_priority() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC)]);
    let worker = env.start();
    worker.pause();
    worker.register(&project).unwrap();
    wait_until(
        "проверка обнаружила документы",
        || !worker.queued().is_empty(),
    )
    .await;

    let queued = worker.queued();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].kind, JobKind::Check);
    assert_eq!(queued[0].priority, Priority::Explicit);
    worker.resume();
    idle(&worker).await;
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_does_not_hang_on_a_paused_queue() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC)]);
    let worker = env.start();
    worker.pause();
    worker.register(&project).unwrap();
    tokio::time::timeout(Duration::from_secs(10), worker.shutdown())
        .await
        .expect("воркер на паузе должен завершаться");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn summary_follows_the_queue_and_the_model() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC)]);
    let worker = env.start();
    assert_eq!(worker.summary().health, Health::Idle);
    assert_eq!(worker.summary().tooltip, "bars");

    worker.pause();
    worker.register(&project).unwrap();
    wait_until(
        "проверка обнаружила документы",
        || !worker.queued().is_empty(),
    )
    .await;
    let busy = worker.summary();
    assert_eq!(busy.health, Health::Busy);
    assert!(busy.indexing.unwrap().text.contains("alpha"));
    assert!(busy.tooltip.contains("в очереди"));
    assert_eq!(busy.projects, 1);

    worker.resume();
    idle(&worker).await;
    assert_eq!(worker.summary().health, Health::Idle);

    // Другая модель в настройках: проверка переводит проект в «нужна переиндексация».
    env.provider.set_model("m2");
    worker.check_all();
    wait_until("нужна переиндексация", || {
        state(&worker, &project) == ProjectState::ReindexRequired
    })
    .await;
    let attention = worker.summary();
    assert_eq!(attention.health, Health::Attention);
    assert!(
        attention
            .attention
            .unwrap()
            .text
            .contains("Нужна переиндексация: alpha")
    );
    assert!(attention.tooltip.contains("нужно внимание"));
    worker.shutdown().await;
}
