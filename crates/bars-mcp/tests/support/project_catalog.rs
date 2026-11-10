use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn projects_default_to_session_and_expose_only_requested_catalog() {
    let harness = Harness::new("project-catalog");
    let root = harness.project("current");
    let other = harness.project("other");
    fs::write(
        root.join("docs/INDEX.md"),
        "---\ntitle: Общие правила\n---\n# Общие правила\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/api/Index.md"),
        "---\ntitle: Протокол проекта\n---\n# Протокол проекта\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("docs/api/nested")).unwrap();
    fs::write(
        root.join("docs/api/nested/Index.md"),
        "# Не назначение API\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("docs/without-index")).unwrap();
    fs::write(
        root.join("docs/without-index/page.md"),
        "# Документ без оглавления\n",
    )
    .unwrap();
    let mut mcp = harness.connect(harness.env(&root.join("docs/api")));
    let init = mcp.initialize().await;
    assert!(
        init["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("bars_search")
    );
    let (error, text) = mcp.tool("bars_status", json!({})).await;
    assert!(error, "{text}");
    assert!(
        text.contains("project_not_found") && text.contains("all=true"),
        "{text}"
    );
    harness.register(&root).await;
    harness.register(&other).await;
    wait_status(&harness, &root, "индекс каталога", indexed).await;
    let loads = harness.provider.counters.loads.load(Ordering::SeqCst);
    let (error, text) = mcp.tool("bars_status", json!({})).await;
    assert!(!error, "{text}");
    assert!(
        text.contains(&format!("path: {}", root.display())),
        "{text}"
    );
    assert!(text.contains("docs_root: docs"), "{text}");
    assert!(
        text.contains(". (2) — Общие правила, api (3) — Протокол проекта, without-index (1)"),
        "{text}"
    );
    assert!(!text.contains(&other.display().to_string()), "{text}");
    assert!(!text.contains("Не назначение API"), "{text}");
    for project in ["other".to_owned(), other.display().to_string()] {
        let (error, text) = mcp.tool("bars_status", json!({"project":project})).await;
        assert!(!error, "{text}");
        assert!(
            text.contains(&format!("path: {}", other.display())),
            "{text}"
        );
        assert!(
            !text.contains(&format!("path: {}", root.display())),
            "{text}"
        );
    }
    let (error, text) = mcp.tool("bars_status", json!({"all":true})).await;
    assert!(!error, "{text}");
    assert!(
        text.contains(&format!("path: {}", root.display())),
        "{text}"
    );
    assert!(
        text.contains(&format!("path: {}", other.display())),
        "{text}"
    );
    assert_eq!(
        harness.provider.counters.loads.load(Ordering::SeqCst),
        loads
    );
    let history = bars_worker::mcp_history::History::new(harness.home.path().into())
        .page(&Default::default())
        .unwrap();
    assert!(history.records.iter().any(|r| r.tool == "bars_status"
        && r.requested.get("all") == Some(&json!(true))
        && r.applied["all"] == true));
    assert!(history.records.iter().any(|r| r.tool == "bars_status"
        && r.requested["project"] == "other"
        && r.applied["resolved_project"] == other.display().to_string()));
    let (error, text) = mcp
        .tool("bars_status", json!({"all":true,"project":"other"}))
        .await;
    assert!(error && text.contains("invalid_params"), "{text}");
    let (error, text) = mcp.tool("bars_status", json!({"project":"missing"})).await;
    assert!(error && text.contains("project_not_found"), "{text}");
    let mut outside = harness.connect(harness.env(harness.projects.path()));
    outside.initialize().await;
    let (error, text) = outside.tool("bars_status", json!({})).await;
    assert!(error && text.contains("project_not_found"), "{text}");
    assert!(!text.contains(&other.display().to_string()), "{text}");
}
