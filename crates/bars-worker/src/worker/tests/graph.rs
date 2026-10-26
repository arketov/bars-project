use super::*;
use bars_core::graph::{LayerState, store};

#[tokio::test(flavor = "multi_thread")]
async fn graph_restart_recovers_interrupted_build_without_reembedding() {
    let env = Env::new();
    let project = env.project(
        "graph-restart",
        &[("a.md", "# Правило\n\nТ-1 зависит от Т-2.")],
    );
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    let snapshot = store::load(index.conn()).unwrap().snapshot;
    store::set_state(
        index.conn(),
        LayerState::Building,
        Some("interrupted-job"),
        None,
    )
    .unwrap();
    drop(index);
    worker.shutdown().await;
    let worker = env.start();
    idle(&worker).await;
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    let status = store::status(index.conn()).unwrap();
    assert_eq!(status.state, LayerState::Ready);
    assert!(status.active_job.is_none());
    assert_eq!(store::load(index.conn()).unwrap().snapshot, snapshot);
    drop(index);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn graph_cancel_queued_build_then_retry_and_rename_remove_old_evidence() {
    let env = Env::new();
    let project = env.project(
        "graph-cancel",
        &[("a.md", "# Правило\n\nТ-1 зависит от Т-2.")],
    );
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    let gate = {
        let projects = lock(&worker.inner.projects);
        projects[&project].job_gate.clone()
    };
    let held = gate.lock_owned().await;
    let accepted = worker.graph_build(&project).await.unwrap();
    assert!(worker.graph_cancel(&project, &accepted.job_id).unwrap());
    drop(held);
    let deadline = Instant::now() + Duration::from_secs(10);
    while worker.active_graph_job(&project).is_some() {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    assert_eq!(
        store::status(index.conn()).unwrap().job_state,
        bars_core::graph::JobState::Stopped
    );
    assert!(store::status(index.conn()).unwrap().paused);
    let old = store::load(index.conn()).unwrap();
    drop(index);
    worker.reindex(&project, false, false).unwrap();
    idle(&worker).await;
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    assert!(store::status(index.conn()).unwrap().paused);
    assert_eq!(
        store::status(index.conn()).unwrap().job_state,
        bars_core::graph::JobState::Stopped
    );
    drop(index);
    worker.shutdown().await;
    let worker = env.start();
    idle(&worker).await;
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    assert!(store::status(index.conn()).unwrap().paused);
    drop(index);
    worker.graph_build(&project).await.unwrap();
    while worker.active_graph_job(&project).is_some() {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    fs::rename(project.join("docs/a.md"), project.join("docs/b.md")).unwrap();
    worker.reindex(&project, false, false).unwrap();
    idle(&worker).await;
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    let current = store::load(index.conn()).unwrap();
    assert!(!store::status(index.conn()).unwrap().paused);
    assert_ne!(old.snapshot, current.snapshot);
    assert_eq!(current.sources[0].meta.docs_path, "b.md");
    assert!(
        current
            .evidence
            .iter()
            .all(|e| e.document != old.sources[0].meta.reference)
    );
    drop(index);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn automatic_graph_job_is_visible_and_duplicate_build_uses_same_id() {
    let env = Env::new();
    let project = env.project("graph-auto", &[("a.md", "# A\n\nТ-1 зависит от Т-2.")]);
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    let gate = lock(&worker.inner.projects)[&project].job_gate.clone();
    let held = gate.lock_owned().await;
    let auto = worker.graph_start(&project, false).await.unwrap();
    let repeated = worker.graph_build(&project).await.unwrap();
    assert_eq!(auto.job_id, repeated.job_id);
    assert_eq!(worker.active_graph_job(&project), Some(auto.job_id.clone()));
    assert!(worker.graph_cancel(&project, &auto.job_id).unwrap());
    drop(held);
    idle(&worker).await;
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    assert!(store::status(index.conn()).unwrap().paused);
    drop(index);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn graph_bad_utf8_is_partial_and_recovery_reuses_index_vectors() {
    let env = Env::new();
    let text = "# A\n\nТ-1 зависит от Т-2.";
    let project = env.project(
        "graph-errors",
        &[("a.md", text), ("b.md", "# B\n\nТ-1 зависит от Т-2.")],
    );
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    worker.shutdown().await;
    let mut index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    let original = store::load(index.conn()).unwrap().snapshot;
    fs::write(project.join("docs/a.md"), [0xff, 0xfe]).unwrap();
    store::set_state(index.conn(), LayerState::Stale, None, None).unwrap();
    let status = bars_core::graph::build_index_graph(&project, &mut index, || false).unwrap();
    assert_eq!(status.file_errors_total, 1);
    assert_eq!(status.file_errors[0].code, "invalid_utf8");
    assert_eq!(status.layers["structure"].failed, 1);
    let partial = store::load(index.conn()).unwrap();
    assert_eq!(partial.failed_sources.len(), 1);
    assert_ne!(partial.snapshot, original);
    fs::write(project.join("docs/a.md"), text).unwrap();
    let status = bars_core::graph::build_index_graph(&project, &mut index, || false).unwrap();
    assert_eq!(status.file_errors_total, 0);
    assert_eq!(store::load(index.conn()).unwrap().snapshot, original);
}

#[tokio::test(flavor = "multi_thread")]
async fn selected_layer_preserves_current_layers_but_never_reuses_old_revision() {
    use bars_core::graph::{
        BuildLayer, GraphError, GraphRequest, Operation, build_index_graph_layer, query,
    };
    let env = Env::new();
    let project = env.project(
        "graph-layers",
        &[
            ("a.md", "# A\n\nТ-1 зависит от Т-2."),
            ("b.md", "# B\n\nТ-2 зависит от Т-3."),
        ],
    );
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    worker.shutdown().await;
    let mut index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    let before = store::status(index.conn()).unwrap();
    let structure =
        build_index_graph_layer(&project, &mut index, BuildLayer::Structure, || false).unwrap();
    assert_eq!(before.snapshot, structure.snapshot);
    assert_eq!(before.layers["semantic"], structure.layers["semantic"]);
    assert_eq!(before.layers["similarity"], structure.layers["similarity"]);
    index
        .conn()
        .execute("DELETE FROM documents WHERE path='docs/a.md'", [])
        .unwrap();
    let structure =
        build_index_graph_layer(&project, &mut index, BuildLayer::Structure, || false).unwrap();
    assert_eq!(structure.layers["semantic"].state, LayerState::NotBuilt);
    assert_eq!(structure.layers["similarity"].state, LayerState::NotBuilt);
    let data = store::load(index.conn()).unwrap();
    assert!(matches!(
        query::execute(
            &data,
            &GraphRequest {
                operation: Operation::Relations,
                query: Some("Т-2".into()),
                ..Default::default()
            },
            || false
        ),
        Err(GraphError::NotReady(_))
    ));
    let semantic =
        build_index_graph_layer(&project, &mut index, BuildLayer::Semantic, || false).unwrap();
    assert_eq!(semantic.layers["semantic"].state, LayerState::Partial);
    assert_eq!(semantic.layers["similarity"].state, LayerState::NotBuilt);
    let similarity =
        build_index_graph_layer(&project, &mut index, BuildLayer::Similarity, || false).unwrap();
    assert_eq!(semantic.layers["semantic"], similarity.layers["semantic"]);
    assert_eq!(similarity.layers["similarity"].state, LayerState::Ready);
    assert_eq!(
        similarity.layers["similarity"].completeness,
        bars_core::graph::Completeness::Complete
    );
    assert_eq!(store::load(index.conn()).unwrap().sources.len(), 1);
    index.conn().execute("DELETE FROM vectors", []).unwrap();
    assert_eq!(
        store::status(index.conn()).unwrap().freshness,
        bars_core::graph::Freshness::Stale
    );
    let missing =
        build_index_graph_layer(&project, &mut index, BuildLayer::Similarity, || false).unwrap();
    let layer = &missing.layers["similarity"];
    assert_eq!(layer.freshness, bars_core::graph::Freshness::Current);
    assert_eq!(layer.completeness, bars_core::graph::Completeness::Partial);
    assert_eq!(layer.coverage.unit, "chunks");
    assert_eq!(layer.coverage.processed, 0);
    assert_eq!(layer.coverage.missing, layer.coverage.total);
    assert!(layer.coverage.total > 0);
}

async fn graph_idle(worker: &Worker, project: &Path) {
    wait_until("задание графа завершится", || {
        worker.active_graph_job(project).is_none()
    })
    .await;
}
fn quiet(worker: &Worker, project: &Path) {
    lock(&worker.inner.projects)
        .get_mut(project)
        .unwrap()
        .watcher
        .take();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn graph_scan_without_changes_repairs_stale_and_resumes_stopped_revision() {
    let env = Env::new();
    let project = env.project("graph-scan", &[("a.md", "# A\n\nТ-1 зависит от Т-2.")]);
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    quiet(&worker, &project);
    let calls = env.provider.counters.embed_calls.load(Ordering::SeqCst);
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    let snapshot = store::load(index.conn()).unwrap().snapshot;
    store::set_state(index.conn(), LayerState::Stale, None, None).unwrap();
    drop(index);
    worker.scan_receipt(&project).unwrap();
    idle(&worker).await;
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    assert_eq!(
        store::status(index.conn()).unwrap().state,
        LayerState::Ready
    );
    assert_eq!(store::load(index.conn()).unwrap().snapshot, snapshot);
    drop(index);
    let gate = lock(&worker.inner.projects)[&project].job_gate.clone();
    let held = gate.lock_owned().await;
    let accepted = worker.graph_build(&project).await.unwrap();
    worker.graph_cancel(&project, &accepted.job_id).unwrap();
    drop(held);
    graph_idle(&worker, &project).await;
    worker.graph_start(&project, false).await.unwrap();
    graph_idle(&worker, &project).await;
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    assert!(store::status(index.conn()).unwrap().paused);
    drop(index);
    worker.scan_receipt(&project).unwrap();
    idle(&worker).await;
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    let status = store::status(index.conn()).unwrap();
    assert!(!status.paused);
    assert_eq!(status.state, LayerState::Ready);
    assert_eq!(
        env.provider.counters.embed_calls.load(Ordering::SeqCst),
        calls
    );
    drop(index);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn graph_cancel_does_not_drop_new_revision_merged_into_queued_job() {
    let env = Env::new();
    let project = env.project(
        "graph-merge",
        &[("a.md", "# A\n\nАльфа."), ("b.md", "# B\n\nБета.")],
    );
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    quiet(&worker, &project);
    let gate = lock(&worker.inner.projects)[&project].job_gate.clone();
    let held = gate.lock_owned().await;
    let accepted = worker.graph_build(&project).await.unwrap();
    worker.graph_cancel(&project, &accepted.job_id).unwrap();
    // Публикация новой индексной ревизии до освобождения очереди графа.
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    index
        .conn()
        .execute("DELETE FROM documents WHERE path='docs/b.md'", [])
        .unwrap();
    drop(index);
    let merged = worker.graph_start(&project, false).await.unwrap();
    assert_eq!(merged.job_id, accepted.job_id);
    drop(held);
    graph_idle(&worker, &project).await;
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    let status = store::status(index.conn()).unwrap();
    assert!(!status.paused);
    assert_eq!(status.state, LayerState::Ready);
    assert_eq!(store::load(index.conn()).unwrap().sources.len(), 1);
    drop(index);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn graph_unchanged_file_error_skips_calculation_and_same_hash_recovery_rebuilds() {
    let env = Env::new();
    let text = "# A\n\nТ-1 зависит от Т-2.";
    let project = env.project(
        "graph-error-repeat",
        &[("a.md", text), ("b.md", "# B\n\nТекст.")],
    );
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    quiet(&worker, &project);
    let calls = env.provider.counters.embed_calls.load(Ordering::SeqCst);
    fs::write(project.join("docs/a.md"), [0xff, 0xfe]).unwrap();
    worker.graph_build(&project).await.unwrap();
    graph_idle(&worker, &project).await;
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    assert_eq!(store::status(index.conn()).unwrap().file_errors_total, 1);
    let marker = bars_core::graph::GraphProgress {
        phase: "skip-proof".into(),
        processed: 7,
        total: Some(7),
        elapsed_ms: 123,
    };
    store::set_progress(index.conn(), &marker).unwrap();
    drop(index);
    for _ in 0..2 {
        worker.graph_start(&project, false).await.unwrap();
        graph_idle(&worker, &project).await;
        let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
        assert_eq!(
            store::status(index.conn()).unwrap().progress.unwrap().phase,
            "skip-proof"
        );
    }
    fs::write(project.join("docs/a.md"), text).unwrap();
    worker.graph_start(&project, false).await.unwrap();
    graph_idle(&worker, &project).await;
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    assert_eq!(store::status(index.conn()).unwrap().file_errors_total, 0);
    assert_eq!(store::load(index.conn()).unwrap().sources.len(), 2);
    assert_eq!(
        env.provider.counters.embed_calls.load(Ordering::SeqCst),
        calls
    );
    drop(index);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn graph_new_index_revision_after_stop_and_restart_updates_automatically() {
    let env = Env::new();
    let project = env.project(
        "graph-stop-new",
        &[("a.md", "# A\n\nАльфа."), ("b.md", "# B\n\nБета.")],
    );
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    quiet(&worker, &project);
    let gate = lock(&worker.inner.projects)[&project].job_gate.clone();
    let held = gate.lock_owned().await;
    let accepted = worker.graph_build(&project).await.unwrap();
    worker.graph_cancel(&project, &accepted.job_id).unwrap();
    drop(held);
    graph_idle(&worker, &project).await;
    worker.shutdown().await;
    let worker = env.start();
    idle(&worker).await;
    quiet(&worker, &project);
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    assert!(store::status(index.conn()).unwrap().paused);
    index
        .conn()
        .execute("DELETE FROM documents WHERE path='docs/b.md'", [])
        .unwrap();
    drop(index);
    worker.graph_start(&project, false).await.unwrap();
    graph_idle(&worker, &project).await;
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    assert!(!store::status(index.conn()).unwrap().paused);
    assert_eq!(store::load(index.conn()).unwrap().sources.len(), 1);
    drop(index);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn graph_signal_at_completion_publishes_latest_index_without_retry_delay() {
    let env = Env::new();
    let project = env.project(
        "graph-finishing-signal",
        &[("a.md", "# A\n\nАльфа."), ("b.md", "# B\n\nБета.")],
    );
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    quiet(&worker, &project);
    let gate = lock(&worker.inner.projects)[&project].job_gate.clone();
    let held = gate.lock_owned().await;
    worker.graph_build(&project).await.unwrap();
    let reached = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    lock(&worker.inner.graph_jobs)
        .get_mut(&project)
        .unwrap()
        .finish_hook = Some((reached.clone(), release.clone()));
    drop(held);
    tokio::time::timeout(Duration::from_secs(5), reached.notified())
        .await
        .unwrap();
    // Расчёт окончен, но задание ещё активно: сигнал не должен потеряться при снятии.
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    index
        .conn()
        .execute("DELETE FROM documents WHERE path='docs/b.md'", [])
        .unwrap();
    drop(index);
    worker.graph_start(&project, false).await.unwrap();
    release.notify_one();
    graph_idle(&worker, &project).await;
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    assert_eq!(
        store::status(index.conn()).unwrap().state,
        LayerState::Ready
    );
    assert_eq!(store::load(index.conn()).unwrap().sources.len(), 1);
    drop(index);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn graph_persisted_retry_releases_gates_and_manual_build_bypasses_delay() {
    let env = Env::new();
    let project = env.project("graph-delayed-retry", &[("a.md", "# A\n\nТекст.")]);
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    quiet(&worker, &project);
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    let key: String = index
        .conn()
        .query_row(
            "SELECT value FROM graph_meta WHERE key='auto_completed'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let after = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        + 300_000;
    let retry = serde_json::json!([key, {"failures": 3, "after_ms": after}]).to_string();
    index.conn().execute("INSERT INTO graph_meta(key,value) VALUES('auto_retry',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [&retry]).unwrap();
    store::set_state(
        index.conn(),
        LayerState::Failed,
        None,
        Some("сбой предыдущего процесса"),
    )
    .unwrap();
    drop(index);
    worker.graph_start(&project, false).await.unwrap();
    wait_until("отложенный повтор", || {
        lock(&worker.inner.graph_jobs)
            .get(&project)
            .is_some_and(|j| j.waiting_retry)
    })
    .await;
    assert!(worker.is_idle());
    let gate = lock(&worker.inner.projects)[&project].job_gate.clone();
    let held = tokio::time::timeout(Duration::from_secs(1), gate.lock_owned())
        .await
        .unwrap();
    drop(held);
    worker.graph_build(&project).await.unwrap();
    graph_idle(&worker, &project).await;
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    assert_eq!(
        store::status(index.conn()).unwrap().state,
        LayerState::Ready
    );
    let retry: String = index
        .conn()
        .query_row(
            "SELECT value FROM graph_meta WHERE key='auto_retry'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(retry.is_empty());
    drop(index);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn graph_late_cancel_with_new_file_error_does_not_restart_same_inputs() {
    let env = Env::new();
    let project = env.project(
        "graph-late-cancel",
        &[("a.md", "# A\n\nАльфа."), ("b.md", "# B\n\nБета.")],
    );
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    quiet(&worker, &project);
    fs::write(project.join("docs/a.md"), [0xff, 0xfe]).unwrap();
    let gate = lock(&worker.inner.projects)[&project].job_gate.clone();
    let held = gate.lock_owned().await;
    let job = worker.graph_build(&project).await.unwrap();
    let reached = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    lock(&worker.inner.graph_jobs)
        .get_mut(&project)
        .unwrap()
        .finish_hook = Some((reached.clone(), release.clone()));
    drop(held);
    tokio::time::timeout(Duration::from_secs(5), reached.notified())
        .await
        .unwrap();
    worker.graph_cancel(&project, &job.job_id).unwrap();
    release.notify_one();
    graph_idle(&worker, &project).await;
    worker.graph_start(&project, false).await.unwrap();
    graph_idle(&worker, &project).await;
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    assert!(store::status(index.conn()).unwrap().paused);
    assert_eq!(store::status(index.conn()).unwrap().file_errors_total, 1);
    drop(index);
    worker.shutdown().await;
}
