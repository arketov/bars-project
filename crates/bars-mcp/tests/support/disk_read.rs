use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn disk_reads_freshness_continuation_scope_and_history() {
    let harness = Harness::new("disk-reads");
    let root = harness.project("disk");
    let mut mcp = harness.connect(harness.env(&root));
    mcp.initialize().await;
    mcp.tool("bars_status", json!({"all":true})).await;
    harness.register(&root).await;
    fs::write(
        root.join("docs/fresh.md"),
        "😀 свежий\n".repeat(12) + "[REST](api/rest.md) [нет](absent.md)\n",
    )
    .unwrap();
    // occurrences: все вхождения строкой, пять на документ, продолжение курсором.
    let (error, text) = mcp
        .tool(
            "bars_search",
            json!({"query":"свежий","occurrences":true,"in":["fresh.md"]}),
        )
        .await;
    assert!(!error, "{text}");
    assert!(text.contains("docs/fresh.md:1:4: 😀 свежий\n"), "{text}");
    assert!(text.contains("docs/fresh.md:5:4: 😀 свежий\n"), "{text}");
    assert!(text.contains("totals: documents 1, hits 12"), "{text}");
    let cursor = text
        .lines()
        .find_map(|line| line.strip_prefix("more in docs/fresh.md: repeat with cursor "))
        .unwrap()
        .to_string();
    let (error, text) = mcp
        .tool(
            "bars_search",
            json!({"query":"свежий","occurrences":true,"in":["fresh.md"],"cursor":cursor}),
        )
        .await;
    assert!(!error, "{text}");
    assert!(text.contains("docs/fresh.md:6:4: 😀 свежий"), "{text}");
    fs::write(
        root.join("docs/fresh.md"),
        "изменён свежий\n[REST](api/rest.md) [нет](absent.md)\n",
    )
    .unwrap();
    let (error, text) = mcp
        .tool(
            "bars_search",
            json!({"query":"свежий","occurrences":true,"in":["fresh.md"],"cursor":cursor}),
        )
        .await;
    assert!(error && text.contains("stale_revision"), "{text}");
    // Ссылки документа — view=links, свежая сверка диска.
    let (error, text) = mcp
        .tool("bars_read", json!({"path":"docs/fresh.md","view":"links"}))
        .await;
    assert!(!error, "{text}");
    assert!(text.contains("2: REST → docs/api/rest.md"), "{text}");
    assert!(text.contains("2: нет → absent.md BROKEN"), "{text}");
    fs::write(root.join("docs/absent.md"), "# Теперь есть\n").unwrap();
    let (error, text) = mcp
        .tool("bars_read", json!({"path":"docs/fresh.md","view":"links"}))
        .await;
    assert!(!error, "{text}");
    assert!(text.contains("2: REST → docs/api/rest.md"), "{text}");
    assert!(text.contains("2: нет → docs/absent.md"), "{text}");
    // Входящие ссылки.
    let (error, text) = mcp
        .tool(
            "bars_read",
            json!({"path":"docs/absent.md","view":"links","direction":"in"}),
        )
        .await;
    assert!(!error, "{text}");
    assert!(text.contains("docs/fresh.md:2"), "{text}");
    for path in [
        "../secret.md",
        "C:/secret.md",
        "docs/../../secret.md",
        "docs/file.txt",
    ] {
        let (error, text) = mcp
            .tool("bars_read", json!({"path":path,"view":"links"}))
            .await;
        assert!(error, "{path}: {text}");
    }
    assert!(
        mcp.tool(
            "bars_search",
            json!({"query":"x","occurrences":true,"limit":0})
        )
        .await
        .0
    );
    let (bad, bad_text) = mcp
        .tool("bars_search", json!({"query":"/[/","occurrences":true}))
        .await;
    assert!(bad && bad_text.contains("invalid_regex"), "{bad_text}");
    // Широкий шаблон — ошибка too_broad с отсылкой к единому поиску (ISS-109);
    // ветки считаются у регулярного выражения, поэтому форма /…/.
    let (broad, broad_text) = mcp
        .tool(
            "bars_search",
            json!({"query":"/foo|bar|baz|qux|quux/","occurrences":true}),
        )
        .await;
    assert!(
        broad && broad_text.contains("too_broad") && broad_text.contains("bars_search"),
        "{broad_text}"
    );
    let (error, text) = mcp
        .tool(
            "bars_search",
            json!({"query":"/свеж\\w+/","occurrences":true}),
        )
        .await;
    assert!(!error && text.contains("изменён свежий"), "{text}");
    // Неизвестные поля не роняют вызов: история хранит только известные аргументы.
    let (error, _) = mcp
        .tool(
            "bars_search",
            json!({"query":"свежий","occurrences":true,"window_id":"other"}),
        )
        .await;
    assert!(!error);
    let (error, text) = mcp
        .tool(
            "bars_search",
            json!({"query":"свежий","occurrences":true,"limit":999}),
        )
        .await;
    assert!(!error, "{text}");
    assert!(text.contains("clamped"), "{text}");
    let history = bars_worker::mcp_history::History::new(harness.home.path().into());
    let records = history.page(&Default::default()).unwrap().records;
    assert!(records.iter().any(|a| a.tool == "bars_search"));
    assert!(records.iter().any(|a| a.tool == "bars_read"));
    let text = serde_json::to_string(&records).unwrap();
    assert!(!text.contains("raw_target") && !text.contains("😀"));
}

#[tokio::test(flavor = "multi_thread")]
async fn disk_reads_bounded_skips_and_parallel_clients() {
    let harness = Harness::new("disk-parallel");
    let root = harness.project("parallel");
    let mut a = harness.connect(harness.env(&root));
    a.initialize().await;
    a.tool("bars_status", json!({"all":true})).await;
    harness.register(&root).await;
    for n in 0..30 {
        fs::write(root.join(format!("docs/bad{n}.md")), [0xff, 0xfe]).unwrap();
    }
    fs::write(root.join("docs/needle.md"), "needle\n").unwrap();
    let mut b = harness.connect(harness.env(&root));
    b.initialize().await;
    let (x, y) = tokio::join!(
        a.tool("bars_search", json!({"query":"needle","occurrences":true})),
        b.tool("bars_search", json!({"query":"needle","occurrences":true}))
    );
    for (error, text) in [x, y] {
        assert!(!error, "{text}");
        assert!(text.contains("skipped: 30 files"), "{text}");
        assert!(text.contains("docs/needle.md:1:1: needle"), "{text}");
    }
    assert!(
        a.tool("bars_read", json!({"path":"docs/needle.md","view":"links"}))
            .await
            .0,
        "неполная сверка ссылок должна дать ошибку"
    );
}
