//! Вид завершённого поиска: отбор из всей области, без клиентского kNN и моделей.
use crate::physical::*;
use crate::{Dataset, GraphError, Kind, LayerState, Predicate, Scope};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[cfg(test)]
#[path = "query_slice_tests.rs"]
mod tests;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryHit {
    pub path: String,
    pub title: String,
    pub heading: String,
    pub first: usize,
    pub last: usize,
    pub score: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryScoreKind {
    Cosine,
    Rerank,
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuerySliceRequest {
    pub id: String,
    pub text: String,
    pub scope: Scope,
    pub hits: Vec<QueryHit>,
    pub score_kind: QueryScoreKind,
    pub depth: usize,
    pub fanout: usize,
    pub min_strength: f32,
    pub max_nodes: usize,
    pub max_edges: usize,
    pub max_bytes: usize,
    pub snapshot: Option<String>,
}

impl QuerySliceRequest {
    pub fn validate(&self) -> Result<(), GraphError> {
        if self.id.is_empty()
            || self.text.len() > 32_768
            || self.hits.len() > 500
            || self.depth > 2
            || !(1..=20).contains(&self.fanout)
            || !(1..=500).contains(&self.max_nodes)
            || !(1..=2000).contains(&self.max_edges)
            || !(1024..=524_288).contains(&self.max_bytes)
            || !self.min_strength.is_finite()
            || !(0.0..=1.0).contains(&self.min_strength)
            || self
                .hits
                .iter()
                .any(|h| h.score.is_some_and(|s| !s.is_finite()))
        {
            return Err(GraphError::InvalidParams(
                "неверные параметры вида запроса".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuerySliceResult {
    pub path: String,
    pub title: String,
    pub rank: usize,
    pub node: Option<usize>,
    pub score: Option<f32>,
    pub fragments: Vec<QueryHit>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryNode {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryResultEdge {
    pub to: usize,
    pub raw_score: Option<f32>,
    pub score_kind: QueryScoreKind,
    pub strength: f32,
    pub neutral: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryGraphSlice {
    pub physical: PhysicalGraphSlice,
    pub fingerprint: String,
    pub query_node: QueryNode,
    pub query_edges: Vec<QueryResultEdge>,
    pub id: String,
    pub text: String,
    pub score_kind: QueryScoreKind,
    pub results: Vec<QuerySliceResult>,
    /// Индексы выбранных Rust узлов для каждого режима; смена режима не вызывает IPC.
    pub views: BTreeMap<String, Vec<usize>>,
    pub depth_of: Vec<usize>,
    pub anchor_of: Vec<usize>,
    pub known_nodes: usize,
    pub known_edges: usize,
    pub used_bytes: usize,
}

/// Cosine передаёт bars-core: точный расчёт anchor со ВСЕМИ документами переданной области.
pub fn project(
    data: &Dataset,
    request: &QuerySliceRequest,
    inputs: LayerInputs,
    mut cosine: impl FnMut(&str, &HashSet<String>) -> Result<Vec<DocumentPair>, GraphError>,
) -> Result<QueryGraphSlice, GraphError> {
    request.validate()?;
    if request
        .snapshot
        .as_ref()
        .is_some_and(|s| s != &data.snapshot)
    {
        return Err(GraphError::SnapshotExpired);
    }
    let sources: HashMap<_, _> = data
        .sources
        .iter()
        .filter(|s| request.scope.allows(&s.meta))
        .map(|s| (s.meta.reference.id.clone(), s))
        .collect();
    let by_path: HashMap<_, _> = sources
        .values()
        .map(|s| (s.meta.path.as_str(), s.meta.reference.id.clone()))
        .collect();
    let all: HashSet<String> = sources.keys().cloned().collect();
    let mut results: Vec<QuerySliceResult> = Vec::new();
    for (rank, hit) in request.hits.iter().enumerate() {
        if let Some(r) = results.iter_mut().find(|r| r.path == hit.path) {
            if request.score_kind != QueryScoreKind::None {
                r.score = r.score.into_iter().chain(hit.score).max_by(f32::total_cmp);
            }
            r.fragments.push(hit.clone());
        } else {
            results.push(QuerySliceResult {
                path: hit.path.clone(),
                title: hit.title.clone(),
                rank: rank + 1,
                node: None,
                score: if request.score_kind == QueryScoreKind::None {
                    None
                } else {
                    hit.score
                },
                fragments: vec![hit.clone()],
            });
        }
    }
    let result_ids: Vec<String> = results
        .iter()
        .filter_map(|r| by_path.get(r.path.as_str()).cloned())
        .collect();
    let result_set: HashSet<_> = result_ids.iter().cloned().collect();
    let owners: HashMap<_, _> = data
        .nodes
        .iter()
        .filter_map(|n| {
            if n.reference.kind == Kind::Document {
                Some((n.reference.id.as_str(), n.reference.id.as_str()))
            } else {
                n.source_ids
                    .first()
                    .map(|id| (n.reference.id.as_str(), id.as_str()))
            }
        })
        .collect();
    let lines: HashMap<_, _> = data
        .evidence
        .iter()
        .map(|e| (e.id.as_str(), e.span.first_line))
        .collect();
    let mut links: BTreeMap<(String, String), BTreeSet<usize>> = BTreeMap::new();
    for edge in data
        .edges
        .iter()
        .filter(|e| e.predicate == Predicate::LinksTo)
    {
        if let (Some(&a), Some(&b)) = (owners.get(edge.from.as_str()), owners.get(edge.to.as_str()))
            && a != b
            && all.contains(a)
            && all.contains(b)
        {
            links.entry((a.into(), b.into())).or_default().extend(
                edge.evidence
                    .iter()
                    .filter_map(|e| lines.get(e.as_str()).copied()),
            );
        }
    }
    let mut neighbours: HashMap<String, (Vec<String>, Vec<String>)> = HashMap::new();
    let mut order = result_ids.clone();
    let mut selected: HashSet<String> = order.iter().cloned().collect();
    let mut depths: HashMap<String, usize> = order.iter().map(|id| (id.clone(), 0)).collect();
    let mut anchors: HashMap<String, String> =
        order.iter().map(|id| (id.clone(), id.clone())).collect();
    let mut frontier = order.clone();
    for depth in 1..=request.depth {
        for id in &frontier {
            let mut linked: Vec<String> = links
                .keys()
                .filter_map(|(a, b)| {
                    if a == id {
                        Some(b.clone())
                    } else if b == id {
                        Some(a.clone())
                    } else {
                        None
                    }
                })
                .collect();
            linked.sort_by_key(|other| {
                (
                    !result_set.contains(other),
                    !(links.contains_key(&(id.clone(), other.clone()))
                        && links.contains_key(&(other.clone(), id.clone()))),
                    other.clone(),
                )
            });
            linked.dedup();
            linked.truncate(request.fanout);
            let mut pairs = cosine(id, &all)?;
            pairs.retain(|p| {
                p.cosine.is_finite()
                    && p.cosine >= request.min_strength
                    && p.b != *id
                    && all.contains(&p.b)
            });
            pairs.sort_by(|a, b| {
                (!result_set.contains(&a.b))
                    .cmp(&(!result_set.contains(&b.b)))
                    .then(b.cosine.total_cmp(&a.cosine))
                    .then(a.b.cmp(&b.b))
            });
            let similar = pairs
                .into_iter()
                .map(|p| p.b)
                .take(request.fanout)
                .collect();
            neighbours.insert(id.clone(), (linked, similar));
        }
        let mut next = Vec::new();
        for k in 0..request.fanout * 2 {
            for id in &frontier {
                let (link, sim) = &neighbours[id];
                let Some(other) = link.iter().chain(sim).nth(k) else {
                    continue;
                };
                if selected.insert(other.clone()) {
                    order.push(other.clone());
                    next.push(other.clone());
                    depths.insert(other.clone(), depth);
                    anchors.insert(other.clone(), anchors[id].clone());
                }
            }
        }
        frontier = next;
    }
    let known_nodes = order.len() + 1;
    // Запрос считается узлом; результаты входят раньше окружения и также подчиняются жёсткому пределу.
    let cap = if result_ids.len() >= request.max_edges {
        request.max_edges.min(request.max_nodes.saturating_sub(1))
    } else {
        request.max_nodes.saturating_sub(1)
    };
    order.truncate(cap);
    let selected: HashSet<String> = order.iter().cloned().collect();
    let mut similarities: BTreeMap<(String, String), f32> = BTreeMap::new();
    {
        for id in &order {
            for pair in cosine(id, &selected)? {
                if pair.cosine >= request.min_strength
                    && pair.a != pair.b
                    && selected.contains(&pair.b)
                {
                    let key = if pair.a < pair.b {
                        (pair.a, pair.b)
                    } else {
                        (pair.b, pair.a)
                    };
                    similarities.insert(key, pair.cosine);
                }
            }
        }
    }
    let nodes: Vec<PhysicalNode> = order
        .iter()
        .map(|id| {
            let s = sources[id];
            PhysicalNode {
                id: id.clone(),
                revision: s.meta.reference.revision.clone(),
                kind: PhysicalNodeKind::Document,
                label: s.meta.title.clone(),
                path: s.meta.path.clone(),
                docs_path: s.meta.docs_path.clone(),
                module: s.meta.module.clone(),
                owner: id.clone(),
                chunks: 0,
                ordinal: None,
                first_line: 1,
                last_line: 1,
            }
        })
        .collect();
    let indices: HashMap<_, _> = order
        .iter()
        .enumerate()
        .map(|(i, id)| (id.clone(), i))
        .collect();
    let mut edges = Vec::new();
    for ((a, b), lines) in &links {
        if let (Some(&from), Some(&to)) = (indices.get(a), indices.get(b)) {
            edges.push(PhysicalEdge {
                kind: PhysicalEdgeKind::MarkdownLink,
                from,
                to,
                raw: None,
                strength: 1.0,
                lines: lines.iter().copied().collect(),
            });
        }
    }
    for ((a, b), raw) in similarities {
        edges.push(PhysicalEdge {
            kind: PhysicalEdgeKind::DocumentSimilarity,
            from: indices[&a],
            to: indices[&b],
            raw: Some(raw),
            strength: raw.clamp(0.0, 1.0),
            lines: vec![],
        });
    }
    for result in &mut results {
        result.node = by_path
            .get(result.path.as_str())
            .and_then(|id| indices.get(id))
            .copied();
    }
    let query_edges = results.iter().filter(|r| r.node.is_some()).count();
    let known_edges = edges.len() + result_ids.len();
    edges.sort_by(|a, b| {
        a.from
            .max(a.to)
            .cmp(&b.from.max(b.to))
            .then(b.strength.total_cmp(&a.strength))
            .then(a.from.cmp(&b.from))
            .then(a.to.cmp(&b.to))
    });
    edges.truncate(request.max_edges.saturating_sub(query_edges));
    let mut views = BTreeMap::new();
    for mode in ["links", "meaning", "all", "chunks"] {
        let mut visible: HashSet<String> = result_ids
            .iter()
            .filter(|id| selected.contains(*id))
            .cloned()
            .collect();
        let mut frontier: Vec<_> = visible.iter().cloned().collect();
        for _ in 0..request.depth {
            let mut next = Vec::new();
            for id in frontier {
                if let Some((link, sim)) = neighbours.get(&id) {
                    for other in link
                        .iter()
                        .filter(|_| mode == "links" || mode == "all")
                        .chain(sim.iter().filter(|_| mode == "meaning" || mode == "all"))
                    {
                        if selected.contains(other) && visible.insert(other.clone()) {
                            next.push(other.clone());
                        }
                    }
                }
            }
            frontier = next;
        }
        views.insert(
            mode.into(),
            order
                .iter()
                .enumerate()
                .filter(|(_, id)| visible.contains(*id))
                .map(|(i, _)| i)
                .collect(),
        );
    }
    let layer = |l: Layer| PhysicalLayer {
        state: l.state,
        note: l.note,
    };
    let physical = PhysicalGraphSlice {
        schema_version: 1,
        method: "query-v1".into(),
        snapshot: data.snapshot.clone(),
        scope: request.scope.clone(),
        shown: PhysicalShown {
            documents: nodes.len(),
            chunks: 0,
            edges: edges.len(),
        },
        totals: PhysicalTotals {
            documents: sources.len(),
            chunks: 0,
            links: links.len(),
            document_pairs: 0,
        },
        nodes,
        edges,
        layers: PhysicalLayers {
            markdown_link: layer(inputs.markdown_link),
            document_similarity: layer(inputs.document_similarity),
            chunk_similarity: PhysicalLayer {
                state: LayerState::NotBuilt,
                note: "Вид документов; расчёт чанков — отдельная проекция".into(),
            },
        },
        applied: PhysicalLimits {
            max_documents: request.max_nodes,
            max_chunks: 1,
            max_edges: request.max_edges,
            max_bytes: request.max_bytes,
        },
        truncated: false,
        limit_reasons: vec![],
        warnings: vec![],
        used_bytes: 0,
    };
    let mut reply = QueryGraphSlice {
        physical,
        query_node: QueryNode {
            id: format!("query:{}", request.id),
            label: request.text.clone(),
        },
        query_edges: results
            .iter()
            .filter_map(|r| {
                r.node.map(|to| QueryResultEdge {
                    to,
                    raw_score: r.score,
                    score_kind: request.score_kind,
                    strength: r.score.map_or(0.5, |score| score.clamp(0.0, 1.0)),
                    neutral: r.score.is_none(),
                })
            })
            .collect(),
        fingerprint: blake3::hash(&serde_json::to_vec(&(
            data.snapshot.as_str(),
            "query-v1",
            request,
        ))?)
        .to_hex()
        .to_string(),
        id: request.id.clone(),
        text: request.text.clone(),
        score_kind: request.score_kind,
        results,
        views,
        depth_of: order.iter().map(|id| depths[id]).collect(),
        anchor_of: order
            .iter()
            .map(|id| indices.get(&anchors[id]).copied().unwrap_or(0))
            .collect(),
        known_nodes,
        known_edges,
        used_bytes: 0,
    };
    if known_nodes > request.max_nodes {
        reply.physical.limit_reasons.push("nodes".into());
    }
    if known_edges > request.max_edges {
        reply.physical.limit_reasons.push("edges".into());
    }
    // Размер включает метаданные результатов и узел запроса, а не только физические рёбра.
    loop {
        reply.physical.truncated = !reply.physical.limit_reasons.is_empty();
        reply.physical.shown.documents = reply.physical.nodes.len();
        reply.physical.shown.edges = reply.physical.edges.len();
        let size = serde_json::to_vec(&reply)?.len();
        if size <= request.max_bytes {
            if reply.used_bytes == size {
                break;
            }
            reply.used_bytes = size;
            continue;
        }
        if !reply.physical.limit_reasons.iter().any(|s| s == "bytes") {
            reply.physical.limit_reasons.push("bytes".into());
        }
        if reply.physical.edges.pop().is_some() {
            continue;
        }
        let Some(node) = reply.physical.nodes.pop() else {
            return Err(GraphError::InvalidParams(
                "описание поиска превышает байтовый предел".into(),
            ));
        };
        let removed = reply.physical.nodes.len();
        reply.query_edges.retain(|edge| edge.to < removed);
        reply.depth_of.pop();
        reply.anchor_of.pop();
        for view in reply.views.values_mut() {
            view.retain(|&i| i < removed);
        }
        for r in &mut reply.results {
            if r.path == node.path {
                r.node = None;
            }
        }
    }
    Ok(reply)
}
