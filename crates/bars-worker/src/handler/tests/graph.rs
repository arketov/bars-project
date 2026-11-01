use super::*;
use bars_core::graph::{
    Endpoint as GraphEndpoint, GraphRequest, Item, Kind, LayerState, Operation, View,
};
use bars_protocol::{GraphBuildParams, GraphCancelParams, GraphQueryParams, GraphStatusParams};

#[tokio::test(flavor = "multi_thread")]
async fn graph_real_ipc_auto_update_refs_scope_and_read_only_mcp() {
    let harness = Harness::start("graph-auto", FakeProvider::new("m1")).await;
    let root = harness.project(
        "graph",
        &[
            (
                "features/requirements.md",
                "# Требования\n\n`A` зависит от `B`.\n\n`B` зависит от `C`.",
            ),
            (
                "guide.md",
                "# Руководство\n\n[Требования](features/requirements.md)",
            ),
        ],
    );
    let mut cli = harness.client(ClientKind::Cli).await;
    cli.call(&RegisterParams {
        path: root.display().to_string(),
    })
    .await
    .unwrap();
    wait_status(
        &mut cli,
        &root,
        "автоматический граф",
        |s| {
            indexed(s)
                && s.graph
                    .as_ref()
                    .is_some_and(|g| g.state == LayerState::Ready)
        },
    )
    .await;
    let mut mcp = harness.client(ClientKind::Mcp).await;
    let graph = mcp
        .call(&GraphQueryParams {
            text: false,
            target: target(&root),
            request: GraphRequest {
                operation: Operation::Graph,
                view: View::Mixed,
                ..Default::default()
            },
        })
        .await
        .unwrap();
    let a = graph
        .nodes
        .iter()
        .find(|n| n.reference.kind == Kind::Entity && n.label == "A")
        .unwrap()
        .reference
        .clone();
    let c = graph
        .nodes
        .iter()
        .find(|n| n.reference.kind == Kind::Entity && n.label == "C")
        .unwrap()
        .reference
        .clone();
    let path = mcp
        .call(&GraphQueryParams {
            text: false,
            target: target(&root),
            request: GraphRequest {
                operation: Operation::Paths,
                from: Some(GraphEndpoint::Ref(a.clone())),
                to: Some(GraphEndpoint::Ref(c)),
                ..Default::default()
            },
        })
        .await
        .unwrap();
    assert!(
        path.items
            .iter()
            .any(|i| matches!(i,Item::Path{path} if path.steps.len()==2))
    );
    assert!(path.evidence.iter().all(|e| !e.quote.is_empty()));
    let denied = rpc(mcp
        .call(&GraphBuildParams {
            target: target(&root),
            layer: Default::default(),
        })
        .await);
    assert_eq!(denied.code, METHOD_NOT_FOUND);
    let denied = rpc(mcp
        .call(&GraphCancelParams {
            target: target(&root),
            job_id: "no-job".into(),
        })
        .await);
    assert_eq!(denied.code, METHOD_NOT_FOUND);
    let old = graph.snapshot;
    for mode in [
        bars_core::graph::RetrievalMode::Hybrid,
        bars_core::graph::RetrievalMode::Mix,
        bars_core::graph::RetrievalMode::Graph,
    ] {
        let mut search = search(&root, "Требования");
        search.min_score = Some(0.0);
        let result = mcp
            .call(&bars_protocol::GraphSearchParams {
                text: false,
                search,
                mode,
                graph_budgets: Default::default(),
                include_related: true,
                related_budget: None,
            })
            .await
            .unwrap();
        assert_eq!(result.applied_mode, mode);
        assert!(result.fallback_reason.is_none());
        if mode == bars_core::graph::RetrievalMode::Graph {
            assert!(result.graph_results.is_some());
            assert!(result.search.results.is_empty());
        }
        if mode == bars_core::graph::RetrievalMode::Mix {
            assert!(result.related.is_some());
        }
    }
    fs::write(
        root.join("docs/features/requirements.md"),
        "# Требования\n\n`A` не зависит от `B`.",
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let state = cli
            .call(&GraphStatusParams {
                target: target(&root),
            })
            .await
            .unwrap();
        if state.snapshot.as_ref().is_some_and(|s| s != &old) && state.state == LayerState::Ready {
            break;
        }
        assert!(Instant::now() < deadline, "автопересчёт графа: {state:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let stale = rpc(mcp
        .call(&GraphQueryParams {
            text: false,
            target: target(&root),
            request: GraphRequest {
                operation: Operation::Relations,
                entity_ref: Some(a),
                ..Default::default()
            },
        })
        .await);
    assert_eq!(stale.kind(), Some(ErrorKind::StaleRef));
    let expired = rpc(mcp
        .call(&GraphQueryParams {
            text: false,
            target: target(&root),
            request: GraphRequest {
                snapshot: Some(old),
                ..Default::default()
            },
        })
        .await);
    assert_eq!(expired.kind(), Some(ErrorKind::SnapshotExpired));
}

#[tokio::test(flavor = "multi_thread")]
async fn graph_search_fallback_is_explicit_and_hybrid_shape_unchanged() {
    let harness = Harness::start("graph-fallback", FakeProvider::new("m1")).await;
    let root = harness.project(
        "fallback",
        &[("guide.md", "# Поиск\n\nИндекс ищет документацию.")],
    );
    let mut cli = harness.client(ClientKind::Cli).await;
    cli.call(&RegisterParams {
        path: root.display().to_string(),
    })
    .await
    .unwrap();
    wait_status(&mut cli, &root, "граф готов", |s| {
        indexed(s) && s.graph.as_ref().is_some_and(|g| g.snapshot.is_some())
    })
    .await;
    let before = cli.call(&search(&root, "поиск")).await.unwrap();
    let hybrid = cli
        .call(&bars_protocol::GraphSearchParams {
            text: false,
            search: search(&root, "поиск"),
            mode: bars_core::graph::RetrievalMode::Hybrid,
            graph_budgets: Default::default(),
            include_related: true,
            related_budget: None,
        })
        .await
        .unwrap();
    assert_eq!(before, hybrid.search);
    let disabled = cli
        .call(&bars_protocol::GraphSearchParams {
            text: false,
            search: search(&root, "поиск"),
            mode: bars_core::graph::RetrievalMode::Mix,
            graph_budgets: Default::default(),
            include_related: false,
            related_budget: None,
        })
        .await
        .unwrap();
    assert_eq!(disabled.search, before);
    assert_eq!(
        disabled.applied_mode,
        bars_core::graph::RetrievalMode::Hybrid
    );
    assert!(disabled.related.is_none());
    assert!(
        disabled
            .fallback_reason
            .unwrap()
            .contains("include_related=false")
    );
    // ISS-88: пакет не помещается даже пустым — выдача без него и причина о related_budget.
    let tiny = cli
        .call(&bars_protocol::GraphSearchParams {
            text: false,
            search: search(&root, "поиск"),
            mode: bars_core::graph::RetrievalMode::Mix,
            graph_budgets: Default::default(),
            include_related: true,
            related_budget: Some(1),
        })
        .await
        .unwrap();
    assert_eq!(tiny.applied_mode, bars_core::graph::RetrievalMode::Hybrid);
    assert!(tiny.related.is_none());
    assert!(
        tiny.fallback_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("related_budget")),
        "{:?}",
        tiny.fallback_reason
    );
    assert_eq!(tiny.search.results, before.results);
    for (mode, include_related, related_budget) in [
        (bars_core::graph::RetrievalMode::Mix, true, Some(0)),
        (bars_core::graph::RetrievalMode::Graph, false, None),
        (bars_core::graph::RetrievalMode::Hybrid, true, Some(10000)),
    ] {
        let error = rpc(cli
            .call(&bars_protocol::GraphSearchParams {
                text: false,
                search: search(&root, "поиск"),
                mode,
                graph_budgets: Default::default(),
                include_related,
                related_budget,
            })
            .await);
        assert_eq!(error.kind(), Some(ErrorKind::InvalidParams));
    }
    assert!(
        serde_json::to_value(&before)
            .unwrap()
            .get("related")
            .is_none()
    );
    let db = bars_core::storage::Index::open(
        &bars_core::storage::index_path(&root),
        bars_core::storage::Journal::Wal,
    )
    .unwrap();
    bars_core::graph::store::set_state(db.conn(), LayerState::Stale, None, None).unwrap();
    let out = cli
        .call(&bars_protocol::GraphSearchParams {
            text: false,
            search: search(&root, "поиск"),
            mode: bars_core::graph::RetrievalMode::Mix,
            graph_budgets: Default::default(),
            include_related: true,
            related_budget: None,
        })
        .await
        .unwrap();
    assert_eq!(out.applied_mode, bars_core::graph::RetrievalMode::Hybrid);
    assert!(out.fallback_reason.is_some());
    assert_eq!(out.search.results, before.results);
    let previous = cli
        .call(&bars_protocol::GraphSearchParams {
            text: false,
            search: search(&root, "поиск"),
            mode: bars_core::graph::RetrievalMode::Graph,
            graph_budgets: Default::default(),
            include_related: true,
            related_budget: None,
        })
        .await
        .unwrap();
    assert!(
        previous
            .graph_results
            .unwrap()
            .warnings
            .iter()
            .any(|w| w.contains("Граф устарел"))
    );
    db.conn()
        .execute("UPDATE documents SET hash='changed'", [])
        .unwrap();
    let error = rpc(cli
        .call(&GraphQueryParams {
            text: false,
            target: target(&root),
            request: GraphRequest {
                operation: Operation::Context,
                query: Some("поиск".into()),
                ..Default::default()
            },
        })
        .await);
    assert_eq!(error.kind(), Some(ErrorKind::GraphNotReady));
}

