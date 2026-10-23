//! Regression/development, including the published 20 October census; not a new holdout.
use bars_graph::{
    source::{InputDocument, build},
    *,
};

fn doc(path: &str, text: &str) -> InputDocument {
    InputDocument {
        path: format!("docs/{path}"),
        docs_path: path.into(),
        module: "features".into(),
        title: path.into(),
        text: text.into(),
        chunks: vec![],
    }
}
fn graph(text: &str) -> Dataset {
    build(
        "fixture",
        "docs",
        vec![doc("a.md", &format!("# Тест\n\n{text}"))],
    )
    .unwrap()
}
fn facts(data: &Dataset) -> Vec<&Edge> {
    data.edges
        .iter()
        .filter(|e| e.origin == Origin::ExplicitRule)
        .collect()
}
fn label<'a>(data: &'a Dataset, id: &str) -> &'a str {
    &data
        .nodes
        .iter()
        .find(|n| n.reference.id == id)
        .unwrap()
        .label
}
fn exact_proofs(data: &Dataset) {
    for e in &data.evidence {
        let source = data
            .sources
            .iter()
            .find(|s| s.meta.reference == e.document)
            .unwrap();
        assert_eq!(
            source.text.get(e.span.start..e.span.end),
            Some(e.quote.as_str())
        );
        assert_eq!(source.meta.hash, e.source_hash);
    }
}

#[test]
fn negation_and_conditions_belong_to_the_assertion() {
    for (text, polarity, condition) in [
        ("`Реранкер` не требует `GPU`.", Polarity::Negative, None),
        (
            "Если выбран CPU, `Реранкер` не требует `GPU`.",
            Polarity::Negative,
            Some("Если выбран CPU"),
        ),
        (
            "Без драйвера Vulkan модель использует CPU fallback.",
            Polarity::Positive,
            Some("Без драйвера Vulkan"),
        ),
        (
            "Без драйвера Vulkan, модель использует CPU fallback.",
            Polarity::Positive,
            Some("Без драйвера Vulkan"),
        ),
        (
            "`Реранкер` не использует `GPU`, если выбран CPU.",
            Polarity::Negative,
            Some("если выбран CPU"),
        ),
        (
            "При выборе CPU, `Реранкер` использует `Индекс`.",
            Polarity::Positive,
            Some("При выборе CPU"),
        ),
        (
            "`Реранкер` использует `Индекс`, когда выбран CPU.",
            Polarity::Positive,
            Some("когда выбран CPU"),
        ),
    ] {
        let data = graph(text);
        let edges = facts(&data);
        assert_eq!(edges.len(), 1, "{text}");
        assert_eq!(edges[0].polarity, polarity, "{text}");
        assert_eq!(
            edges[0].qualifiers.get("condition").map(String::as_str),
            condition,
            "{text}"
        );
        assert!(!label(&data, &edges[0].from).starts_with("Без "));
        assert!(edges.iter().all(|e| e.predicate != Predicate::Requires));
        exact_proofs(&data);
    }
    for text in [
        "не требует отдельного фонового сервиса.",
        "Не требует `GPU`.",
        "Без достаточной памяти модель использует CPU.",
        "Если выбран CPU, A использует B, если включён fallback.",
    ] {
        assert!(facts(&graph(text)).is_empty(), "{text}");
    }
}

#[test]
fn cleaned_names_preserve_paired_code_and_do_not_swallow_other_clauses() {
    let data = graph("Снятие регистрации `unregister` не заменяет полное удаление.");
    let edges = facts(&data);
    assert_eq!(edges.len(), 1);
    assert_eq!(
        label(&data, &edges[0].from),
        "Снятие регистрации `unregister`"
    );
    assert_eq!(edges[0].polarity, Polarity::Negative);
    for text in [
        "У модели fastembed метод `embed требует доступа.",
        "У модели fastembed метод `embed` требует доступа: одна модель считает один вызов.",
        "Бэкенд заново проверяет цель перед действием.",
        "A использует тот же проверенный путь.",
        "A проверяет этот отчёт.",
        "A использует B и проверяет C.",
        "`A использует B` проверяет C.",
        "'Незакрытое имя использует B.",
    ] {
        let data = graph(text);
        assert!(facts(&data).is_empty(), "{text}");
        assert!(data.evidence.iter().any(|e| e.quote.contains(text)));
    }
}

