use super::*;
use crate::source::{InputDocument, build};
fn corpus(count: usize) -> Dataset {
    build(
        "test",
        "docs",
        (0..count)
            .map(|i| InputDocument {
                path: format!("docs/m/{i}.md"),
                docs_path: format!("m/{i}.md"),
                module: "m".into(),
                title: format!("Документ {i}"),
                text: format!("# Документ {i}\n"),
                chunks: vec![],
            })
            .collect(),
    )
    .unwrap()
}
fn request(data: &Dataset) -> QuerySliceRequest {
    QuerySliceRequest {
        id: "q1".into(),
        text: "индекс".into(),
        scope: Scope::default(),
        hits: vec![QueryHit {
            source_hash: String::new(),
            channel: Default::default(),
            path: data.sources[0].meta.path.clone(),
            title: "Результат".into(),
            heading: "Раздел".into(),
            first: 1,
            last: 2,
            score: Some(0.8),
        }],
        score_kind: QueryScoreKind::Rerank,
        depth: 1,
        fanout: 4,
        min_strength: 0.5,
        max_nodes: 200,
        max_edges: 800,
        max_bytes: 262144,
        snapshot: None,
    }
}
fn inputs() -> LayerInputs {
    let ready = || Layer {
        state: LayerState::Ready,
        note: "готов".into(),
    };
    LayerInputs {
        markdown_link: ready(),
        document_similarity: ready(),
        chunk_similarity: ready(),
    }
}
fn pairs(id: &str, targets: &HashSet<String>) -> Result<Vec<DocumentPair>, GraphError> {
    Ok(targets
        .iter()
        .filter(|b| b.as_str() != id)
        .map(|b| DocumentPair {
            a: id.into(),
            b: b.clone(),
            cosine: 0.9,
        })
        .collect())
}
#[test]
fn scoped_neighbour_beyond_physical_slice_and_all_inner_edges() {
    let data = corpus(610);
    let mut r = request(&data);
    r.scope.files = [0, 608, 609]
        .map(|i| data.sources[i].meta.docs_path.clone())
        .to_vec();
    let ids: HashSet<_> = [0, 608, 609]
        .map(|i| data.sources[i].meta.reference.id.clone())
        .into_iter()
        .collect();
    let output = project(&data, &r, inputs(), |id, targets| {
        assert!(targets.is_subset(&ids));
        pairs(id, targets)
    })
    .unwrap();
    assert_eq!(output.physical.nodes.len(), 3);
    assert_eq!(output.physical.edges.len(), 3);
    assert!(
        output
            .physical
            .nodes
            .iter()
            .any(|n| n.path == data.sources[609].meta.path)
    );
    assert_eq!(output.views["meaning"].len(), 3);
    assert!(!output.physical.truncated);
}
#[test]
fn grouped_results_keep_absolute_score_and_precede_environment() {
    let data = corpus(12);
    let mut r = request(&data);
    let mut second = r.hits[0].clone();
    second.score = Some(1.2);
    second.first = 10;
    r.hits.push(second);
    r.hits.push(QueryHit {
        source_hash: String::new(),
        channel: Default::default(),
        path: data.sources[11].meta.path.clone(),
        ..r.hits[0].clone()
    });
    r.max_nodes = 3;
    let o = project(&data, &r, inputs(), pairs).unwrap();
    assert_eq!(o.results.len(), 2);
    assert_eq!(o.results[0].score, Some(1.2));
    assert_eq!(o.query_edges[0].raw_score, Some(1.2));
    assert_eq!(o.query_edges[0].strength, 1.0);
    assert_eq!(o.query_node.label, r.text);
    assert_eq!(o.results[0].fragments.len(), 2);
    assert_eq!(
        o.results[1].rank, 3,
        "место первого фрагмента в исходной выдаче"
    );
    assert!(o.results.iter().all(|r| r.node.is_some()));
    assert_eq!(o.physical.nodes.len(), 2);
    assert!(o.physical.truncated);
    assert!(o.used_bytes <= r.max_bytes);
    assert_eq!(o.used_bytes, serde_json::to_vec(&o).unwrap().len());
}
#[test]
fn zero_depth_keeps_result_to_result_edges_without_environment() {
    let data = corpus(8);
    let mut r = request(&data);
    r.depth = 0;
    r.hits.push(QueryHit {
        source_hash: String::new(),
        channel: Default::default(),
        path: data.sources[7].meta.path.clone(),
        ..r.hits[0].clone()
    });
    let o = project(&data, &r, inputs(), |id, targets| {
        assert_eq!(targets.len(), 2);
        pairs(id, targets)
    })
    .unwrap();
    assert_eq!(o.physical.nodes.len(), 2);
    assert_eq!(o.physical.edges.len(), 1);
}
#[test]
fn invalid_snapshot_params_empty_results_and_cancellation() {
    let data = corpus(2);
    let mut r = request(&data);
    r.snapshot = Some("expired".into());
    assert!(matches!(
        project(&data, &r, inputs(), |_, _| unreachable!()),
        Err(GraphError::SnapshotExpired)
    ));
    r.snapshot = None;
    r.depth = 3;
    assert!(r.validate().is_err());
    r.depth = 1;
    r.hits[0].score = Some(f32::NAN);
    assert!(r.validate().is_err());
    r.hits[0].score = None;
    assert!(matches!(
        project(&data, &r, inputs(), |_, _| Err(GraphError::Cancelled)),
        Err(GraphError::Cancelled)
    ));
    r.hits.clear();
    let o = project(&data, &r, inputs(), |_, _| unreachable!()).unwrap();
    assert!(o.physical.nodes.is_empty());
    assert_eq!(o.known_nodes, 1);
}
#[test]
fn hard_edge_and_byte_limits_include_query_metadata() {
    let data = corpus(8);
    let mut r = request(&data);
    r.hits = (0..8)
        .map(|i| QueryHit {
            source_hash: String::new(),
            channel: Default::default(),
            path: data.sources[i].meta.path.clone(),
            ..r.hits[0].clone()
        })
        .collect();
    r.max_edges = 2;
    let o = project(&data, &r, inputs(), |_, _| Ok(vec![])).unwrap();
    assert_eq!(o.results.iter().filter(|r| r.node.is_some()).count(), 2);
    assert!(
        o.physical.edges.len() + o.results.iter().filter(|r| r.node.is_some()).count()
            <= r.max_edges
    );
    r.max_bytes = 1024;
    r.text = "я".repeat(1000);
    assert!(matches!(
        project(&data, &r, inputs(), |_, _| Ok(vec![])),
        Err(GraphError::InvalidParams(_))
    ));
}
#[test]
fn depth_two_fanout_and_determinism() {
    let data = corpus(5);
    let mut r = request(&data);
    r.fanout = 1;
    r.depth = 2;
    let a = project(&data, &r, inputs(), pairs).unwrap();
    let b = project(&data, &r, inputs(), pairs).unwrap();
    assert_eq!(a, b);
    assert!(a.physical.nodes.len() <= 3);
    assert_eq!(a.depth_of[0], 0);
}