#[tokio::test(flavor = "multi_thread")]
async fn stale_snapshot_status_and_context_share_revision_rules_over_ipc() {
    let harness = Harness::start("graph-stale-layers", FakeProvider::new("m1")).await;
    let root = harness.project(
        "stale-layers",
        &[
            ("a.md", "# Устройство\n\nТ-1 использует Т-2."),
            ("b.md", "# Решение\n\nТ-3 использует Т-4."),
        ],
    );
    let mut cli = harness.client(ClientKind::Cli).await;
    cli.call(&RegisterParams {
        path: root.display().to_string(),
    })
    .await
    .unwrap();
    let built = wait_status(
        &mut cli,
        &root,
        "полный граф без задания",
        |s| {
            indexed(s)
                && s.graph
                    .as_ref()
                    .is_some_and(|g| g.state == LayerState::Ready && g.active_job.is_none())
        },
    )
    .await;
    let snapshot = built.graph.unwrap().snapshot.unwrap();
    let db = bars_core::storage::Index::open(
        &bars_core::storage::index_path(&root),
        bars_core::storage::Journal::Wal,
    )
    .unwrap();
    db.conn()
        .execute(
            "UPDATE documents SET hash='changed' WHERE path='docs/a.md'",
            [],
        )
        .unwrap();
    bars_core::graph::store::set_state(db.conn(), LayerState::Building, Some("new-job"), None)
        .unwrap();
    let status = cli
        .call(&StatusParams {
            target: target(&root),
        })
        .await
        .unwrap()
        .graph
        .unwrap();
    assert_eq!(status.freshness, bars_core::graph::Freshness::Stale);
    assert_eq!(status.job_state, bars_core::graph::JobState::Running);
    assert_eq!(status.state, LayerState::Stale);
    let old = cli
        .call(&GraphQueryParams {
            text: false,
            target: target(&root),
            request: GraphRequest::default(),
        })
        .await
        .unwrap();
    assert_eq!(old.snapshot, snapshot);
    assert!(old.warnings.iter().any(|w| w.contains("Граф устарел")));
    let context = cli
        .call(&GraphQueryParams {
            text: false,
            target: target(&root),
            request: GraphRequest {
                operation: Operation::Context,
                query: Some("Решение".into()),
                ..Default::default()
            },
        })
        .await
        .unwrap();
    assert_eq!(context.coverage.failed, 1);
    assert!(context.sources.iter().all(|s| s.path != "docs/a.md"));
    assert!(context.sources.iter().any(|s| s.path == "docs/b.md"));
    assert!(context.evidence.iter().all(|e| {
        context
            .sources
            .iter()
            .any(|s| s.reference == e.document && s.path != "docs/a.md")
    }));
    assert!(context.warnings.iter().any(|w| w.contains("Граф устарел")));
}
