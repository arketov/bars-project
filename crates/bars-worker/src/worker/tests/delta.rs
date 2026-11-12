//! Дельта worktree в воркере: ленивое первое построение, пересверка путей, уборка и удаление
//! (`docs/features/projects/worktrees.md`, Т-48, шаг 6.3.2).

use std::collections::BTreeSet;
use std::sync::atomic::Ordering;

use bars_core::indexing::worktree_delta as delta;
use bars_protocol::DeltaState;

use super::*;

const OTHER: &str = "# Другой документ\n\nРаздел про слежение за файлами.\n";

/// Владелец с `.git`-каталогом и worktree `feat-x`, где `docs/` — копия документов владельца.
fn repo_with_worktree(env: &Env) -> (PathBuf, PathBuf) {
    let main = env.project("main", &[("a.md", DOC), ("b.md", OTHER)]);
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

fn delta_state(worker: &Worker, main: &Path) -> DeltaState {
    worker.worktree_delta_status(main, "feat-x").state
}

fn delta_file(main: &Path) -> PathBuf {
    delta::delta_path(main, "feat-x")
}

/// Дельта обработана воркером до конца: готова и очередь пуста.
async fn built(worker: &Worker, main: &Path) {
    wait_until("дельта готова", || {
        delta_state(worker, main) == DeltaState::Ready && worker.is_idle()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_session_in_worktree_builds_delta_without_embedding() {
    let env = Env::new();
    let (main, work) = repo_with_worktree(&env);
    let worker = env.start();
    worker.register(&main).unwrap();
    idle(&worker).await;
    // Обнаружение дельту не создаёт: ей нужно обращение.
    assert_eq!(delta_state(&worker, &main), DeltaState::None);
    assert!(!delta_file(&main).exists());
    let embedded = env.provider.counters.texts.load(Ordering::SeqCst);

    // Хук сессии в W — первое обращение.
    assert_eq!(
        worker.session_start(&work.join("docs")).unwrap(),
        Some(main.clone())
    );
    built(&worker, &main).await;

    let status = worker.worktree_delta_status(&main, "feat-x");
    assert_eq!(
        status.documents, 0,
        "вид без правок: документов в дельте нет"
    );
    assert!(delta_file(&main).is_file());
    let index = Index::open_read_only(&delta_file(&main)).unwrap();
    assert!(delta::is_ready(&index).unwrap());
    assert_eq!(delta::view_files(&index).unwrap().len(), 2);
    assert_eq!(
        env.provider.counters.texts.load(Ordering::SeqCst),
        embedded,
        "вид без правок модель не вызывает"
    );
    drop(index);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn edit_in_worktree_lands_in_delta_after_resync_of_that_path() {
    let env = Env::new();
    let (main, work) = repo_with_worktree(&env);
    let worker = env.start();
    worker.register(&main).unwrap();
    idle(&worker).await;
    assert!(worker.ensure_worktree_delta(&main, "feat-x"));
    built(&worker, &main).await;

    fs::write(
        work.join("docs/a.md"),
        "# Заголовок\n\nНовый текст раздела про worktree.\n",
    )
    .unwrap();
    let paths: BTreeSet<String> = ["docs/a.md".to_string()].into();
    assert!(worker.resync_worktree_paths(&main, "feat-x", Priority::Edit, paths));
    wait_until("документ в дельте", || {
        worker.worktree_delta_status(&main, "feat-x").documents == 1 && worker.is_idle()
    })
    .await;
    assert_eq!(delta_state(&worker, &main), DeltaState::Ready);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn removed_worktree_deletes_delta_on_next_check() {
    let env = Env::new();
    let (main, work) = repo_with_worktree(&env);
    let worker = env.start();
    worker.register(&main).unwrap();
    idle(&worker).await;
    worker.ensure_worktree_delta(&main, "feat-x");
    built(&worker, &main).await;
    assert!(delta_file(&main).is_file());

    // `git worktree remove` и удаление папки вручную: запись пропала.
    fs::remove_dir_all(main.join(".git").join("worktrees").join("feat-x")).unwrap();
    fs::remove_dir_all(&work).unwrap();
    worker.rediscover_worktrees(&main);
    check_deltas(&worker).await;

    assert!(!delta::delta_dir(&main, "feat-x").exists());
    assert_eq!(delta_state(&worker, &main), DeltaState::None);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn detached_worktree_deletes_delta_on_next_check() {
    let env = Env::new();
    let (main, work) = repo_with_worktree(&env);
    let worker = env.start();
    worker.register(&main).unwrap();
    idle(&worker).await;
    worker.ensure_worktree_delta(&main, "feat-x");
    built(&worker, &main).await;

    // Репозиторий переехал: `.git` рабочей копии ведёт не в ту запись (оторванный).
    fs::write(work.join(".git"), "gitdir: C:/нет/такой/записи\n").unwrap();
    worker.rediscover_worktrees(&main);
    check_deltas(&worker).await;
    assert!(!delta::delta_dir(&main, "feat-x").exists());
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn model_change_deletes_deltas() {
    let env = Env::new();
    let (main, _work) = repo_with_worktree(&env);
    let worker = env.start();
    worker.register(&main).unwrap();
    idle(&worker).await;
    worker.ensure_worktree_delta(&main, "feat-x");
    built(&worker, &main).await;

    // Смена модели: владелец ждёт переиндексации, дельта ему не нужна и заданий не получает.
    env.provider.set_model("m2");
    worker
        .check_files(&main, vec![main.join("docs/a.md")])
        .unwrap();
    idle(&worker).await;
    assert_eq!(state(&worker, &main), ProjectState::ReindexRequired);
    assert!(!worker.resync_worktree_paths(&main, "feat-x", Priority::Edit, BTreeSet::new()));
    check_deltas(&worker).await;
    assert!(
        delta_file(&main).is_file(),
        "до переиндексации дельту не трогают"
    );

    // Владелец переиндексирован новой моделью: отпечаток дельты не совпал — дельта удалена.
    worker.reindex(&main, true, true).unwrap();
    idle(&worker).await;
    assert_eq!(state(&worker, &main), ProjectState::Ready);
    check_deltas(&worker).await;
    assert!(!delta::delta_dir(&main, "feat-x").exists());
    assert_eq!(delta_state(&worker, &main), DeltaState::None);

    // Следующее обращение строит дельту заново.
    worker.ensure_worktree_delta(&main, "feat-x");
    built(&worker, &main).await;
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chunking_or_docs_root_change_deletes_deltas() {
    let env = Env::new();
    let (main, _work) = repo_with_worktree(&env);
    fs::create_dir_all(main.join("manual")).unwrap();
    fs::write(main.join("manual/m.md"), DOC).unwrap();
    let worker = env.start();
    worker.register(&main).unwrap();
    idle(&worker).await;
    worker.ensure_worktree_delta(&main, "feat-x");
    built(&worker, &main).await;

    worker.set_docs_root(&main, "manual").unwrap();
    idle(&worker).await;
    check_deltas(&worker).await;
    assert!(!delta::delta_dir(&main, "feat-x").exists());
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hourly_check_restores_ready_from_file_and_queues_nothing() {
    let env = Env::new();
    let (main, _work) = repo_with_worktree(&env);
    let worker = env.start();
    worker.register(&main).unwrap();
    idle(&worker).await;
    worker.ensure_worktree_delta(&main, "feat-x");
    built(&worker, &main).await;
    worker.shutdown().await;

    // Новый воркер: состояния в памяти пропали, «готова» восстанавливается из файла.
    let worker = env.start();
    idle(&worker).await;
    wait_until("готова из файла", || {
        delta_state(&worker, &main) == DeltaState::Ready
    })
    .await;
    // Владелец не менялся: подпись совпала, заданий не поставлено.
    assert!(worker.is_idle());
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn project_deletion_removes_the_delta_folder() {
    let env = Env::new();
    let (main, _work) = repo_with_worktree(&env);
    let worker = env.start();
    worker.register(&main).unwrap();
    idle(&worker).await;
    worker.ensure_worktree_delta(&main, "feat-x");
    built(&worker, &main).await;
    assert!(delta_file(&main).is_file());

    worker
        .delete_project(&main.display().to_string())
        .await
        .unwrap();
    assert!(!main.join(".bars").join("worktrees").exists());
    worker.shutdown().await;
}

/// Часовая проверка и проверка старта: блокирующее тело, как в таймере воркера.
async fn check_deltas(worker: &Worker) {
    let inner = worker.inner.clone();
    tokio::task::spawn_blocking(move || inner.check_deltas())
        .await
        .unwrap();
}