#[test]
fn byte_truncation_measures_the_complete_serialized_reply() {
    let data = corpus(8);
    let mut r = request(&data);
    r.max_bytes = 4096;
    r.hits = (0..8)
        .map(|i| QueryHit {
            source_hash: String::new(),
            channel: Default::default(),
            path: data.sources[i].meta.path.clone(),
            ..r.hits[0].clone()
        })
        .collect();
    let o = project(&data, &r, inputs(), pairs).unwrap();
    assert!(o.physical.limit_reasons.iter().any(|s| s == "bytes"));
    assert_eq!(o.used_bytes, serde_json::to_vec(&o).unwrap().len());
    assert!(o.used_bytes <= r.max_bytes);
    for view in o.views.values() {
        assert!(view.iter().all(|&i| i < o.physical.nodes.len()));
    }
    assert!(
        o.results
            .iter()
            .filter_map(|r| r.node)
            .all(|i| i < o.physical.nodes.len())
    );
}

#[test]
fn changed_or_missing_results_are_ghosts_without_fresh_edges() {
    let data = corpus(3);
    let mut r = request(&data);
    r.hits[0].source_hash = "old".into();
    let reply = project(&data, &r, inputs(), pairs).unwrap();
    assert_eq!(reply.results[0].status, QuerySourceState::NoGraphObject);
    assert_eq!(reply.physical.nodes.len(), 1);
    assert!(reply.physical.edges.is_empty());
    assert_eq!(reply.query_edges.len(), 1);
    r.hits[0].path = "docs/m/missing.md".into();
    let reply = project(&data, &r, inputs(), pairs).unwrap();
    assert_eq!(reply.results[0].status, QuerySourceState::NoGraphObject);
    assert!(reply.results[0].node.is_some());
    let reply = project_checked(&data, &r, inputs(), pairs, |_| {
        Ok(Some(QuerySourceState::DeletedSinceSearch))
    })
    .unwrap();
    assert_eq!(
        reply.results[0].status,
        QuerySourceState::DeletedSinceSearch
    );
    assert!(reply.physical.edges.is_empty());
}

