use bars_graph::{
    query::{QueryOverlay, execute, execute_overlay},
    source::{InputDocument, build},
    *,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

fn data(documents: &[(&str, &str)]) -> Dataset {
    build(
        "fixture",
        "docs",
        documents
            .iter()
            .map(|(path, text)| InputDocument {
                path: format!("docs/{path}"),
                docs_path: (*path).into(),
                module: ".".into(),
                title: (*path).into(),
                text: (*text).into(),
                chunks: vec![],
            })
            .collect(),
    )
    .unwrap()
}
fn reference(data: &Dataset, label: &str) -> ObjectRef {
    data.nodes
        .iter()
        .find(|n| n.reference.kind == Kind::Entity && n.label == label)
        .unwrap()
        .reference
        .clone()
}
fn paths(data: &Dataset, from: &str, to: &str) -> GraphRequest {
    GraphRequest {
        operation: Operation::Paths,
        from: Some(Endpoint::Ref(reference(data, from))),
        to: Some(Endpoint::Ref(reference(data, to))),
        direction: Direction::Outgoing,
        ..Default::default()
    }
}
fn verify(reply: &GraphReply, data: &Dataset) {
    for item in &reply.items {
        let path = match item {
            Item::Path { path } | Item::Impact { path, .. } => path,
            _ => continue,
        };
        assert!(path.complete);
        assert_eq!(path.nodes.len(), path.steps.len() + 1);
        let mut proofs = BTreeSet::new();
        for (i, step) in path.steps.iter().enumerate() {
            let edge = reply.assertions.iter().find(|e| e.id == step.edge).unwrap();
            let (from, to) = if step.reversed {
                (&edge.to, &edge.from)
            } else {
                (&edge.from, &edge.to)
            };
            assert_eq!(&path.nodes[i].id, from);
            assert_eq!(&path.nodes[i + 1].id, to);
            assert!(!edge.evidence.is_empty());
            for id in &edge.evidence {
                let proof = reply.evidence.iter().find(|e| &e.id == id).unwrap();
                let source = data
                    .sources
                    .iter()
                    .find(|s| s.meta.reference == proof.document)
                    .unwrap();
                assert_eq!(proof.source_hash, source.meta.hash);
                assert_eq!(
                    source.text.get(proof.span.start..proof.span.end),
                    Some(proof.quote.as_str())
                );
                assert!(reply.sources.iter().any(|s| s.reference == proof.document));
                proofs.insert(id.clone());
            }
        }
        assert_eq!(proofs, path.evidence.iter().cloned().collect());
    }
}

#[test]
fn each_step_retains_its_own_evidence_conditions_and_direction() {
    let data = data(&[(
        "a.md",
        "# Связи\n\nЕсли выбран CPU, Т-1 зависит от Т-2.\n\nТ-2 использует Т-3.\n\nТ-3 зависит от Т-1.",
    )]);
    let mut request = paths(&data, "Т-1", "Т-3");
    request.budgets.max_depth = Some(4);
    let result = execute(&data, &request, || false).unwrap();
    assert_eq!(result.items.len(), 1);
    verify(&result, &data);
    assert!(
        result
            .assertions
            .iter()
            .any(|e| e.qualifiers.contains_key("condition"))
    );
    let result = execute(
        &data,
        &GraphRequest {
            operation: Operation::Impact,
            roots: vec![reference(&data, "Т-3")],
            ..Default::default()
        },
        || false,
    )
    .unwrap();
    assert!(
        result
            .items
            .iter()
            .any(|i| matches!(i, Item::Impact { direct: false, .. }))
    );
    verify(&result, &data);
    assert!(
        result.items.iter().all(
            |i| matches!(i, Item::Impact { path, .. } if path.steps.iter().all(|s| s.reversed))
        )
    );
}

#[test]
fn negative_steps_require_explicit_selection_and_do_not_signal_depth_truncation() {
    let data = data(&[("a.md", "# Связи\n\nТ-1 не использует Т-2.")]);
    let mut request = paths(&data, "Т-1", "Т-2");
    let result = execute(&data, &request, || false).unwrap();
    assert!(result.items.is_empty());
    request.budgets.max_depth = Some(0);
    assert!(!execute(&data, &request, || false).unwrap().truncated);
    request.budgets.max_depth = Some(2);
    request.polarity = Some(Polarity::Negative);
    let result = execute(&data, &request, || false).unwrap();
    assert_eq!(result.items.len(), 1);
    verify(&result, &data);
}

#[test]
fn scoped_or_changed_proof_does_not_remove_an_independently_supported_relation() {
    let data = data(&[
        ("a.md", "# Связи\n\nТ-1 зависит от Т-2."),
        ("b.md", "# Связи\n\nТ-1 зависит от Т-2."),
    ]);
    let request = paths(&data, "Т-1", "Т-2");
    let current = BTreeMap::from([(
        data.sources[1].meta.path.clone(),
        data.sources[1].meta.hash.clone(),
    )]);
    let result = execute_overlay(
        &data,
        &request,
        &BTreeMap::new(),
        Duration::ZERO,
        &QueryOverlay {
            current_sources: Some(current),
            ..Default::default()
        },
        || false,
    )
    .unwrap();
    assert_eq!(result.items.len(), 1);
    assert!(result.sources.iter().all(|s| s.docs_path == "b.md"));
    verify(&result, &data);
    let mut scoped = request.clone();
    scoped.scope.files = vec!["a.md".into()];
    let result = execute(&data, &scoped, || false).unwrap();
    assert_eq!(result.items.len(), 1);
    assert!(result.sources.iter().all(|s| s.docs_path == "a.md"));
    verify(&result, &data);
}

#[test]
fn scope_excludes_intermediate_step_instead_of_returning_a_broken_chain() {
    let data = data(&[
        ("a.md", "# Связи\n\nТ-1 зависит от Т-2."),
        ("b.md", "# Связи\n\nТ-2 зависит от Т-3."),
        ("c.md", "# Связи\n\nТ-3 использует Т-4."),
    ]);
    let mut request = paths(&data, "Т-1", "Т-3");
    assert!(!execute(&data, &request, || false).unwrap().items.is_empty());
    request.scope.files = vec!["a.md".into(), "c.md".into()];
    let result = execute(&data, &request, || false).unwrap();
    assert!(result.items.is_empty());
    assert!(
        result
            .evidence
            .iter()
            .all(|e| e.document != data.sources[1].meta.reference)
    );
}

#[test]
fn impact_shares_the_path_limit_across_roots_and_keeps_complete_proof() {
    let data = data(&[(
        "a.md",
        "# Связи\n\nТ-1 зависит от Т-2.\n\nТ-3 зависит от Т-4.",
    )]);
    let result = execute(
        &data,
        &GraphRequest {
            operation: Operation::Impact,
            roots: vec![reference(&data, "Т-2"), reference(&data, "Т-4")],
            max_paths: Some(1),
            ..Default::default()
        },
        || false,
    )
    .unwrap();
    assert_eq!(result.items.len(), 1);
    assert!(result.limit_reasons.iter().any(|r| r == "max_paths"));
    verify(&result, &data);
}

#[test]
fn structural_paths_exclude_similarity_while_mixed_paths_keep_its_origin() {
    let mut data = data(&[("a.md", "# Связи\n\nТ-1 зависит от Т-2.")]);
    let edge = data
        .edges
        .iter_mut()
        .find(|e| e.predicate == Predicate::DependsOn)
        .unwrap();
    edge.predicate = Predicate::SimilarTo;
    edge.origin = Origin::Similarity;
    let mut request = paths(&data, "Т-1", "Т-2");
    request.path_kind = PathKind::Structural;
    assert!(execute(&data, &request, || false).unwrap().items.is_empty());
    request.path_kind = PathKind::Mixed;
    let result = execute(&data, &request, || false).unwrap();
    assert!(!result.items.is_empty());
    assert!(
        result
            .assertions
            .iter()
            .any(|e| e.origin == Origin::Similarity)
    );
    verify(&result, &data);
}

#[test]
fn a_hub_obeys_node_budget_without_claiming_an_unproved_path() {
    let mut text = String::from("# Связи\n\n");
    for i in 2..150 {
        text.push_str(&format!("Т-1 зависит от Т-{i}.\n\n"));
    }
    let data = data(&[("a.md", &text)]);
    let mut request = paths(&data, "Т-1", "Т-149");
    request.budgets.max_nodes = Some(8);
    request.budgets.max_fanout = Some(200);
    let result = execute(&data, &request, || false).unwrap();
    assert!(result.truncated);
    assert!(result.limit_reasons.iter().any(|r| r == "max_nodes"));
    verify(&result, &data);
}
