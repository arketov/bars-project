use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn session_modules_reach_initialize_schemas_search_and_history() {
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
    wait_status(&mut bootstrap, "индекс модулей", indexed).await;
    let loads = harness.provider.counters.loads.load(Ordering::SeqCst);
    let history = bars_worker::mcp_history::History::new(harness.home.path().into());
    let before = history.page(&Default::default()).unwrap().records.len();
    let mut mcp = harness.connect(harness.env(&root.join("docs/api")));
    let init = mcp.initialize().await;
    let text = init["result"]["instructions"].as_str().unwrap();
    assert!(
        text.contains("Модули проекта modules: . (2), api (3)"),
        "{text}"
    );
    assert!(text.chars().count() <= 2000);
    for _ in 0..2 {
        let list = mcp.request("tools/list", json!({})).await;
        for name in ["bars_rag_search", "bars_get_similar", "bars_get_duplicates"] {
            let tool = list["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["name"] == name)
                .unwrap();
            let field = &tool["inputSchema"]["properties"]["modules"];
            let description = field["description"].as_str().unwrap();
            assert!(description.contains("Общее назначение"), "{description}");
            assert!(
                description.contains(&title.chars().take(60).collect::<String>()),
                "{description}"
            );
            assert!(!description.contains("Вложенное назначение"));
            assert!(field.get("enum").is_none());
            assert!(field["items"].get("enum").is_none());
            assert!(description.contains("другого project"));
        }
    }
    assert_eq!(
        harness.provider.counters.loads.load(Ordering::SeqCst),
        loads,
        "метаданные не загружают модели"
    );
    assert_eq!(
        history.page(&Default::default()).unwrap().records.len(),
        before,
        "метаданные не создают историю поиска"
    );
    let (error, body) = mcp
        .tool(
            "bars_rag_search",
            json!({"query":"поиск", "limit":1, "min_score":0.0, "diagnostic":true}),
        )
        .await;
    assert!(!error, "{body}");
    let output: Value = serde_json::from_str(&body).unwrap();
    let candidates: u64 = output["by_module"]["candidates"]
        .as_object()
        .unwrap()
        .values()
        .map(|n| n.as_u64().unwrap())
        .sum();
    assert!(
        candidates > output["results"].as_array().unwrap().len() as u64,
        "{output}"
    );
    assert_eq!(output["applied"]["scope"], json!({"modules":[],"files":[]}));
    let page = history
        .page(&bars_worker::mcp_history::Filter {
            tool: Some("bars_rag_search".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(page.records[0].applied["by_module"], output["by_module"]);
    assert_eq!(page.records[0].applied["scope"], output["applied"]["scope"]);
    let (error, text) = mcp.tool("bars_rag_search", json!({"query":"поиск", "limit":20, "min_score":0.0, "format":"txt", "diagnostic":true})).await;
    assert!(!error, "{text}");
    assert!(text.contains("scope: whole project"), "{text}");
    assert!(text.contains("by_module: candidates"), "{text}");
    assert!(text.contains("Results span several modules"), "{text}");
    let (error, text) = mcp.tool("bars_rag_search", json!({"query":"поиск", "modules":["api"], "files":["docs/guide.md"], "format":"txt", "diagnostic":true})).await;
    assert!(!error, "{text}");
    assert!(
        text.contains("scope: modules api; files docs/guide.md"),
        "{text}"
    );
    assert!(!text.contains("Results span several modules"));
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
            .contains("Модули проекта later")
    );
    harness.register(&root).await;
    wait_status(&mut mcp, "индекс позднего проекта", indexed).await;
    let list = mcp.request("tools/list", json!({})).await;
    let tool = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "bars_rag_search")
        .unwrap();
    assert!(
        tool["inputSchema"]["properties"]["modules"]["description"]
            .as_str()
            .unwrap()
            .contains("Модули проекта сессии later")
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
            .contains("имена даёт bars_projects")
    );
    let started = Instant::now();
    mcp.request("tools/list", json!({})).await;
    assert!(started.elapsed() < Duration::from_secs(5));
    let started = Instant::now();
    mcp.request("tools/list", json!({})).await;
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(harness.launches.load(Ordering::SeqCst), 2);
}