#[test]
fn chain_is_extracted_across_files_with_alias_declaration_proof() {
    let data = build(
        "fixture",
        "docs",
        vec![
            doc(
                "a.md",
                "# Первый\n\n`bars::worker` использует `bars::queue`.",
            ),
            doc(
                "b.md",
                "# Второй\n\n`Очередь` зависит от Т-19.\n\n`bars::queue` (алиас: `Очередь`).",
            ),
            doc("c.md", "# Третий\n\nТ-19 требует `bars::consent`."),
        ],
    )
    .unwrap();
    assert_eq!(facts(&data).len(), 3);
    let queue = data
        .nodes
        .iter()
        .find(|n| n.label == "bars::queue")
        .unwrap();
    assert_eq!(queue.source_ids.len(), 2);
    assert_eq!(queue.aliases, ["Очередь"]);
    let to = |name: &str| Endpoint::Name {
        label: name.into(),
        kind: Some(Kind::Entity),
        namespace: None,
    };
    let request = GraphRequest {
        operation: Operation::Paths,
        from: Some(to("bars::worker")),
        to: Some(to("bars::consent")),
        max_hops: Some(3),
        budgets: Budgets {
            max_depth: Some(3),
            ..Default::default()
        },
        path_kind: PathKind::Semantic,
        ..Default::default()
    };
    let reply = bars_graph::query::execute(&data, &request, || false).unwrap();
    let value = serde_json::to_value(&reply).unwrap();
    assert!(!reply.items.is_empty(), "{value}");
    // Every extracted step, plus identity evidence, survives the returned path.
    for edge in facts(&data) {
        for id in &edge.evidence {
            assert!(reply.evidence.iter().any(|e| &e.id == id));
        }
    }
    assert!(reply.evidence.iter().any(|e| e.quote.contains("(алиас: ")));
    exact_proofs(&data);
    let reversed = build(
        "fixture",
        "docs",
        vec![
            doc("c.md", "# Третий\n\nТ-19 требует `bars::consent`."),
            doc(
                "b.md",
                "# Второй\n\n`Очередь` зависит от Т-19.\n\n`bars::queue` (алиас: `Очередь`).",
            ),
            doc(
                "a.md",
                "# Первый\n\n`bars::worker` использует `bars::queue`.",
            ),
        ],
    )
    .unwrap();
    assert_eq!(data.snapshot, reversed.snapshot);
    let scoped = GraphRequest {
        operation: Operation::Relations,
        query: Some("Очередь".into()),
        scope: Scope {
            files: vec!["b.md".into()],
            ..Default::default()
        },
        ..Default::default()
    };
    let reply = bars_graph::query::execute(&data, &scoped, || false).unwrap();
    assert!(!reply.items.is_empty());
    assert!(reply.evidence.iter().all(|e| {
        data.sources
            .iter()
            .any(|s| s.meta.docs_path == "b.md" && s.meta.reference == e.document)
    }));
    let scoped = GraphRequest {
        scope: Scope {
            files: vec!["a.md".into()],
            ..Default::default()
        },
        ..scoped
    };
    let reply = bars_graph::query::execute(&data, &scoped, || false).unwrap();
    assert!(
        reply.items.is_empty(),
        "alias from excluded b.md must not resolve in a.md"
    );
}

