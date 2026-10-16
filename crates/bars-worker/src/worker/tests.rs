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
async fn registered_project_is_indexed_and_watched() {
    let env = Env::new();
    let project = env.project("alpha", &[("a.md", DOC), ("mod/b.md", DOC)]);
    let worker = env.start();

    let registered = worker.register(&project).unwrap();
    assert!(!registered.already);
    idle(&worker).await;

    let status = worker.status(&project).unwrap();
    assert_eq!(status.state, ProjectState::Ready);
    assert!(status.updated_at.is_some());
    assert!(status.watching);
    assert_eq!(status.last_error, None);
    assert_eq!(status.model_index, status.model_settings);
    assert_eq!(status.model_index.as_deref(), Some("builtin:m1"));
    assert_eq!(status.last_report.unwrap().added, 2);
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
    // Корня документации нет: слежение не стартует, первая проверка кончается ошибкой.
    let root = env.projects.path().join("late");
    fs::create_dir_all(&root).unwrap();
    let mut config = env.config();
    config.periodic_interval = Duration::from_millis(200);
    let worker = Worker::start(config, env.provider.clone()).unwrap();
    worker.register(&root).unwrap();
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
    idle(&worker).await;
    assert_eq!(documents(&project), 1);

    // Слежение «оглохло»: оно живо, но событий не передаёт.
    let deaf =
        ProjectWatcher::start(&project, "docs", Duration::from_millis(100), Arc::new(|_| {}))
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
    wait_until("слежение снова видит правки", || {
        worker.is_idle() && documents(&project) == 3
    })
    .await;
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
    assert!(status.watching);
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
