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
    assert_eq!(status.state, LayerState::Partial);
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
        store::status(index.conn()).unwrap().state,
        LayerState::Cancelled
    );
    assert!(store::status(index.conn()).unwrap().paused);
    let old = store::load(index.conn()).unwrap();
    drop(index);
    worker.reindex(&project, false, false).unwrap();
    idle(&worker).await;
    let index = Index::open(&index_path(&project), Journal::Wal).unwrap();
    assert!(store::status(index.conn()).unwrap().paused);
    assert_eq!(
        store::status(index.conn()).unwrap().state,
        LayerState::Cancelled
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
    assert_eq!(similarity.layers["similarity"].state, LayerState::Partial);
    assert_eq!(store::load(index.conn()).unwrap().sources.len(), 1);
}
