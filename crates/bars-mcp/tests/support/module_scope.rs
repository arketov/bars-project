use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn session_modules_reach_initialize_scopes_and_history() {
    let harness = Harness::new("session-modules");
    let root = harness.project("modules");
    fs::write(
        root.join("docs/INDEX.md"),
        "---\ntitle: Общее назначение\n---\n# Общее назначение\n\nОбщие правила поиска.\n",
    )
    .unwrap();
    let title = "Назначение API 🦀".repeat(10);
    fs::write(
        root.join("docs/api/Index.md"),
        format!("---\ntitle: {title}\n---\n# API\n\nОписание протокола поиска.\n"),
    )
    .unwrap();
    fs::create_dir_all(root.join("docs/api/nested")).unwrap();
    fs::write(
        root.join("docs/api/nested/Index.md"),
        "# Вложенное назначение\n\nПоиск.\n",
    )
    .unwrap();
    let mut bootstrap = harness.connect(harness.env(&root));
    bootstrap.initialize().await;
    harness.register(&root).await;
    wait_status(&harness, &root, "индекс модулей", indexed).await;
    let loads = harness.provider.counters.loads.load(Ordering::SeqCst);
    let history = bars_worker::mcp_history::History::new(harness.home.path().into());
    let before = history.page(&Default::default()).unwrap().records.len();
    let mut mcp = harness.connect(harness.env(&root.join("docs/api")));
    let init = mcp.initialize().await;
    let text = init["result"]["instructions"].as_str().unwrap();
    assert!(
        text.contains("Project modules modules: . (2), api (3)"),
        "{text}"
    );
    assert!(text.chars().count() <= 2000);
    assert!(
        harness.provider.counters.loads.load(Ordering::SeqCst) == loads,
        "метаданные не загружают модели"
    );
    assert_eq!(
        history.page(&Default::default()).unwrap().records.len(),
        before,
        "метаданные не создают историю поиска"
    );
    // Широкий поиск печатает разбивку по модулям; область in — произвольные пути.
    let (error, text) = mcp
        .tool(
            "bars_search",
            json!({"query":"поиск", "limit":1, "diagnostic":true}),
        )
        .await;
    assert!(!error, "{text}");
    assert!(text.contains("scope: whole project"), "{text}");
    assert!(text.contains("by_module: candidates"), "{text}");
    assert!(text.contains("Results span several modules"), "{text}");
    let (error, text) = mcp
        .tool(
            "bars_search",
            json!({"query":"поиск", "in":["api", "docs/guide.md"], "diagnostic":true}),
        )
        .await;
    assert!(!error, "{text}");
    assert!(
        text.contains("scope: in api, docs/guide.md"),
        "{text}"
    );
    assert!(!text.contains("Results span several modules"));
    let page = history
        .page(&bars_worker::mcp_history::Filter {
            tool: Some("bars_search".into()),
            ..Default::default()
        })
        .unwrap();
    let scoped = page
        .records
        .iter()
        .find(|r| r.applied["scope"]["paths"] == json!(["api", "docs/guide.md"]))
        .unwrap();
    assert!(scoped.applied["by_module"].is_object());
    let whole = page
        .records
        .iter()
        .find(|r| r.applied["scope"]["paths"] == json!([]))
        .unwrap();
    assert!(whole.applied["by_module"]["candidates"].is_object());
}

#[tokio::test(flavor = "multi_thread")]
async fn tools_list_retries_missing_session_metadata_once() {
    let harness = Harness::new("metadata-retry");
    let root = harness.project("later");
    let mut mcp = harness.connect(harness.env(&root));
    let init = mcp.initialize().await;
    assert!(
        !init["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("Project later modules:")
    );
    harness.register(&root).await;
    wait_status(&harness, &root, "индекс позднего проекта", indexed).await;
    // Метаданные подхватываются со второй попытки (tools/list) и новая сессия их видит.
    mcp.request("tools/list", json!({})).await;
    let mut fresh = harness.connect(harness.env(&root));
    let init = fresh.initialize().await;
    assert!(
        init["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("Project later modules:")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn stalled_worker_is_bounded_and_retried_only_once_by_tools_list() {
    let harness = Harness::new("metadata-timeout");
    let mut env = harness.env(harness.projects.path());
    let launches = harness.launches.clone();
    env.launch = Launch::With(Arc::new(move || {
        launches.fetch_add(1, Ordering::SeqCst);
        Ok(()) // Воркер не появляется.
    }));
    let mut mcp = harness.connect(env);
    let started = Instant::now();
    let init = mcp.initialize().await;
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(
        init["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("modules are top-level")
    );
    let started = Instant::now();
    mcp.request("tools/list", json!({})).await;
    assert!(started.elapsed() < Duration::from_secs(5));
    let started = Instant::now();
    mcp.request("tools/list", json!({})).await;
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(harness.launches.load(Ordering::SeqCst), 2);
}
