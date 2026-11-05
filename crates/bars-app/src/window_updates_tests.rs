use super::*;
use bars_worker::worker::{Worker, WorkerConfig, models::fake::FakeProvider};

async fn settle(handler: &WorkerHandler) {
    for _ in 0..1000 {
        if handler.worker().is_idle() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("Воркер не завершил начальную проверку");
}

async fn fixture() -> (tempfile::TempDir, WorkerHandler, Coordinator, String) {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir_all(project.join("docs/module.v2")).unwrap();
    std::fs::write(project.join("docs/module.v2/a.md"), "# Альфа\n\nТекст.").unwrap();
    let worker = Worker::start(
        WorkerConfig::new(temp.path().join("home")).without_graph_pause(),
        FakeProvider::new("snapshots"),
    )
    .unwrap();
    let registered = worker.register(&project).unwrap().path;
    for _ in 0..500 {
        if worker.is_idle() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let handler = WorkerHandler::new(worker);
    let coordinator = Coordinator::default();
    (temp, handler, coordinator, registered.display().to_string())
}

#[tokio::test]
async fn concurrent_windows_share_parts_and_unchanged_reads_do_no_io() {
    let (_temp, handler, coordinator, project) = fixture().await;
    let tokens: Vec<_> = (0..4)
        .map(|i| {
            coordinator
                .observe(&handler, &format!("w{i}"), &project, ALL)
                .unwrap()["subscription"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    settle(&handler).await;
    let (a, b, c, d) = tokio::join!(
        coordinator.snapshot(&handler, "w0", &tokens[0], Value::Null, false),
        coordinator.snapshot(&handler, "w1", &tokens[1], Value::Null, false),
        coordinator.snapshot(&handler, "w2", &tokens[2], Value::Null, false),
        coordinator.snapshot(&handler, "w3", &tokens[3], Value::Null, false)
    );
    let a = a.unwrap();
    for snapshot in [b.unwrap(), c.unwrap(), d.unwrap()] {
        assert_eq!(a["revisions"], snapshot["revisions"]);
    }
    assert_eq!(
        coordinator
            .0
            .builds
            .each_ref()
            .map(|v| v.load(Ordering::SeqCst)),
        [1, 1, 1]
    );
    let unchanged = coordinator
        .snapshot(
            &handler,
            "w0",
            &tokens[0],
            json!({"epoch":a["epoch"],"revisions":a["revisions"]}),
            false,
        )
        .await
        .unwrap();
    assert_eq!(unchanged["parts"]["disk"]["unchanged"], true);
    assert_eq!(coordinator.0.builds[0].load(Ordering::SeqCst), 1);
    assert!(
        coordinator
            .snapshot(&handler, "w1", &tokens[0], Value::Null, false)
            .await
            .is_err()
    );
    handler.worker().shutdown().await;
}

#[tokio::test]
async fn eviction_preserves_revisions_and_new_readers_get_full_parts() {
    let (_temp, handler, coordinator, project) = fixture().await;
    let binding = coordinator
        .observe(&handler, "main", &project, ALL)
        .unwrap();
    let token = binding["subscription"].as_str().unwrap();
    settle(&handler).await;
    let first = coordinator
        .snapshot(&handler, "main", token, Value::Null, false)
        .await
        .unwrap();
    let entry = coordinator.subscription("main", token).unwrap().entry;
    *lock(&entry.cache) = None;
    let same = coordinator
        .snapshot(
            &handler,
            "main",
            token,
            json!({"epoch":first["epoch"],"revisions":first["revisions"]}),
            false,
        )
        .await
        .unwrap();
    assert_eq!(same["revisions"], first["revisions"]);
    assert_eq!(coordinator.0.builds[0].load(Ordering::SeqCst), 1);
    let full = coordinator
        .snapshot(&handler, "main", token, Value::Null, false)
        .await
        .unwrap();
    assert_eq!(full["revisions"], first["revisions"]);
    assert_eq!(
        full["parts"]["disk"]["value"]["files"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    coordinator.release("main");
    assert!(lock(&coordinator.0.state).entries.is_empty());
    assert!(
        coordinator
            .snapshot(&handler, "main", token, Value::Null, false)
            .await
            .is_err()
    );
    handler.worker().shutdown().await;
}

#[tokio::test]
async fn disk_events_empty_dotted_folders_errors_and_registration_identity() {
    let (_temp, handler, coordinator, project) = fixture().await;
    let binding = coordinator.observe(&handler, "doc", &project, 3).unwrap();
    let token = binding["subscription"].as_str().unwrap();
    settle(&handler).await;
    let first = coordinator
        .snapshot(&handler, "doc", token, Value::Null, false)
        .await
        .unwrap();
    assert!(first["parts"].get("status").is_none());
    let path = PathBuf::from(&project);
    std::fs::create_dir(path.join("docs/empty.v3")).unwrap();
    handler.worker().window_updates().changed(&path, 1, false);
    let added = coordinator
        .snapshot(&handler, "doc", token, Value::Null, false)
        .await
        .unwrap();
    assert!(
        added["parts"]["disk"]["value"]["dirs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "docs/empty.v3")
    );
    std::fs::rename(path.join("docs"), path.join("saved-docs")).unwrap();
    let error = coordinator
        .snapshot(&handler, "doc", token, Value::Null, true)
        .await
        .unwrap();
    assert_eq!(error["parts"]["disk"]["state"], "error");
    assert!(error["parts"]["disk"]["value"]["files"].is_array());
    std::fs::rename(path.join("saved-docs"), path.join("docs")).unwrap();
    let restored = coordinator
        .snapshot(&handler, "doc", token, Value::Null, true)
        .await
        .unwrap();
    assert_eq!(restored["parts"]["disk"]["state"], "ready");
    handler.worker().set_docs_root(&path, "saved-docs").unwrap();
    let replaced = coordinator
        .snapshot(&handler, "doc", token, Value::Null, true)
        .await
        .unwrap();
    assert_ne!(replaced["epoch"], first["epoch"]);
    assert_ne!(replaced["identity"], first["identity"]);
    handler.worker().shutdown().await;
}

#[tokio::test]
async fn events_during_io_remain_dirty_and_reset_rejects_old_tokens() {
    let (_temp, handler, coordinator, project) = fixture().await;
    let binding = coordinator
        .observe(&handler, "main", &project, ALL)
        .unwrap();
    let token = binding["subscription"].as_str().unwrap();
    settle(&handler).await;
    let entry = coordinator.subscription("main", token).unwrap().entry;
    let gate = handler
        .worker()
        .gate(&PathBuf::from(&project))
        .unwrap()
        .write_owned()
        .await;
    let task = {
        let c = coordinator.clone();
        let h = handler.clone();
        let token = token.to_owned();
        tokio::spawn(async move { c.snapshot(&h, "main", &token, Value::Null, false).await })
    };
    for _ in 0..500 {
        if coordinator.0.builds[1].load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(coordinator.0.builds[1].load(Ordering::SeqCst), 1);
    entry.invalidate(1);
    drop(gate);
    let intermediate = task.await.unwrap().unwrap();
    assert_eq!(intermediate["updating"], true);
    let final_snapshot = coordinator
        .snapshot(&handler, "main", token, Value::Null, false)
        .await
        .unwrap();
    assert_eq!(final_snapshot["updating"], false);
    coordinator.reset();
    assert!(
        coordinator
            .snapshot(&handler, "main", token, Value::Null, false)
            .await
            .is_err()
    );
    handler.worker().shutdown().await;
}

#[tokio::test]
async fn watcher_updates_empty_dotted_directory_and_hidden_projects_stop_building() {
    let (_temp, handler, coordinator, project) = fixture().await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let visible = Arc::new(AtomicBool::new(true));
    let seen = visible.clone();
    coordinator.start(
        handler.clone(),
        Arc::new(move |_, value| {
            let _ = tx.send(value);
        }),
        Arc::new(move |_| seen.load(Ordering::SeqCst)),
    );
    let binding = coordinator
        .observe(&handler, "main", &project, ALL)
        .unwrap();
    let token = binding["subscription"].as_str().unwrap();
    settle(&handler).await;
    let first = coordinator
        .snapshot(&handler, "main", token, Value::Null, false)
        .await
        .unwrap();
    while rx.try_recv().is_ok() {}
    std::fs::create_dir(PathBuf::from(&project).join("docs/new.v4")).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = rx.recv().await.unwrap();
            if event["revisions"][0] != first["revisions"][0] {
                break;
            }
        }
    })
    .await
    .expect("Внешняя пустая папка видна в пределах T-01");
    let fresh = coordinator
        .snapshot(&handler, "main", token, Value::Null, false)
        .await
        .unwrap();
    assert!(
        fresh["parts"]["disk"]["value"]["dirs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "docs/new.v4")
    );
    visible.store(false, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let before = coordinator
        .0
        .builds
        .each_ref()
        .map(|v| v.load(Ordering::SeqCst));
    assert!(lock(&coordinator.0.state).entries.is_empty());
    std::fs::create_dir(PathBuf::from(&project).join("docs/hidden.v5")).unwrap();
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert_eq!(
        before,
        coordinator
            .0
            .builds
            .each_ref()
            .map(|v| v.load(Ordering::SeqCst))
    );
    handler.worker().shutdown().await;
}

#[tokio::test]
async fn renamed_dotted_directory_updates_published_index_without_periodic_scan() {
    let (_temp, handler, coordinator, project) = fixture().await;
    coordinator
        .observe(&handler, "main", &project, ALL)
        .unwrap();
    settle(&handler).await;
    let root = PathBuf::from(&project);
    std::fs::rename(root.join("docs/module.v2"), root.join("docs/moved.v3")).unwrap();
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let tree = read_part(&handler, &root, 1).await.unwrap();
            let paths: Vec<_> = tree["documents"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|d| d["path"].as_str())
                .collect();
            if paths.contains(&"docs/moved.v3/a.md") && !paths.contains(&"docs/module.v2/a.md") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("Переименование папки с точкой обновляет индекс по watcher");
    handler.worker().shutdown().await;
}

#[tokio::test]
async fn cache_limits_retention_to_eight_projects_without_removing_subscription_revisions() {
    let (temp, handler, coordinator, project) = fixture().await;
    for i in 0..9 {
        let path = temp.path().join(format!("cache{i}"));
        std::fs::create_dir_all(path.join("docs")).unwrap();
        std::fs::write(path.join("docs/a.md"), "# Документ").unwrap();
        handler.worker().register(&path).unwrap();
        settle(&handler).await;
        let binding = coordinator
            .observe(&handler, &format!("w{i}"), &path.display().to_string(), 3)
            .unwrap();
        settle(&handler).await;
        let snapshot = coordinator
            .snapshot(
                &handler,
                &format!("w{i}"),
                binding["subscription"].as_str().unwrap(),
                Value::Null,
                false,
            )
            .await
            .unwrap();
        assert!(snapshot["parts"]["disk"]["value"]["files"].is_array());
    }
    let entries: Vec<_> = lock(&coordinator.0.state)
        .entries
        .values()
        .cloned()
        .collect();
    assert_eq!(entries.len(), 9);
    assert_eq!(
        entries.iter().filter(|e| lock(&e.cache).is_some()).count(),
        8
    );
    assert!(coordinator.statistics()["cache_bytes"].as_u64().unwrap() <= CACHE_BYTES as u64);
    assert!(!project.is_empty());
    handler.worker().shutdown().await;
}
