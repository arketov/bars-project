use super::*;

async fn value(mcp: &mut Pipe, name: &str, args: Value) -> Value {
    let (error, text) = mcp.tool(name, args).await;
    assert!(!error, "{text}");
    serde_json::from_str(&text).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn disk_reads_freshness_continuation_scope_and_history() {
    let harness = Harness::new("disk-reads");
    let root = harness.project("disk");
    let mut mcp = harness.connect(harness.env(&root));
    mcp.initialize().await;
    mcp.tool("bars_projects", json!({"all":true})).await;
    harness.register(&root).await;
    fs::write(
        root.join("docs/fresh.md"),
        "😀 свежий\n".repeat(12) + "[REST](api/rest.md) [нет](absent.md)\n",
    )
    .unwrap();
    let first = value(
        &mut mcp,
        "bars_text_search",
        json!({"query":"свежий","files":["fresh.md"]}),
    )
    .await;
    assert_eq!(first["documents"][0]["hits"].as_array().unwrap().len(), 5);
    assert_eq!(first["documents"][0]["hits"][0]["column"], 4);
    assert_eq!(first["documents"][0]["path"], "docs/fresh.md");
    assert_eq!(first["documents"][0]["has_more"], true);
    let revision = first["documents"][0]["revision"].clone();
    let next = value(&mut mcp,"bars_text_search",json!({"query":"свежий","path":"docs/fresh.md","offset":5,"revision":revision,"limit_hits":3})).await;
    assert_eq!(next["documents"][0]["hits"].as_array().unwrap().len(), 3);
    assert_eq!(next["documents"][0]["hits"][0]["line"], 6);
    fs::write(
        root.join("docs/fresh.md"),
        "изменён свежий\n[REST](api/rest.md) [нет](absent.md)\n",
    )
    .unwrap();
    let (error, text) = mcp
        .tool(
            "bars_text_search",
            json!({"query":"свежий","path":"docs/fresh.md","offset":5,"revision":revision}),
        )
        .await;
    assert!(error && text.contains("stale_revision"), "{text}");
    let links = value(
        &mut mcp,
        "bars_links",
        json!({"path":"docs/fresh.md","limit":1}),
    )
    .await;
    assert_eq!(links["items"][0]["target"], "docs/api/rest.md");
    assert_eq!(links["has_more"], true);
    let cursor = links["next_cursor"].clone();
    let next = value(
        &mut mcp,
        "bars_links",
        json!({"path":"docs/fresh.md","limit":1,"cursor":cursor}),
    )
    .await;
    assert_eq!(next["items"][0]["broken"], true);
    assert_eq!(next["has_more"], false);
    assert!(
        mcp.tool(
            "bars_links",
            json!({"path":"docs/api/rest.md","cursor":cursor})
        )
        .await
        .0
    );
    fs::write(root.join("docs/absent.md"), "# Теперь есть\n").unwrap();
    let (error, text) = mcp
        .tool(
            "bars_links",
            json!({"path":"docs/fresh.md","cursor":cursor}),
        )
        .await;
    assert!(error && text.contains("stale_cursor"), "{text}");
    let fresh = value(&mut mcp, "bars_links", json!({"path":"docs/fresh.md"})).await;
    assert_eq!(fresh["items"][1]["broken"], false);
    let incoming = value(
        &mut mcp,
        "bars_links",
        json!({"path":"docs/absent.md","direction":"incoming"}),
    )
    .await;
    assert_eq!(incoming["items"][0]["source"], "docs/fresh.md");
    for path in [
        "../secret.md",
        "C:/secret.md",
        "docs/../../secret.md",
        "docs/file.txt",
    ] {
        assert!(mcp.tool("bars_links", json!({"path":path})).await.0);
    }
    assert!(
        mcp.tool("bars_text_search", json!({"query":"x","limit_hits":0}))
            .await
            .0
    );
    assert!(
        mcp.tool("bars_text_search", json!({"query":"[","regex":true}))
            .await
            .0
    );
    let invalid = mcp
        .request(
            "tools/call",
            json!({"name":"bars_text_search","arguments":{"query":"x","window_id":"other"}}),
        )
        .await;
    assert!(invalid.get("error").is_some() || invalid["result"]["isError"] == true);
    let clipped = value(
        &mut mcp,
        "bars_text_search",
        json!({"query":"свежий","limit_docs":999,"limit_hits":999}),
    )
    .await;
    assert_eq!(clipped["clamped"], true);
    assert_eq!(clipped["applied"]["limit_docs"], 100);
    let (_, txt) = mcp
        .tool("bars_links", json!({"path":"docs/fresh.md","format":"txt"}))
        .await;
    assert!(txt.contains("=== LINKS ===") && txt.contains("limit_reasons"));
    let history = bars_worker::mcp_history::History::new(harness.home.path().into());
    let records = history.page(&Default::default()).unwrap().records;
    assert!(records.iter().any(|a| a.tool == "bars_text_search"));
    assert!(records.iter().any(|a| a.tool == "bars_links"));
    let text = serde_json::to_string(&records).unwrap();
    assert!(!text.contains("raw_target") && !text.contains("😀"));
}

#[tokio::test(flavor = "multi_thread")]
async fn disk_reads_bounded_skips_and_parallel_clients() {
    let harness = Harness::new("disk-parallel");
    let root = harness.project("parallel");
    let mut a = harness.connect(harness.env(&root));
    a.initialize().await;
    a.tool("bars_projects", json!({"all":true})).await;
    harness.register(&root).await;
    for n in 0..30 {
        fs::write(root.join(format!("docs/bad{n}.md")), [0xff, 0xfe]).unwrap();
    }
    fs::write(root.join("docs/needle.md"), "needle\n").unwrap();
    let mut b = harness.connect(harness.env(&root));
    b.initialize().await;
    let (x, y) = tokio::join!(
        value(&mut a, "bars_text_search", json!({"query":"needle"})),
        value(&mut b, "bars_text_search", json!({"query":"needle"}))
    );
    for out in [x, y] {
        assert_eq!(out["skipped_total"], 30);
        assert_eq!(out["skipped"].as_array().unwrap().len(), 20);
        assert_eq!(out["skipped_truncated"], true);
        assert_eq!(out["documents"][0]["path"], "docs/needle.md");
    }
    assert!(
        a.tool("bars_links", json!({"path":"docs/needle.md"}))
            .await
            .0,
        "неполная сверка ссылок должна дать ошибку"
    );
}
