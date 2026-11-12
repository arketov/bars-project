//! Актуальность дельты worktree: слежение за W, пауза Т-41, изменения владельца, перезапуск и
//! отмена проекта (`docs/features/projects/worktrees.md`, Т-10, Т-41, Т-48, шаг 6.3.4).

use bars_core::indexing::worktree_delta::{self as delta, OwnerSnapshot};
use bars_protocol::DeltaState;

use super::super::doc_queue::DocTarget;
use super::*;

const EDITED: &str = "# Заголовок\n\nНовый текст раздела про worktree.\n";

/// Владелец с `.git`-каталогом и worktree `feat-x` (как в `delta.rs`).
fn repo_with_worktree(env: &Env) -> (PathBuf, PathBuf) {
    let main = env.project(
        "main",
        &[
            ("a.md", DOC),
            ("b.md", "# Другой\n\nРаздел про слежение.\n"),
        ],
    );
    fs::create_dir_all(main.join(".git")).unwrap();
    let work = env.projects.path().join("runs").join("feat-x");
    let admin = main.join(".git").join("worktrees").join("feat-x");
    fs::create_dir_all(&admin).unwrap();
    fs::create_dir_all(work.join("docs")).unwrap();
    fs::write(admin.join("HEAD"), "ref: refs/heads/feat/x\n").unwrap();
    fs::write(admin.join("commondir"), "../..\n").unwrap();
    fs::write(
        admin.join("gitdir"),
        work.join(".git").to_string_lossy().replace('\\', "/"),
    )
    .unwrap();
    fs::write(
        work.join(".git"),
        format!("gitdir: {}\n", admin.to_string_lossy().replace('\\', "/")),
    )
    .unwrap();
    for file in ["a.md", "b.md"] {
        fs::copy(main.join("docs").join(file), work.join("docs").join(file)).unwrap();
    }
    (main, work)
}

fn started(env: &Env, pause: Duration) -> Worker {
    let mut config = env.config();
    config.edit_pause = Some(pause);
    // Хвостовая полная проверка не должна вмешиваться в замеры.
    config.tail_delay = Duration::from_secs(60);
    Worker::start(config, env.provider.clone()).unwrap()
}

fn delta_state(worker: &Worker, main: &Path) -> DeltaState {
    worker.worktree_delta_status(main, "feat-x").state
}

fn delta_documents(worker: &Worker, main: &Path) -> u32 {
    worker.worktree_delta_status(main, "feat-x").documents
}

/// Дельта построена и очередь пуста.
async fn built(worker: &Worker, main: &Path) {
    assert!(worker.ensure_worktree_delta(main, "feat-x"));
    wait_until("дельта готова", || {
        delta_state(worker, main) == DeltaState::Ready && worker.is_idle()
    })
    .await;
}

/// Готовая дельта и слежение за W (аренда MCP).
async fn watched(worker: &Worker, main: &Path) {
    built(worker, main).await;
    worker.observe_worktree_mcp(main, "feat-x");
    assert!(
        lock(&worker.inner.wt_watch)
            .values()
            .any(|watch| watch.has_watcher()),
        "слежение за корнем вида поднято"
    );
}

