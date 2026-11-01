use super::*;
use crate::graph_types::*;

fn dataset() -> Dataset {
    source::build(
        "test",
        "docs",
        vec![source::InputDocument {
            path: "docs/a.md".into(),
            docs_path: "a.md".into(),
            module: ".".into(),
            title: "Требования".into(),
            chunks: vec![],
            text: "# Требования\n\n`A` зависит от `B`.\n\n`B` зависит от `C`.\n".into(),
        }],
    )
    .unwrap()
}

#[test]
fn scoped_graph_with_relations_does_not_claim_empty_search_results() {
    let mut output = GraphSearchOutput {
        search: serde_json::from_value(serde_json::json!({
            "results": [],
            "applied": { "results":0, "candidates":0, "rerank_candidates":0,
                "min_score":0.0, "score_kind":"cosine", "clamped":false,
                "scope":{"modules":["."],"files":[]} },
            "indexing":false, "pending":0
        }))
        .unwrap(),
        requested_mode: RetrievalMode::Graph,
        applied_mode: RetrievalMode::Graph,
        fallback_reason: None,
        related: None,
        graph_results: Some(reply(Operation::Relations)),
    };
    assert!(!output.graph_results.as_ref().unwrap().items.is_empty());
    prepare_graph_search_text(&mut output).unwrap();
    let text = render_graph_search_text(&output);
    assert!(text.contains("scope: modules ."), "{text}");
    assert!(
        !text.contains("No results in the selected modules"),
        "{text}"
    );
    output.applied_mode = RetrievalMode::Mix;
    output.graph_results = None;
    assert!(render_graph_search_text(&output).contains("No results in the selected modules"));
}
fn reply(operation: Operation) -> GraphReply {
    let request = GraphRequest {
        operation,
        query: if matches!(operation, Operation::Context | Operation::Relations) {
            Some("зависит".into())
        } else {
            None
        },
        view: crate::graph_types::View::Mixed,
        ..Default::default()
    };
    query::execute(&dataset(), &request, || false).unwrap()
}

#[test]
fn text_renders_every_item_kind_with_source_locations_and_no_internal_ids() {
    let mut r = reply(Operation::Graph);
    let edge = r
        .assertions
        .iter()
        .find(|e| e.predicate.semantic())
        .unwrap()
        .clone();
    let node = r
        .nodes
        .iter()
        .find(|n| n.reference.kind == Kind::Entity)
        .unwrap()
        .clone();
    let path = GraphPath {
        nodes: vec![
            r.nodes
                .iter()
                .find(|n| n.reference.id == edge.from)
                .unwrap()
                .reference
                .clone(),
            r.nodes
                .iter()
                .find(|n| n.reference.id == edge.to)
                .unwrap()
                .reference
                .clone(),
        ],
        steps: vec![Traversal {
            edge: edge.id.clone(),
            reversed: true,
        }],
        evidence: edge.evidence.clone(),
        complete: true,
    };
    let proof = edge.evidence.clone();
    r.items = vec![
        Item::Relation {
            assertion: edge.clone(),
        },
        Item::Edge { edge: edge.clone() },
        Item::Path { path: path.clone() },
        Item::Impact {
            node: node.reference.clone(),
            direct: false,
            path,
        },
        Item::RoleGroup {
            role: Role::Requirement,
            evidence: proof.clone(),
        },
        Item::Node { node: node.clone() },
        Item::Topic {
            id: "hidden".into(),
            label: "Требования".into(),
            members: vec![node.reference.clone()],
            representatives: vec![node.reference.clone()],
        },
        Item::Audit {
            kind: "isolate".into(),
            node: Some(node.reference),
            evidence: proof.clone(),
            detail: "Кандидат пробела".into(),
        },
        Item::Conflict {
            assertions: vec![edge.id.clone()],
            evidence: proof,
            candidate: true,
        },
    ];
    let text = render_graph_text(&r);
    for expected in [
        "depends_on",
        "reverse traversal",
        "indirect",
        "role: requirement",
        "topic:",
        "Кандидат пробела",
        "not a verdict",
        "a.md:",
        "see fact",
    ] {
        assert!(text.contains(expected), "{expected}: {text}");
    }
    for hidden in [&edge.id, &r.snapshot, &r.evidence[0].id] {
        assert!(!text.contains(hidden), "внутренний id {hidden}");
    }
    assert!(!text.contains("null"));
    assert!(text.chars().count() <= MAX_TEXT_CHARS);
}

