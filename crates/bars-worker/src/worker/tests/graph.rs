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
    let old = store::load(index.conn()).unwrap();
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
