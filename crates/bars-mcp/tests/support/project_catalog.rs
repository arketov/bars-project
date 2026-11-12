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

#[tokio::test(flavor = "multi_thread")]
async fn status_filter_lists_only_matching_registry_projects() {
    let harness = Harness::new("project-filter");
    let root = harness.project("current");
    let other = harness.project("other");
    harness.register(&root).await;
    harness.register(&other).await;
    wait_status(&harness, &root, "индекс каталога", indexed).await;
    let mut mcp = harness.connect(harness.env(&root));
    mcp.initialize().await;
    // Регистр и `\` не важны; совпавшая запись одна, части одного проекта не выводятся.
    let (error, text) = mcp.tool("bars_status", json!({"filter":" OTHER "})).await;
    assert!(!error, "{text}");
    assert!(
        text.contains(&format!("path: {}", other.display())),
        "{text}"
    );
    assert!(
        !text.contains(&format!("path: {}", root.display())),
        "{text}"
    );
    assert!(text.contains("=== PROJECTS: 1 ==="), "{text}");
    assert!(
        text.contains("per-project parts need one project: repeat with project=<name from this list>"),
        "{text}"
    );
    assert!(text.contains("=== VERSIONS"), "{text}");
    for part in ["=== INDEX", "=== AUDITS", "=== NEW TOPICS", "=== GRAPH"] {
        assert!(!text.contains(part), "{part}: {text}");
    }
    // `all=true` вместе с filter избыточен: ошибки нет, результат тот же.
    let (error, both) = mcp
        .tool("bars_status", json!({"filter":"other","all":true}))
        .await;
    assert!(!error, "{both}");
    assert!(both.contains("=== PROJECTS: 1 ==="), "{both}");
    // Пустой и пробельный filter — как без filter: проект сессии и его части.
    let (error, blank) = mcp.tool("bars_status", json!({"filter":"   "})).await;
    assert!(!error, "{blank}");
    assert!(blank.contains("=== INDEX"), "{blank}");
    assert!(
        blank.contains(&format!("path: {}", root.display())),
        "{blank}"
    );
    // Сочетания с project и refresh — invalid_params.
    let (error, text) = mcp
        .tool("bars_status", json!({"filter":"other","project":"other"}))
        .await;
    assert!(
        error && text.contains("invalid_params") && text.contains("filter cannot be combined with project"),
        "{text}"
    );
    let (error, text) = mcp
        .tool("bars_status", json!({"filter":"other","refresh":true}))
        .await;
    assert!(error && text.contains("invalid_params"), "{text}");
    // Пустой результат называет размер реестра.
    let (error, text) = mcp.tool("bars_status", json!({"filter":"no-such-project-zz"})).await;
    assert!(!error, "{text}");
    assert!(
        text.contains("no project matches `no-such-project-zz`; the registry has 2 projects (bars_status(all=true) lists them)"),
        "{text}"
    );
}
