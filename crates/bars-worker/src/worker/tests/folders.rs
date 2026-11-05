use super::*;

/// Изолированные данные для ручной проверки Tauri, без загрузки моделей.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "только по явному BARS_FOLDER_FIXTURE в новом временном каталоге"]
async fn folder_native_fixture() {
    let root = PathBuf::from(std::env::var_os("BARS_FOLDER_FIXTURE").expect("BARS_FOLDER_FIXTURE"));
    assert!(!root.exists(), "нужен новый каталог");
    let home = root.join("profile/.bars");
    let project = root.join("Проект с пробелами");
    fs::create_dir_all(project.join("manual/docs/модуль/Папка с пробелами")).unwrap();
    fs::create_dir_all(project.join(".bars")).unwrap();
    fs::write(
        project.join(".bars/config.toml"),
        "docs_root = \"manual/docs\"\n",
    )
    .unwrap();
    for name in [
        "Index.md",
        "модуль/Index.md",
        "модуль/Папка с пробелами/Тема.md",
    ] {
        fs::write(project.join("manual/docs").join(name), DOC).unwrap();
    }
    let worker = Worker::start(
        WorkerConfig::new(home).without_graph_pause(),
        FakeProvider::new("m1"),
    )
    .unwrap();
    worker.register(&project).unwrap();
    idle(&worker).await;
    assert_eq!(documents(&project), 3);
    worker.shutdown().await;
    println!("Профиль: {}", root.join("profile").display());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn folder_identity_changes_on_docs_root_roundtrip_and_reregistration() {
    let env = Env::new();
    let project = env.project("folder-identity", &[("a.md", DOC)]);
    let worker = env.start();
    worker.pause();
    worker.register(&project).unwrap();
    let first = worker.folder_identity(&project).unwrap();
    assert_eq!(worker.folder_identity(&project).unwrap(), first);
    fs::create_dir_all(project.join("manual/docs")).unwrap();
    worker.set_docs_root(&project, "manual/docs").unwrap();
    let second = worker.folder_identity(&project).unwrap();
    assert_ne!(first, second);
    worker.set_docs_root(&project, "docs").unwrap();
    assert_ne!(worker.folder_identity(&project).unwrap(), first);
    let before_delete = worker.folder_identity(&project).unwrap();
    worker.delete_project("folder-identity").await.unwrap();
    assert!(worker.folder_identity(&project).is_err());
    worker.register(&project).unwrap();
    assert_ne!(worker.folder_identity(&project).unwrap(), before_delete);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn folder_action_serializes_settings_and_keeps_full_explicit_receipt() {
    let env = Env::new();
    let project = env.project("folder-action", &[("module/a.md", DOC)]);
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    worker.pause();
    let guard = worker.folder_action_guard().await;
    assert!(worker.set_docs_root(&project, ".").is_err());
    let (id, _, kind) = worker.reindex_receipt(&project, false, false).unwrap();
    assert!(id > 0);
    assert_eq!(kind, JobKind::Check);
    let queued = worker.queued();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].priority, Priority::Explicit);
    assert_eq!(queued[0].kind, JobKind::Check);
    drop(guard);
    env.provider.set_model("m2");
    // Согласие на новую модель проверяется обычным путём очереди.
    worker.resume();
    worker.reindex(&project, false, false).unwrap();
    idle(&worker).await;
    assert!(matches!(
        worker.reindex_receipt(&project, false, false),
        Err(WorkerError::ConsentRequired(_))
    ));
    worker.shutdown().await;
}