#[test]
fn ambiguous_aliases_short_names_case_and_code_blocks_do_not_merge() {
    let data = build("fixture", "docs", vec![
        doc("a.md", "# A\n\n`embed` использует `rerank`.\n\n`bars::embed` (алиас: `Модель`).\n\n`bars::rerank` (алиас: `Модель`).\n\n`Модель` использует `GPU`.\n\n```md\n`bars::fake` (алиас: `embed`).\n```"),
        doc("b.md", "# B\n\n`embed` использует `rerank`.\n\n`Bars::Queue` использует `bars::Queue`."),
    ]).unwrap();
    assert_eq!(facts(&data).len(), 3);
    for name in ["embed", "rerank"] {
        let nodes: Vec<_> = data.nodes.iter().filter(|n| n.label == name).collect();
        assert_eq!(nodes.len(), 2);
        assert_ne!(nodes[0].namespace, nodes[1].namespace);
    }
    assert!(!data.nodes.iter().any(|n| n.label == "bars::fake"));
    assert!(
        !facts(&data)
            .iter()
            .any(|e| label(&data, &e.from) == "Модель")
    );
    assert_ne!(
        data.nodes
            .iter()
            .find(|n| n.label == "Bars::Queue")
            .unwrap()
            .reference
            .id,
        data.nodes
            .iter()
            .find(|n| n.label == "bars::Queue")
            .unwrap()
            .reference
            .id
    );
    let named = |label: &str| Endpoint::Name {
        label: label.into(),
        kind: Some(Kind::Entity),
        namespace: None,
    };
    let request = GraphRequest {
        operation: Operation::Paths,
        from: Some(named("Bars::Queue")),
        to: Some(named("bars::Queue")),
        ..Default::default()
    };
    let reply = bars_graph::query::execute(&data, &request, || false).unwrap();
    assert_eq!(reply.resolution, "resolved");
    assert!(!reply.items.is_empty());
}

#[test]
fn requires_is_only_a_legacy_input_filter_and_warns() {
    let data = graph("A требует B.\n\nA зависит от C.\n\nA выполняется только после D.");
    let request: GraphRequest = serde_json::from_value(
        serde_json::json!({"operation":"relations", "query":"A", "predicates":["requires"]}),
    )
    .unwrap();
    let reply = bars_graph::query::execute(&data, &request, || false).unwrap();
    assert!(!reply.items.is_empty());
    assert!(
        reply
            .warnings
            .iter()
            .any(|w| w.contains("requires устарел"))
    );
    assert!(reply.items.iter().all(|item| matches!(item, Item::Relation { assertion: e }
        if e.predicate == Predicate::DependsOn && e.qualifiers.get("kind").map(String::as_str) == Some("precondition"))));
    assert!(
        facts(&data)
            .iter()
            .all(|e| e.predicate != Predicate::Requires)
    );
}

#[test]
fn october_census_keeps_supported_facts_and_corrects_confirmed_errors() {
    let review: serde_json::Value = serde_json::from_str(include_str!(
        "../../../docs/knowledge/graph-quality-2026-10-20/review.json"
    ))
    .unwrap();
    for entry in review["assertions"].as_array().unwrap() {
        let quote = entry["proof"][0]["quote"].as_str().unwrap();
        let data = graph(quote);
        let edges = facts(&data);
        if entry["verdict"] == "supported" {
            assert_eq!(edges.len(), 1, "{quote}");
            assert_eq!(
                label(&data, &edges[0].from),
                entry["subject"].as_str().unwrap()
            );
            assert_eq!(
                label(&data, &edges[0].to),
                entry["object"].as_str().unwrap()
            );
        }
        for node in data
            .nodes
            .iter()
            .filter(|n| n.reference.kind == Kind::Entity)
        {
            assert_eq!(
                node.label.bytes().filter(|b| *b == b'`').count() % 2,
                0,
                "{quote}"
            );
            assert_ne!(node.label, "не");
        }
        exact_proofs(&data);
    }
}
