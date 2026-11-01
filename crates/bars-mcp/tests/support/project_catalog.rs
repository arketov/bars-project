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
            .contains("model_loading/graph_not_ready")
    );
    let (error, text) = mcp.tool("bars_projects", json!({})).await;
    assert!(error, "{text}");
    assert!(
        text.contains("project_not_found") && text.contains("all=true"),
        "{text}"
    );
    harness.register(&root).await;
    harness.register(&other).await;
    wait_status(&mut mcp, "индекс каталога", indexed).await;
    let loads = harness.provider.counters.loads.load(Ordering::SeqCst);
    let (error, text) = mcp.tool("bars_projects", json!({})).await;
    assert!(!error, "{text}");
    let catalog: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(catalog["projects"].as_array().unwrap().len(), 1);
    assert_eq!(catalog["projects"][0]["path"], root.display().to_string());
    let modules = catalog["projects"][0]["modules"].as_array().unwrap();
    assert_eq!(
        modules.iter().find(|m| m["name"] == ".").unwrap()["title"],
        "Общие правила"
    );
    assert_eq!(
        modules.iter().find(|m| m["name"] == "api").unwrap()["title"],
        "Протокол проекта"
    );
    assert!(
        modules
            .iter()
            .find(|m| m["name"] == "without-index")
            .unwrap()
            .get("title")
            .is_none()
    );
    let (error, text) = mcp.tool("bars_projects", json!({"format":"txt"})).await;
    assert!(!error, "{text}");
    assert!(text.contains("api (3) — Протокол проекта"), "{text}");
    assert!(!text.contains(&other.display().to_string()) && !text.contains("Не назначение API"));
    for project in ["other".to_owned(), other.display().to_string()] {
        let (error, text) = mcp.tool("bars_projects", json!({"project":project})).await;
        assert!(!error, "{text}");
        let output: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(output["projects"].as_array().unwrap().len(), 1);
        assert_eq!(output["projects"][0]["path"], other.display().to_string());
    }
    let (error, text) = mcp.tool("bars_projects", json!({"all":true})).await;
    assert!(!error, "{text}");
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["projects"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        harness.provider.counters.loads.load(Ordering::SeqCst),
        loads
    );
    let history = bars_worker::mcp_history::History::new(harness.home.path().into())
        .page(&Default::default())
        .unwrap();
    assert!(history.records.iter().any(|r| r.tool == "bars_projects"
        && r.requested["all"] == true
        && r.applied["all"] == true));
    assert!(history.records.iter().any(|r| r.tool == "bars_projects"
        && r.requested["project"] == "other"
        && r.applied["resolved_project"] == other.display().to_string()));
    let (error, text) = mcp
        .tool("bars_projects", json!({"all":true,"project":"other"}))
        .await;
    assert!(error && text.contains("invalid_params"), "{text}");
    let (error, text) = mcp
        .tool("bars_projects", json!({"project":"missing"}))
        .await;
    assert!(error && text.contains("project_not_found"), "{text}");
    let mut outside = harness.connect(harness.env(harness.projects.path()));
    outside.initialize().await;
    let (error, text) = outside.tool("bars_projects", json!({})).await;
    assert!(error && text.contains("project_not_found"), "{text}");
    assert!(!text.contains(&other.display().to_string()), "{text}");
    let tools = mcp.request("tools/list", json!({})).await;
    let reindex = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "bars_reindex")
        .unwrap();
    let description = reindex["description"].as_str().unwrap();
    for term in [
        "model_loading",
        "graph_not_ready",
        "bars-cli graph build",
        "reindex_needs_consent",
    ] {
        assert!(description.contains(term), "{description}");
    }
}
