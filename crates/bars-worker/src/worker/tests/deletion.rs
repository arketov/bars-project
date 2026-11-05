use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deletion_cleans_project_and_keeps_documents_other_projects_and_shared_settings() {
    let env = Env::new();
    let project = env.project("delete", &[("a.md", DOC)]);
    let other = env.project("keep", &[("b.md", DOC)]);
    let worker = env.start();
    worker.register(&project).unwrap();
    worker.register(&other).unwrap();
    // Файл инструкции кладёт работа агента, а не регистрация (Т-8).
    worker.observe_mcp(&project).unwrap();
    idle(&worker).await;
    worker.pause();
    let instruction = fs::read(project.join("docs/BARS_INSTRUCTION.md")).unwrap();
    fs::write(env.home.path().join("settings.toml"), "# сохранить\n").unwrap();
    fs::create_dir_all(env.home.path().join("models")).unwrap();
    fs::write(env.home.path().join("models/keep"), "model").unwrap();
    for name in [
        "index.sqlite.new",
        "index.sqlite.old",
        "index.sqlite-wal",
        "index.sqlite-shm",
        "config.toml.bars-tmp",
    ] {
        fs::write(project.join(".bars").join(name), "temporary").unwrap();
    }
    worker.reindex(&project, false, false).unwrap();
    worker.reindex(&other, false, false).unwrap();
    worker.delete_project("delete").await.unwrap();
    assert!(!project.join(".bars").exists());
    assert_eq!(fs::read_to_string(project.join("docs/a.md")).unwrap(), DOC);
    assert_eq!(
        fs::read(project.join("docs/BARS_INSTRUCTION.md")).unwrap(),
        instruction
    );
    assert_eq!(
        fs::read_to_string(env.home.path().join("settings.toml")).unwrap(),
        "# сохранить\n"
    );
    assert!(env.home.path().join("models/keep").is_file());
    assert_eq!(documents(&other), 1);
    assert_eq!(
        Registry::load(env.home.path()).unwrap().paths(),
        vec![other.clone()]
    );
    assert_eq!(
        worker
            .queued()
            .iter()
            .map(|j| &j.project)
            .collect::<Vec<_>>(),
        vec![&other]
    );
    worker.check_all();
    assert!(!lock(&worker.inner.projects).contains_key(&project));
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deletion_waits_for_readers_and_does_not_revive_on_watch_tail_or_timer() {
    let env = Env::new();
    let project = env.project("delete", &[("a.md", DOC)]);
    let mut config = env.config();
    config.tail_delay = Duration::from_millis(70);
    let worker = Worker::start(config, env.provider.clone()).unwrap();
    worker.register(&project).unwrap();
    idle(&worker).await;
    let reader = worker.gate(&project).unwrap().read_owned().await;
    worker.inner.arm_tail(&project);
    let deleting = {
        let worker = worker.clone();
        tokio::spawn(async move { worker.delete_project("delete").await })
    };
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(!deleting.is_finished());
    assert!(worker.register(&project).is_err());
    assert!(worker.set_docs_root(&project, "docs").is_err());
    worker.check_all();
    assert!(worker.queued().is_empty());
    drop(reader);
    tokio::time::timeout(Duration::from_secs(5), deleting)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    fs::write(project.join("docs/b.md"), DOC).unwrap();
    worker.inner.on_watch_event(&project, WatchEvent::Rescan);
    worker.check_all();
    tokio::time::sleep(Duration::from_millis(160)).await;
    assert!(worker.queued().is_empty());
    assert!(!project.join(".bars").exists());
    worker.register(&project).unwrap();
    idle(&worker).await;
    assert_eq!(documents(&project), 2);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deletion_cancels_running_job_even_when_queue_is_paused() {
    let env = Env::new();
    let project = env.project("running", &[("a.md", DOC)]);
    let provider = Arc::new(FakeProvider {
        load_delay: Duration::from_millis(300),
        ..FakeProvider::plain("m1")
    });
    let worker = Worker::start(env.config(), provider).unwrap();
    worker.register(&project).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while worker.progress().is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    worker.pause();
    tokio::time::timeout(Duration::from_secs(5), worker.delete_project("running"))
        .await
        .unwrap()
        .unwrap();
    assert!(worker.progress().is_none());
    assert!(!project.join(".bars").exists());
    assert_eq!(fs::read_to_string(project.join("docs/a.md")).unwrap(), DOC);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deletion_keeps_unknown_files_and_removes_missing_project_registration() {
    let env = Env::new();
    let project = env.project("unknown", &[("a.md", DOC)]);
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    fs::write(project.join(".bars/user.md"), DOC).unwrap();
    worker.delete_project("unknown").await.unwrap();
    assert_eq!(
        fs::read_to_string(project.join(".bars/user.md")).unwrap(),
        DOC
    );
    assert!(!project.join(".bars/config.toml").exists());
    worker.register(&project).unwrap();
    idle(&worker).await;
    let moved = env.projects.path().join("moved");
    // Имитируем исчезновение папки; Windows не даёт переименовать открытый watcher-каталог.
    let watcher = lock(&worker.inner.projects)
        .get_mut(&project)
        .unwrap()
        .watcher
        .take();
    drop(watcher);
    fs::rename(&project, &moved).unwrap();
    worker.delete_project("unknown").await.unwrap();
    assert!(moved.join("docs/a.md").is_file());
    assert!(Registry::load(env.home.path()).unwrap().paths().is_empty());
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deletion_rejects_unexpected_file_types_before_removing_config_and_allows_retry() {
    let env = Env::new();
    let project = env.project("retry", &[("a.md", DOC)]);
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    fs::create_dir(project.join(".bars/index.sqlite.old")).unwrap();
    assert!(worker.delete_project("retry").await.is_err());
    assert!(worker.resolve("retry").is_ok());
    assert!(project.join(".bars/config.toml").is_file());
    fs::remove_dir(project.join(".bars/index.sqlite.old")).unwrap();
    worker.delete_project("retry").await.unwrap();
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deletion_rejects_shared_user_home() {
    let env = Env::new();
    let project = env.project("home", &[("a.md", DOC)]);
    let worker = Worker::start(
        WorkerConfig::new(project.join(".bars")).without_graph_pause(),
        env.provider.clone(),
    )
    .unwrap();
    worker.register(&project).unwrap();
    idle(&worker).await;
    assert!(worker.delete_project("home").await.is_err());
    assert!(worker.resolve("home").is_ok());
    assert!(project.join(".bars/projects.toml").is_file());
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deletion_finishes_when_the_calling_task_disconnects() {
    let env = Env::new();
    let project = env.project("disconnect", &[("a.md", DOC)]);
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    let reader = worker.gate(&project).unwrap().read_owned().await;
    let caller = {
        let worker = worker.clone();
        tokio::spawn(async move { worker.delete_project("disconnect").await })
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        while !lock(&worker.inner.projects)
            .get(&project)
            .is_some_and(|r| r.deleting)
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    caller.abort();
    let _ = caller.await;
    drop(reader);
    tokio::time::timeout(Duration::from_secs(5), async {
        while worker.resolve("disconnect").is_ok() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!project.join(".bars").exists());
    worker.shutdown().await;
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deletion_reports_locked_file_and_allows_retry() {
    use std::os::windows::fs::OpenOptionsExt;
    let env = Env::new();
    let project = env.project("locked", &[("a.md", DOC)]);
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    let handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(project.join(".bars/config.toml"))
        .unwrap();
    assert!(worker.delete_project("locked").await.is_err());
    assert!(worker.resolve("locked").is_ok());
    assert_eq!(fs::read_to_string(project.join("docs/a.md")).unwrap(), DOC);
    drop(handle);
    worker.delete_project("locked").await.unwrap();
    worker.shutdown().await;
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deletion_rejects_junction_and_preserves_target() {
    let env = Env::new();
    let project = env.project("junction", &[("a.md", DOC)]);
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    let outside = env.projects.path().join("outside");
    fs::rename(project.join(".bars"), &outside).unwrap();
    let output = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(project.join(".bars"))
        .arg(&outside)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(worker.delete_project("junction").await.is_err());
    assert!(outside.join("config.toml").is_file());
    assert!(outside.join("index.sqlite").is_file());
    fs::remove_dir(project.join(".bars")).unwrap();
    fs::rename(outside, project.join(".bars")).unwrap();
    worker.shutdown().await;
}