#[test]
fn text_pagination_continues_at_first_hidden_fact_without_skips() {
    let text = (0..120)
        .map(|i| format!("\n`Сущность{i}` зависит от `Основание{i}`.\n"))
        .collect::<String>();
    let data = source::build(
        "test",
        "docs",
        vec![source::InputDocument {
            path: "docs/a.md".into(),
            docs_path: "a.md".into(),
            module: ".".into(),
            title: "Требования".into(),
            chunks: vec![],
            text: format!("# Требования\n{text}"),
        }],
    )
    .unwrap();
    let mut request = GraphRequest {
        operation: Operation::Relations,
        query: Some("зависит".into()),
        limit: Some(500),
        budgets: Budgets {
            max_nodes: Some(2000),
            max_bytes: Some(2_097_152),
            ..Default::default()
        },
        ..Default::default()
    };
    let whole = query::execute(&data, &request, || false).unwrap();
    let expected: Vec<_> = whole
        .items
        .iter()
        .filter_map(|i| {
            if let Item::Relation { assertion } = i {
                Some(assertion.id.clone())
            } else {
                None
            }
        })
        .collect();
    let mut actual = vec![];
    for _ in 0..100 {
        let mut page = query::execute(&data, &request, || false).unwrap();
        prepare_graph_text(&mut page, MAX_TEXT_CHARS).unwrap();
        let text = render_graph_text(&page);
        assert!(
            text.chars().count() <= MAX_TEXT_CHARS,
            "{}",
            text.chars().count()
        );
        actual.extend(page.items.iter().filter_map(|i| {
            if let Item::Relation { assertion } = i {
                Some(assertion.id.clone())
            } else {
                None
            }
        }));
        if let Some(cursor) = page.next_cursor {
            assert!(text.contains("truncated:"));
            request.cursor = Some(cursor);
        } else {
            break;
        }
    }
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), actual.iter().collect::<BTreeSet<_>>().len());
}

#[test]
fn context_deduplicates_contained_quotes_and_bounds_long_cyrillic_text() {
    let mut r = reply(Operation::Graph);
    r.operation = Operation::Context;
    r.requested.operation = Operation::Context;
    let e = r.evidence[0].clone();
    r.evidence[0].quote = "Длинная русская цитата. ".repeat(10000);
    r.items = vec![
        Item::RoleGroup {
            role: Role::Requirement,
            evidence: vec![e.id.clone()],
        },
        Item::RoleGroup {
            role: Role::Decision,
            evidence: vec![e.id],
        },
    ];
    prepare_graph_text(&mut r, MAX_TEXT_CHARS).unwrap();
    let text = render_graph_text(&r);
    assert!(text.chars().count() <= MAX_TEXT_CHARS);
    assert_eq!(text.matches("«Длинная").count(), 1);
    assert!(text.contains("see fact 1"));
}

#[test]
fn empty_and_partial_answers_are_explicit_and_json_stays_intact() {
    let mut r = reply(Operation::Relations);
    r.items.clear();
    r.layer_states
        .insert("semantic".into(), LayerState::Partial);
    let before = serde_json::to_value(&r).unwrap();
    let text = render_graph_text(&r);
    assert!(text.contains("next:"));
    assert!(text.contains("semantic layer partial"));
    assert_eq!(before, serde_json::to_value(&r).unwrap());
}

#[test]
fn structural_locations_do_not_suppress_semantic_quotes_or_quote_structural_paths() {
    let mut r = reply(Operation::Graph);
    let mut edge = r
        .assertions
        .iter()
        .find(|e| e.origin == Origin::ExplicitRule)
        .unwrap()
        .clone();
    let mut structural = edge.clone();
    structural.origin = Origin::Structural;
    structural.id = "structural-edge".into();
    let quote = r
        .evidence
        .iter()
        .find(|e| e.id == edge.evidence[0])
        .unwrap()
        .quote
        .trim()
        .to_owned();
    r.items = vec![
        Item::Edge {
            edge: structural.clone(),
        },
        Item::Relation {
            assertion: edge.clone(),
        },
    ];
    let text = render_graph_text(&r);
    assert!(text.contains(&quote), "{text}");
    edge.origin = Origin::Structural;
    r.assertions = vec![edge.clone()];
    r.items = vec![Item::Path {
        path: GraphPath {
            nodes: vec![],
            steps: vec![Traversal {
                edge: edge.id,
                reversed: false,
            }],
            evidence: edge.evidence,
            complete: true,
        },
    }];
    assert!(!render_graph_text(&r).contains('«'));
}

#[test]
fn nested_ranges_are_merged_before_facts_regardless_of_order() {
    let mut r = reply(Operation::Graph);
    let mut first = r.evidence[0].clone();
    first.span.first_line = 10;
    first.span.last_line = 12;
    first.quote = "Общая цитата".into();
    let mut second = first.clone();
    second.id = "wide".into();
    second.span.last_line = 20;
    second.quote = "Общая цитата и продолжение".into();
    r.evidence = vec![first.clone(), second.clone()];
    r.items = vec![
        Item::RoleGroup {
            role: Role::Requirement,
            evidence: vec![first.id],
        },
        Item::RoleGroup {
            role: Role::Decision,
            evidence: vec![second.id],
        },
    ];
    let text = render_graph_text(&r);
    assert_eq!(text.matches("«Общая").count(), 1, "{text}");
    assert!(text.contains("10-20"));
    assert!(text.contains("see fact 1"));
}