fn target(main: &Path) -> DocTarget {
    DocTarget::worktree(main, "feat-x")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn edit_in_worktree_is_found_after_pause() {
    let env = Env::new();
    let (main, work) = repo_with_worktree(&env);
    let worker = started(&env, Duration::from_secs(2));
    worker.register(&main).unwrap();
    idle(&worker).await;
    watched(&worker, &main).await;
    assert_eq!(delta_documents(&worker, &main), 0);

    fs::write(work.join("docs/a.md"), EDITED).unwrap();
    wait_until("запись ждёт паузы", || {
        lock(&worker.inner.docs).counts(target(&main)).waiting == 1
    })
    .await;
    assert_eq!(
        delta_documents(&worker, &main),
        0,
        "пока идёт пауза, дельта не меняется"
    );

    // Пауза прошла: сверка пути, документ в дельте, очередь документов пуста.
    wait_until("документ в дельте", || {
        delta_documents(&worker, &main) == 1 && worker.is_idle()
    })
    .await;
    assert_eq!(lock(&worker.inner.docs).counts(target(&main)).total(), 0);
    assert_eq!(delta_state(&worker, &main), DeltaState::Ready);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watch_events_are_dropped_while_delta_state_is_none() {
    let env = Env::new();
    let (main, work) = repo_with_worktree(&env);
    let worker = started(&env, Duration::ZERO);
    worker.register(&main).unwrap();
    idle(&worker).await;
    // Аренда есть, дельты нет: слежение не поднимается, правка ничего не создаёт.
    worker.observe_worktree_mcp(&main, "feat-x");
    assert!(
        lock(&worker.inner.wt_watch)
            .values()
            .all(|watch| !watch.has_watcher())
    );
    fs::write(work.join("docs/a.md"), EDITED).unwrap();
    worker.edit_worktree_files(&main, "feat-x", vec![work.join("docs/a.md")]);
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(delta_state(&worker, &main), DeltaState::None);
    assert!(!delta::delta_dir(&main, "feat-x").exists());
    assert_eq!(lock(&worker.inner.docs).counts(target(&main)).total(), 0);
    assert!(worker.is_idle());
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn edit_from_the_window_resyncs_the_path() {
    let env = Env::new();
    let (main, work) = repo_with_worktree(&env);
    let worker = started(&env, Duration::ZERO);
    worker.register(&main).unwrap();
    idle(&worker).await;
    built(&worker, &main).await;
    fs::write(work.join("docs/a.md"), EDITED).unwrap();
    worker.edit_worktree_files(&main, "feat-x", vec![work.join("docs/a.md")]);
    wait_until("документ в дельте", || {
        delta_documents(&worker, &main) == 1 && worker.is_idle()
    })
    .await;
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owner_edit_missing_in_worktree_does_not_change_view() {
    let env = Env::new();
    let (main, work) = repo_with_worktree(&env);
    let worker = started(&env, Duration::ZERO);
    worker.register(&main).unwrap();
    idle(&worker).await;
    watched(&worker, &main).await;

    // Владелец правит документ, которого в W не правили: вид хранит старую версию, документ
    // теперь отличается от владельца и попадает в дельту.
    fs::write(main.join("docs/a.md"), EDITED).unwrap();
    worker
        .check_files(&main, vec![main.join("docs/a.md")])
        .unwrap();
    wait_until(
        "документ вида попал в дельту",
        || delta_documents(&worker, &main) == 1 && worker.is_idle(),
    )
    .await;
    assert_eq!(
        fs::read_to_string(work.join("docs/a.md")).unwrap(),
        DOC,
        "файл в W не менялся: вид прежний"
    );

    // Вид и владелец сошлись: документ уходит из дельты (слежение за W).
    fs::write(work.join("docs/a.md"), EDITED).unwrap();
    wait_until("документ ушёл из дельты", || {
        delta_documents(&worker, &main) == 0 && worker.is_idle()
    })
    .await;
    assert_eq!(delta_state(&worker, &main), DeltaState::Ready);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owner_edit_without_lease_resyncs_the_delta_with_periodic_priority() {
    let env = Env::new();
    let (main, _work) = repo_with_worktree(&env);
    let worker = started(&env, Duration::ZERO);
    worker.register(&main).unwrap();
    idle(&worker).await;
    built(&worker, &main).await;
    let key = (main.clone(), "feat-x".to_string());
    assert!(!worker.inner.wt_observed(&key, Instant::now()));

    fs::write(main.join("docs/a.md"), EDITED).unwrap();
    worker
        .check_files(&main, vec![main.join("docs/a.md")])
        .unwrap();
    wait_until(
        "дельта пересверена по владельцу",
        || delta_documents(&worker, &main) == 1 && worker.is_idle(),
    )
    .await;
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restart_with_changed_owner_digest_resyncs_the_delta() {
    let env = Env::new();
    let (main, _work) = repo_with_worktree(&env);
    let worker = env.start();
    worker.register(&main).unwrap();
    idle(&worker).await;
    built(&worker, &main).await;
    worker.shutdown().await;
    let stored = |main: &Path| {
        let index = Index::open_read_only(&delta::delta_path(main, "feat-x")).unwrap();
        delta::stored_sync(&index).unwrap().0
    };
    let before = stored(&main);

    // Пока воркер стоял, владелец получил новый документ: подпись владельца изменилась.
    fs::write(main.join("docs/c.md"), "# Новый\n\nДокумент владельца.\n").unwrap();
    let worker = env.start();
    idle(&worker).await;
    check_deltas_now(&worker).await;
    let owner_digest = || {
        let index = Index::open_read_only(&index_path(&main)).unwrap();
        OwnerSnapshot::read(&index).unwrap().digest
    };
    wait_until(
        "подпись владельца записана в дельту",
        || stored(&main).as_deref() == Some(owner_digest().as_str()) && worker.is_idle(),
    )
    .await;
    assert_ne!(stored(&main), before);
    assert_eq!(delta_state(&worker, &main), DeltaState::Ready);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_the_project_stops_worktree_watching_and_its_queued_edits() {
    let env = Env::new();
    let (main, work) = repo_with_worktree(&env);
    let worker = started(&env, Duration::from_secs(60));
    worker.register(&main).unwrap();
    idle(&worker).await;
    watched(&worker, &main).await;
    fs::write(work.join("docs/a.md"), EDITED).unwrap();
    wait_until("запись ждёт паузы", || {
        lock(&worker.inner.docs).counts(target(&main)).waiting == 1
    })
    .await;

    worker.cancel(&main).unwrap();
    assert_eq!(lock(&worker.inner.docs).counts(target(&main)).total(), 0);
    assert!(lock(&worker.inner.wt_watch).is_empty());
    assert_eq!(delta_state(&worker, &main), DeltaState::None);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn window_lease_is_released_when_the_window_leaves_the_worktree() {
    let env = Env::new();
    let (main, _work) = repo_with_worktree(&env);
    let worker = started(&env, Duration::ZERO);
    worker.register(&main).unwrap();
    idle(&worker).await;
    built(&worker, &main).await;
    worker.observe_worktree_window("w1", Some((&main, "feat-x")));
    let key = (main.clone(), "feat-x".to_string());
    assert!(worker.inner.wt_observed(&key, Instant::now()));
    worker.observe_worktree_window("w1", None);
    assert!(!worker.inner.wt_observed(&key, Instant::now()));
    assert!(lock(&worker.inner.wt_watch).is_empty());
    worker.shutdown().await;
}

/// Проверка старта и часовая: блокирующее тело, как в таймере воркера.
async fn check_deltas_now(worker: &Worker) {
    let inner = worker.inner.clone();
    tokio::task::spawn_blocking(move || inner.check_deltas())
        .await
        .unwrap();
}