#[test]
fn related_without_physical_connection_stays_in_list_without_query_score() {
    let data = corpus(3);
    let mut r = request(&data);
    let mut related = r.hits[0].clone();
    related.path = data.sources[1].meta.path.clone();
    related.channel = QueryChannel::Related;
    related.score = Some(0.99);
    r.hits.push(related.clone());
    r.hits.push(related);
    let reply = project(&data, &r, inputs(), |_, _| Ok(vec![])).unwrap();
    assert_eq!(reply.results.len(), 2);
    assert!(reply.results[1].node.is_none());
    assert!(reply.results[1].score.is_none());
    assert_eq!(reply.query_edges.len(), 1);
    let reply = project(&data, &r, inputs(), pairs).unwrap();
    assert!(reply.results[1].node.is_some());
    assert_eq!(reply.query_edges.len(), 1);
    assert!(reply.views["meaning"].contains(&reply.results[1].node.unwrap()));
    assert_eq!(reply.depth_of[reply.results[1].node.unwrap()], 1);
    r.depth = 0;
    let reply = project(&data, &r, inputs(), pairs).unwrap();
    assert!(reply.results[1].node.is_none());
    assert!(
        reply.results[1]
            .map_reason
            .as_ref()
            .unwrap()
            .contains("глубине 0")
    );
}

#[test]
fn omitted_neutral_results_report_the_edge_limit() {
    let data = corpus(3);
    let mut r = request(&data);
    r.hits = (0..3)
        .map(|i| QueryHit {
            path: data.sources[i].meta.path.clone(),
            ..r.hits[0].clone()
        })
        .collect();
    r.score_kind = QueryScoreKind::None;
    r.depth = 0;
    r.max_edges = 1;
    let reply = project(&data, &r, inputs(), pairs).unwrap();
    assert_eq!(reply.query_edges.len(), 1);
    assert_eq!(reply.known_edges, 3);
    assert!(reply.physical.limit_reasons.iter().any(|r| r == "edges"));
    assert!(
        reply.results[1]
            .map_reason
            .as_ref()
            .unwrap()
            .contains("связей")
    );
}

#[test]
fn related_sources_are_environment_not_new_depth_zero_roots() {
    let data = corpus(3);
    let ids: Vec<_> = data
        .sources
        .iter()
        .map(|s| s.meta.reference.id.clone())
        .collect();
    let mut r = request(&data);
    r.hits.push(QueryHit {
        path: data.sources[1].meta.path.clone(),
        channel: QueryChannel::Related,
        score: None,
        ..r.hits[0].clone()
    });
    let chain = |id: &str, targets: &HashSet<String>| {
        Ok(ids
            .iter()
            .enumerate()
            .filter(|(i, b)| {
                targets.contains(*b)
                    && ((*i == 1 && (id == ids[0] || id == ids[2])) || (*i != 1 && id == ids[1]))
            })
            .map(|(_, b)| DocumentPair {
                a: id.into(),
                b: b.clone(),
                cosine: 0.9,
            })
            .collect())
    };
    let one = project(&data, &r, inputs(), chain).unwrap();
    assert_eq!(one.physical.nodes.len(), 2);
    assert_eq!(one.views["meaning"].len(), 2);
    r.depth = 2;
    let two = project(&data, &r, inputs(), chain).unwrap();
    assert_eq!(two.physical.nodes.len(), 3);
    assert_eq!(two.views["meaning"].len(), 3);
}