#[test]
fn oversized_structural_unit_fails_explicitly_without_consuming_its_tail() {
    let mut r = reply(Operation::Graph);
    let mut edge = r.assertions[0].clone();
    edge.qualifiers = (0..500)
        .map(|i| (format!("k{i}"), "Длинное условие ".repeat(30)))
        .collect();
    r.items = vec![Item::Edge { edge }];
    let error = prepare_graph_text(&mut r, MAX_TEXT_CHARS).unwrap_err();
    assert!(error.to_string().contains("format=json"));
    assert_eq!(r.items.len(), 1);
    assert!(r.next_cursor.is_none());
}

#[test]
fn global_dedup_includes_node_topic_and_extra_path_quotes() {
    let mut r = reply(Operation::Graph);
    let mut narrow = r.evidence[0].clone();
    narrow.span.first_line = 10;
    narrow.span.last_line = 12;
    narrow.quote = "Общий текст".into();
    let mut wide = narrow.clone();
    wide.id = "wide-node".into();
    wide.span.last_line = 20;
    wide.quote = "Общий текст и продолжение".into();
    let mut edge = r
        .assertions
        .iter()
        .find(|e| e.origin == Origin::ExplicitRule)
        .unwrap()
        .clone();
    edge.evidence = vec![narrow.id.clone()];
    let mut node = r.nodes[0].clone();
    node.evidence = vec![wide.id.clone()];
    r.nodes = vec![node.clone()];
    r.assertions = vec![edge.clone()];
    r.evidence = vec![narrow, wide.clone()];
    let variants = vec![
        Item::Node { node: node.clone() },
        Item::Topic {
            id: "topic".into(),
            label: "Тема".into(),
            members: vec![node.reference.clone()],
            representatives: vec![node.reference],
        },
        Item::Path {
            path: GraphPath {
                nodes: vec![],
                steps: vec![],
                evidence: vec![wide.id],
                complete: true,
            },
        },
    ];
    for item in variants {
        r.items = vec![
            Item::Relation {
                assertion: edge.clone(),
            },
            item,
        ];
        let text = render_graph_text(&r);
        assert_eq!(text.matches("«Общий").count(), 1, "{text}");
        assert!(text.contains("10-20"));
    }
}

#[test]
fn topics_keep_paths_of_members_outside_the_three_representatives() {
    let data = source::build(
        "test",
        "docs",
        (0..4)
            .map(|i| source::InputDocument {
                path: format!("docs/{i}.md"),
                docs_path: format!("{i}.md"),
                title: format!("Документ{i}"),
                module: "features".into(),
                text: format!("# Документ{i}\n\nОписание темы."),
                chunks: vec![],
            })
            .collect(),
    )
    .unwrap();
    let request = GraphRequest {
        operation: Operation::Explore,
        whole_scope: true,
        analysis: Analysis::Topics,
        grouping: Grouping::Modules,
        ..Default::default()
    };
    let mut r = query::execute(&data, &request, || false).unwrap();
    prepare_graph_text(&mut r, MAX_TEXT_CHARS).unwrap();
    let text = render_graph_text(&r);
    for source in &r.sources {
        assert!(
            text.contains(&source.docs_path),
            "{}: {text}",
            source.docs_path
        );
    }
    assert!(text.contains("4 members"));
}

#[test]
fn mix_keeps_result_blocks_related_facts_and_fallback_within_one_limit() {
    let search:SearchOutput=serde_json::from_value(serde_json::json!({
        "results":[{"chunk_id":"primary","path":"docs/a.md","module":".","title":"Документ","heading_chain":"Документ","text":"Исходный результат ".repeat(300),"first_line":1,"last_line":30,"score":0.8,"score_kind":"cosine"}],
        "applied":{"results":3,"candidates":30,"rerank_candidates":0,"min_score":0.0,"score_kind":"cosine","clamped":false},"indexing":false,"pending":0
    })).unwrap();
    let mut output = GraphSearchOutput {
        search,
        requested_mode: RetrievalMode::Mix,
        applied_mode: RetrievalMode::Mix,
        fallback_reason: None,
        related: Some(reply(Operation::Graph)),
        graph_results: None,
    };
    prepare_graph_search_text(&mut output).unwrap();
    let text = render_graph_search_text(&output);
    assert!(text.contains("=== RESULT 1 of 1 ==="));
    assert!(text.contains("=== RELATED ==="));
    assert!(text.contains("why:"));
    assert!(text.chars().count() <= MAX_TEXT_CHARS);
    output.related = None;
    output.applied_mode = RetrievalMode::Hybrid;
    output.fallback_reason = Some("Граф устарел".into());
    prepare_graph_search_text(&mut output).unwrap();
    assert!(render_graph_search_text(&output).contains("fallback: mix→hybrid, Граф устарел"));
}
