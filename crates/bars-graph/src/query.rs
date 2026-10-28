use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{Duration, Instant};

/// Ошибка бюджета, в который не помещается даже пустой ответ; поиск mix по ней отвечает без пакета
/// связанных источников (ISS-88).
pub const ENVELOPE_DOES_NOT_FIT: &str =
    "max_bytes/token_budget: не помещается даже envelope ответа";

#[derive(Serialize, Deserialize)]
struct Cursor {
    snapshot: String,
    query: String,
    offset: usize,
}

fn encode(cursor: &Cursor) -> Result<String, GraphError> {
    Ok(serde_json::to_vec(cursor)?
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
fn decode(value: &str) -> Result<Cursor, GraphError> {
    if value.len() > 4096 || !value.len().is_multiple_of(2) {
        return Err(GraphError::CursorMismatch);
    }
    let bytes: Result<Vec<_>, _> = (0..value.len())
        .step_by(2)
        .map(|i| {
            value
                .get(i..i + 2)
                .and_then(|p| u8::from_str_radix(p, 16).ok())
                .ok_or(GraphError::CursorMismatch)
        })
        .collect();
    serde_json::from_slice(&bytes?).map_err(|_| GraphError::CursorMismatch)
}

struct Allowed<'a> {
    nodes: BTreeMap<String, Node>,
    edges: Vec<Edge>,
    evidence: BTreeMap<String, Evidence>,
    sources: BTreeMap<String, &'a SourceMeta>,
    adjacency: BTreeMap<String, Vec<usize>>,
}
fn allowed<'a>(
    data: &'a Dataset,
    scope: &Scope,
    current: Option<&BTreeMap<String, String>>,
) -> Result<Allowed<'a>, GraphError> {
    validate_scope(data, scope)?;
    let sources: BTreeMap<_, _> = data
        .sources
        .iter()
        .filter(|s| scope.allows(&s.meta))
        .filter(|s| current.is_none_or(|m| m.get(&s.meta.path) == Some(&s.meta.hash)))
        .map(|s| (s.meta.reference.id.clone(), &s.meta))
        .collect();
    allowed_sources(data, scope, sources)
}

pub fn validate_scope(data: &Dataset, scope: &Scope) -> Result<(), GraphError> {
    for module in &scope.modules {
        if !data
            .sources
            .iter()
            .map(|s| &s.meta)
            .chain(data.failed_sources.iter())
            .any(|s| &s.module == module)
        {
            return Err(GraphError::InvalidParams(format!(
                "неизвестный модуль `{module}`"
            )));
        }
    }
    for file in &scope.files {
        if !data
            .sources
            .iter()
            .map(|s| &s.meta)
            .chain(data.failed_sources.iter())
            .any(|s| s.docs_path == file.replace('\\', "/"))
        {
            return Err(GraphError::InvalidParams(format!(
                "неизвестный файл `{file}`"
            )));
        }
    }
    Ok(())
}

fn allowed_sources<'a>(
    data: &'a Dataset,
    scope: &Scope,
    sources: BTreeMap<String, &'a SourceMeta>,
) -> Result<Allowed<'a>, GraphError> {
    let evidence: BTreeMap<_, _> = data
        .evidence
        .iter()
        .filter(|e| {
            sources
                .get(&e.document.id)
                .is_some_and(|s| s.reference == e.document && s.hash == e.source_hash)
        })
        .map(|e| (e.id.clone(), e.clone()))
        .collect();
    let nodes = data
        .nodes
        .iter()
        .filter_map(|n| {
            let mut n = n.clone();
            n.source_ids.retain(|id| sources.contains_key(id));
            n.evidence.retain(|id| evidence.contains_key(id));
            if n.source_ids.is_empty() {
                return None;
            }
            if !scope.modules.is_empty() || !scope.files.is_empty() {
                n.aliases.retain(|alias| {
                    n.evidence
                        .iter()
                        .filter_map(|id| evidence.get(id))
                        .filter(|e| e.method == METHOD_VERSION)
                        .filter_map(|e| crate::source::alias_declaration(&e.quote))
                        .any(|(id, declared)| id == n.label && declared == *alias)
                });
            }
            Some((n.reference.id.clone(), n))
        })
        .collect::<BTreeMap<_, _>>();
    let edges = data
        .edges
        .iter()
        .filter_map(|e| {
            if !nodes.contains_key(&e.from) || !nodes.contains_key(&e.to) {
                return None;
            }
            let mut e = e.clone();
            e.evidence.retain(|id| evidence.contains_key(id));
            if e.evidence.is_empty() {
                return None;
            }
            Some(e)
        })
        .collect::<Vec<_>>();
    let mut adjacency = BTreeMap::<String, Vec<usize>>::new();
    for (index, edge) in edges.iter().enumerate() {
        adjacency.entry(edge.from.clone()).or_default().push(index);
        if edge.to != edge.from {
            adjacency.entry(edge.to.clone()).or_default().push(index);
        }
    }
    Ok(Allowed {
        nodes,
        edges,
        evidence,
        sources,
        adjacency,
    })
}

fn resolve(endpoint: &Endpoint, view: &Allowed<'_>) -> Result<Vec<Node>, GraphError> {
    match endpoint {
        Endpoint::Ref(reference) => {
            let node = view
                .nodes
                .get(&reference.id)
                .filter(|n| n.reference == *reference)
                .ok_or_else(|| GraphError::StaleRef(reference.id.clone()))?;
            Ok(vec![node.clone()])
        }
        Endpoint::Name {
            label,
            kind,
            namespace,
        } => {
            if label.trim().is_empty() {
                return Err(GraphError::InvalidParams("пустая метка объекта".into()));
            }
            let exact_label = label.trim();
            let label = exact_label.to_lowercase();
            Ok(view
                .nodes
                .values()
                .filter(|n| {
                    kind.is_none_or(|k| n.reference.kind == k)
                        && namespace.as_ref().is_none_or(|ns| &n.namespace == ns)
                        && ((if n.reference.kind == Kind::Entity
                            && n.namespace.ends_with(":technical")
                        {
                            n.label == exact_label
                        } else {
                            n.label.to_lowercase() == label
                        }) || n.aliases.iter().any(|a| a.to_lowercase() == label))
                })
                .cloned()
                .collect())
        }
    }
}

fn relevant(text: &str, query: &str) -> usize {
    let text = text.to_lowercase();
    query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| w.chars().count() > 2 && text.contains(w))
        .count()
}
fn matches(edge: &Edge, request: &GraphRequest) -> bool {
    (request.predicates.is_empty()
        || request.predicates.contains(&edge.predicate)
        || (request.predicates.contains(&Predicate::Requires)
            && edge.predicate == Predicate::DependsOn
            && edge
                .qualifiers
                .get("kind")
                .is_some_and(|k| k == "precondition")))
        && request.polarity.is_none_or(|p| edge.polarity == p)
        && request
            .qualifiers
            .iter()
            .all(|(key, value)| edge.qualifiers.get(key) == Some(value))
}

struct WalkState {
    visits: u32,
    edges: u32,
    start: Instant,
    reasons: BTreeSet<String>,
}
impl WalkState {
    fn stop(&mut self, budget: &AppliedBudgets) -> bool {
        let reason = if self.start.elapsed().as_millis() >= u128::from(budget.timeout_ms) {
            Some("timeout_ms")
        } else if self.visits >= budget.max_nodes {
            Some("max_nodes")
        } else if self.edges >= budget.max_edges {
            Some("max_edges")
        } else {
            None
        };
        if let Some(reason) = reason {
            self.reasons.insert(reason.into());
            true
        } else {
            false
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn walk(
    view: &Allowed<'_>,
    request: &GraphRequest,
    budget: &AppliedBudgets,
    from: &ObjectRef,
    target: Option<&ObjectRef>,
    impact: bool,
    state: &mut WalkState,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Vec<GraphPath>, GraphError> {
    let mut queue = VecDeque::from([GraphPath {
        nodes: vec![from.clone()],
        steps: vec![],
        evidence: vec![],
        complete: true,
    }]);
    let mut result = Vec::new();
    let depth = request
        .max_hops
        .unwrap_or(budget.max_depth)
        .min(budget.max_depth);
    let max_paths = request
        .max_paths
        .unwrap_or(budget.max_paths)
        .min(budget.max_paths);
    if max_paths == 0 {
        return Err(GraphError::InvalidParams(
            "max_paths должен быть положительным".into(),
        ));
    }
    while let Some(path) = queue.pop_front() {
        if cancelled() {
            return Err(GraphError::Cancelled);
        }
        if state.stop(budget) {
            break;
        }
        state.visits += 1;
        let last = path.nodes.last().unwrap();
        if target.is_some_and(|t| t.id == last.id)
            || ((impact || (target.is_none() && last.kind == Kind::Chunk))
                && !path.steps.is_empty())
        {
            result.push(path.clone());
            if result.len() >= max_paths as usize {
                if !queue.is_empty() || impact {
                    state.reasons.insert("max_paths".into());
                }
                break;
            }
            if target.is_some() {
                continue;
            }
        }
        if path.steps.len() >= depth as usize {
            if view.edges.iter().any(|e| {
                matches(e, request)
                    && (e.from == last.id || e.to == last.id)
                    && !path
                        .nodes
                        .iter()
                        .any(|n| &n.id == if e.from == last.id { &e.to } else { &e.from })
            }) {
                state.reasons.insert("max_depth".into());
            }
            continue;
        }
        let mut candidates = Vec::new();
        for index in view.adjacency.get(&last.id).into_iter().flatten() {
            let edge = &view.edges[*index];
            if cancelled() {
                return Err(GraphError::Cancelled);
            }
            if state.stop(budget) {
                break;
            }
            state.edges += 1;
            if !matches(edge, request) {
                continue;
            }
            if impact {
                if !edge.predicate.impact()
                    || edge.polarity == Polarity::Negative
                    || edge.to != last.id
                {
                    continue;
                }
            } else if (request.path_kind == PathKind::Semantic && !edge.predicate.semantic())
                || (request.path_kind == PathKind::Structural && edge.predicate.semantic())
            {
                continue;
            }
            let reversed = if edge.to == last.id && request.direction != Direction::Outgoing {
                true
            } else if edge.from == last.id && request.direction != Direction::Incoming && !impact {
                false
            } else {
                continue;
            };
            let id = if reversed { &edge.from } else { &edge.to };
            if path.nodes.iter().any(|n| &n.id == id) {
                continue;
            }
            candidates.push((edge, reversed, id));
        }
        if candidates.len() > budget.max_fanout as usize {
            state.reasons.insert("max_fanout".into());
        }
        for (edge, reversed, id) in candidates.into_iter().take(budget.max_fanout as usize) {
            let mut next = path.clone();
            next.nodes.push(view.nodes[id].reference.clone());
            next.steps.push(Traversal {
                edge: edge.id.clone(),
                reversed,
            });
            next.evidence.extend(edge.evidence.iter().cloned());
            next.evidence.sort();
            next.evidence.dedup();
            queue.push_back(next);
        }
    }
    Ok(result)
}

pub fn validate_request(request: &GraphRequest) -> Result<(), GraphError> {
    request.budgets.resolve()?;
    if request.limit == Some(0) {
        return Err(GraphError::InvalidParams(
            "limit должен быть положительным".into(),
        ));
    }
    if request.operation == Operation::Context {
        if !request.roots.is_empty() {
            return Err(GraphError::CapabilityUnavailable(
                "context roots: отбор по корням пока не реализован".into(),
            ));
        }
        if request.context_roles().is_empty() {
            return Err(GraphError::InvalidParams(
                "roles не пересекаются с task_profile".into(),
            ));
        }
    } else if request.task_profile.is_some() {
        return Err(GraphError::InvalidParams(
            "task_profile применим только к context".into(),
        ));
    }
    if request.roots.len() > 2000
        || request.scope.files.len() > 500
        || request.scope.modules.len() > 500
        || request.qualifiers.len() > 32
        || request
            .qualifiers
            .iter()
            .any(|(k, v)| k.len() > 128 || v.len() > 4096)
    {
        return Err(GraphError::InvalidParams(
            "слишком много roots/scope/qualifiers или слишком длинное значение".into(),
        ));
    }
    if request.operation == Operation::Impact && request.direction == Direction::Outgoing {
        return Err(GraphError::InvalidParams(
            "impact выполняет обратный обход; direction=outgoing неприменим".into(),
        ));
    }
    let nonempty = |text: &Option<String>| text.as_ref().is_some_and(|s| !s.trim().is_empty());
    if request
        .query
        .as_ref()
        .is_some_and(|q| q.trim().is_empty() || q.len() > 16384)
        || request
            .topic
            .as_ref()
            .is_some_and(|q| q.trim().is_empty() || q.len() > 16384)
    {
        return Err(GraphError::InvalidParams(
            "query/topic: требуется непустая строка до 16384 байт".into(),
        ));
    }
    if request.change_description.is_some() {
        return Err(GraphError::CapabilityUnavailable("change_description: модельный фильтр не реализован; используйте документированные зависимости roots".into()));
    }
    if request.tokenizer_id.is_some() {
        return Err(GraphError::CapabilityUnavailable(
            "точный tokenizer; без tokenizer_id используется estimated budget mode".into(),
        ));
    }
    if request.token_budget == Some(0) {
        return Err(GraphError::InvalidParams(
            "token_budget должен быть положительным".into(),
        ));
    }
    match request.operation {
        Operation::Relations if nonempty(&request.query) == request.entity_ref.is_some() => {
            return Err(GraphError::InvalidParams(
                "relations: ровно один query/entity_ref".into(),
            ));
        }
        Operation::Paths if request.from.is_none() || request.to.is_none() => {
            return Err(GraphError::InvalidParams("paths: нужны from и to".into()));
        }
        Operation::Impact if request.roots.is_empty() => {
            return Err(GraphError::InvalidParams("impact: нужны roots".into()));
        }
        Operation::Context if !nonempty(&request.query) => {
            return Err(GraphError::InvalidParams("context: нужен query".into()));
        }
        Operation::Explore if nonempty(&request.topic) == request.whole_scope => {
            return Err(GraphError::InvalidParams(
                "explore: topic либо whole_scope=true".into(),
            ));
        }
        Operation::Graph if request.local && request.roots.is_empty() => {
            return Err(GraphError::InvalidParams("local graph: нужны roots".into()));
        }
        _ => {}
    }
    Ok(())
}

fn communities(
    view: &Allowed<'_>,
    budget: &AppliedBudgets,
    state: &mut WalkState,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<BTreeMap<String, Vec<String>>, GraphError> {
    let mut pairs = BTreeMap::<(String, String), f64>::new();
    for edge in &view.edges {
        if cancelled() {
            return Err(GraphError::Cancelled);
        }
        if edge.polarity == Polarity::Negative
            || !(edge.predicate == Predicate::LinksTo
                || edge.predicate.semantic()
                || (edge.predicate == Predicate::SimilarTo && edge.score.is_some_and(|s| s >= 0.7)))
        {
            continue;
        }
        if state.stop(budget) {
            break;
        }
        state.edges += 1;
        let weight = if edge.predicate.semantic() { 2.0 } else { 1.0 };
        for from in &view.nodes[&edge.from].source_ids {
            for to in &view.nodes[&edge.to].source_ids {
                if from == to {
                    continue;
                }
                let key = if from < to {
                    (from.clone(), to.clone())
                } else {
                    (to.clone(), from.clone())
                };
                pairs
                    .entry(key)
                    .and_modify(|v| *v = v.max(weight))
                    .or_insert(weight);
            }
        }
    }
    let mut neighbors = BTreeMap::<String, Vec<(String, f64)>>::new();
    let mut degrees = BTreeMap::<String, f64>::new();
    let mut labels = BTreeMap::<String, String>::new();
    for id in view.sources.keys().take(budget.max_nodes as usize) {
        labels.insert(id.clone(), id.clone());
        degrees.insert(id.clone(), 0.0);
    }
    if labels.len() < view.sources.len() {
        state.reasons.insert("max_nodes".into());
    }
    for ((a, b), w) in pairs {
        if !labels.contains_key(&a) || !labels.contains_key(&b) {
            continue;
        }
        neighbors.entry(a.clone()).or_default().push((b.clone(), w));
        neighbors.entry(b.clone()).or_default().push((a.clone(), w));
        *degrees.get_mut(&a).unwrap() += w;
        *degrees.get_mut(&b).unwrap() += w;
    }
    let total: f64 = degrees.values().sum();
    let mut volumes = degrees.clone();
    // Детерминированные локальные шаги modularity; веса пар не зависят от числа дубликатов.
    if total > 0.0 {
        'passes: for _ in 0..12 {
            let mut changed = false;
            for id in degrees.keys() {
                if cancelled() {
                    return Err(GraphError::Cancelled);
                }
                if state.start.elapsed().as_millis() >= u128::from(budget.timeout_ms) {
                    state.reasons.insert("timeout_ms".into());
                    break 'passes;
                }
                let old = labels[id].clone();
                let degree = degrees[id];
                *volumes.get_mut(&old).unwrap() -= degree;
                let mut weights = BTreeMap::<String, f64>::new();
                for (other, w) in neighbors.get(id).into_iter().flatten() {
                    *weights.entry(labels[other].clone()).or_default() += w;
                }
                let value = |label: &String| {
                    weights.get(label).copied().unwrap_or(0.0)
                        - degree * volumes.get(label).copied().unwrap_or(0.0) / total
                };
                let mut best = old.clone();
                let mut gain = value(&best);
                for label in weights.keys() {
                    let score = value(label);
                    if score > gain + 1e-12 {
                        best = label.clone();
                        gain = score;
                    }
                }
                changed |= best != old;
                *volumes.entry(best.clone()).or_default() += degree;
                labels.insert(id.clone(), best);
            }
            if !changed {
                break;
            }
        }
    }
    let mut result = BTreeMap::<String, Vec<String>>::new();
    for (id, label) in labels {
        result.entry(label).or_default().push(id);
    }
    Ok(result)
}

/// Все операции используют заранее построенный снимок. Никаких файлов, моделей и записей.
pub fn execute(
    data: &Dataset,
    request: &GraphRequest,
    mut cancelled: impl FnMut() -> bool,
) -> Result<GraphReply, GraphError> {
    execute_ranked(
        data,
        request,
        &BTreeMap::new(),
        Duration::ZERO,
        &mut cancelled,
    )
}

/// Внутренние оценки источников от существующего retrieval. Они не утверждают отношения.
pub fn execute_ranked(
    data: &Dataset,
    request: &GraphRequest,
    evidence_scores: &BTreeMap<String, f32>,
    pre_elapsed: Duration,
    cancelled: impl FnMut() -> bool,
) -> Result<GraphReply, GraphError> {
    execute_overlay(
        data,
        request,
        evidence_scores,
        pre_elapsed,
        &QueryOverlay::default(),
        cancelled,
    )
}

/// Данные одного запроса, не изменяющие общий снимок.
#[derive(Default)]
pub struct QueryOverlay {
    pub current_sources: Option<BTreeMap<String, String>>,
    pub warnings: Vec<String>,
}

pub fn execute_overlay(
    data: &Dataset,
    request: &GraphRequest,
    evidence_scores: &BTreeMap<String, f32>,
    pre_elapsed: Duration,
    overlay: &QueryOverlay,
    mut cancelled: impl FnMut() -> bool,
) -> Result<GraphReply, GraphError> {
    validate_request(request)?;
    let started = Instant::now()
        .checked_sub(pre_elapsed)
        .unwrap_or_else(Instant::now);
    let (mut budget, mut clamped) = request.budgets.resolve()?;
    let requested_limit = request.limit.unwrap_or(50);
    if requested_limit == 0 {
        return Err(GraphError::InvalidParams(
            "limit должен быть положительным".into(),
        ));
    }
    clamped |= requested_limit > 500;
    let limit = requested_limit.min(500);
    if request
        .snapshot
        .as_ref()
        .is_some_and(|s| s != &data.snapshot)
    {
        return Err(GraphError::SnapshotExpired);
    }
    if data.snapshot.is_empty() {
        return Err(GraphError::NotReady(
            LayerState::NotBuilt.not_ready_reason().into(),
        ));
    }
    let needs_semantic = request.operation == Operation::Impact
        || (request.operation == Operation::Paths && request.path_kind != PathKind::Structural)
        || (request.operation == Operation::Relations
            && (request.predicates.is_empty() || request.predicates.iter().any(|p| p.semantic())));
    if needs_semantic && !matches!(data.semantic_state, LayerState::Ready | LayerState::Partial) {
        return Err(GraphError::NotReady(
            "смысловой слой графа не построен".into(),
        ));
    }
    if let Some(paths) = request.max_paths {
        if paths == 0 {
            return Err(GraphError::InvalidParams(
                "max_paths должен быть положительным".into(),
            ));
        }
        clamped |= paths > budget.max_paths;
        budget.max_paths = budget.max_paths.min(paths);
    }
    if let Some(hops) = request.max_hops {
        clamped |= hops > budget.max_depth;
        budget.max_depth = budget.max_depth.min(hops);
    }
    let view = allowed(data, &request.scope, overlay.current_sources.as_ref())?;
    let excluded = data
        .sources
        .iter()
        .filter(|s| {
            request.scope.allows(&s.meta)
                && overlay
                    .current_sources
                    .as_ref()
                    .is_some_and(|m| m.get(&s.meta.path) != Some(&s.meta.hash))
        })
        .count();
    let mut fingerprint_request = request.clone();
    fingerprint_request.cursor = None;
    fingerprint_request.snapshot = None;
    let fingerprint = blake3::hash(&serde_json::to_vec(&(
        &fingerprint_request,
        evidence_scores,
    ))?)
    .to_hex()
    .to_string();
    let offset = if let Some(cursor) = &request.cursor {
        let cursor = decode(cursor)?;
        if cursor.snapshot != data.snapshot {
            return Err(GraphError::SnapshotExpired);
        }
        if cursor.query != fingerprint {
            return Err(GraphError::CursorMismatch);
        }
        cursor.offset
    } else {
        0
    };
    let mut state = WalkState {
        visits: 0,
        edges: 0,
        start: started,
        reasons: BTreeSet::new(),
    };
    let mut reply = GraphReply {
        layers: data
            .layers
            .iter()
            .map(|(name, layer)| {
                let mut layer = layer.clone();
                layer
                    .source_revisions
                    .retain(|path, _| view.sources.values().any(|s| &s.path == path));
                if !request.scope.modules.is_empty() || !request.scope.files.is_empty() {
                    let failed = data.failed_sources.iter().filter(|s| request.scope.allows(s)).count();
                    layer.processed = view.sources.len();
                    layer.total = view.sources.len() + failed;
                    layer.failed = failed;
                    if name == "similarity" {
                        layer.coverage = LayerCoverage {
                            unit: "chunks".into(), processed: 0,
                            total: view.nodes.values().filter(|n| n.reference.kind == Kind::Chunk).count(),
                            missing: 0, comparisons: 0,
                            reason: Some("kNN рассчитан по проекту; точное покрытие выбранной области неизвестно".into()),
                        };
                    } else {
                        layer.coverage.processed = layer.processed;
                        layer.coverage.total = layer.total;
                        layer.coverage.missing = failed;
                    }
                }
                (name.clone(), layer)
            })
            .collect(),
        schema_version: DTO_VERSION,
        operation: request.operation,
        scope: request.scope.clone(),
        snapshot: data.snapshot.clone(),
        generations: BTreeMap::from([
            ("text_structure".into(), data.snapshot.clone()),
            ("method".into(), METHOD_VERSION.into()),
        ])
        .into_iter()
        .chain(data.layers.iter().filter_map(|(name, layer)| {
            layer.generation.as_ref().map(|g| (name.clone(), g.clone()))
        }))
        .collect(),
        layer_state: data.semantic_state,
        layer_states: BTreeMap::from([
            (
                "structure".into(),
                if data.failed_sources.iter().any(|s| request.scope.allows(s)) {
                    LayerState::Partial
                } else {
                    LayerState::Ready
                },
            ),
            ("semantic".into(), data.semantic_state),
            ("similarity".into(), data.similarity_state),
        ]),
        coverage: Coverage {
            indexed: view.sources.len() + excluded
                + data
                    .failed_sources
                    .iter()
                    .filter(|s| request.scope.allows(s))
                    .count(),
            processed: view.sources.len(),
            failed: excluded + data
                .failed_sources
                .iter()
                .filter(|s| request.scope.allows(s))
                .count(),
            denominator: Some(
                view.sources.len() + excluded
                    + data
                        .failed_sources
                        .iter()
                        .filter(|s| request.scope.allows(s))
                        .count(),
            ),
            method: "indexed_source_documents; semantic completeness unknown".into(),
            semantic_completeness: None,
        },
        requested: request.clone(),
        applied: budget.clone(),
        applied_limit: limit,
        clamped,
        items: vec![],
        nodes: vec![],
        sources: vec![],
        assertions: vec![],
        evidence: vec![],
        truncated: false,
        limit_reasons: vec![],
        next_cursor: None,
        warnings: {
            let mut warnings = data.warnings.clone();
            warnings.extend(overlay.warnings.iter().cloned());
            if request.predicates.contains(&Predicate::Requires) {
                warnings
                    .push("requires устарел: используйте depends_on с kind=precondition".into());
            }
            warnings
        },
        method: METHOD_VERSION.into(),
        elapsed_ms: 0,
        resolution: "resolved".into(),
        candidates: vec![],
        missing_roles: vec![],
        budget_mode: "bytes".into(),
        used_bytes: 0,
    };
    let mut items = Vec::new();
    match request.operation {
        Operation::Relations => {
            let selected = if let Some(reference) = &request.entity_ref {
                resolve(&Endpoint::Ref(reference.clone()), &view)?
            } else {
                let query = request.query.as_ref().unwrap();
                let exact = resolve(
                    &Endpoint::Name {
                        label: query.clone(),
                        kind: Some(Kind::Entity),
                        namespace: None,
                    },
                    &view,
                )?;
                if exact.len() > 1 {
                    reply.resolution = "ambiguous".into();
                    reply.candidates = exact;
                    vec![]
                } else if !exact.is_empty() {
                    exact
                } else {
                    vec![]
                }
            };
            let selected: BTreeSet<_> = selected.iter().map(|n| n.reference.id.as_str()).collect();
            for edge in &view.edges {
                if !edge.predicate.semantic() || !matches(edge, request) {
                    continue;
                }
                if cancelled() {
                    return Err(GraphError::Cancelled);
                }
                if state.stop(&budget) {
                    break;
                }
                state.edges += 1;
                let hit = if !selected.is_empty() {
                    (request.direction != Direction::Incoming
                        && selected.contains(edge.from.as_str()))
                        || (request.direction != Direction::Outgoing
                            && selected.contains(edge.to.as_str()))
                } else {
                    reply.resolution != "ambiguous"
                        && request.query.as_ref().is_some_and(|q| {
                            let text = format!(
                                "{} {} {}",
                                view.nodes[&edge.from].label,
                                view.nodes[&edge.to].label,
                                edge.evidence
                                    .iter()
                                    .filter_map(|id| view.evidence.get(id))
                                    .map(|e| e.quote.as_str())
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            );
                            relevant(&text, q) > 0
                        })
                };
                if hit {
                    items.push(Item::Relation {
                        assertion: edge.clone(),
                    });
                }
            }
        }
        Operation::Paths => {
            let from = resolve(request.from.as_ref().unwrap(), &view)?;
            let to = resolve(request.to.as_ref().unwrap(), &view)?;
            if from.len() != 1 || to.len() != 1 {
                reply.resolution = if from.is_empty() || to.is_empty() {
                    "not_found"
                } else {
                    "ambiguous"
                }
                .into();
                reply.candidates = from.into_iter().chain(to).collect();
            } else {
                items.extend(
                    walk(
                        &view,
                        request,
                        &budget,
                        &from[0].reference,
                        Some(&to[0].reference),
                        false,
                        &mut state,
                        &mut cancelled,
                    )?
                    .into_iter()
                    .map(|path| Item::Path { path }),
                );
            }
        }
        Operation::Impact => {
            for root in &request.roots {
                resolve(&Endpoint::Ref(root.clone()), &view)?;
                for path in walk(
                    &view,
                    request,
                    &budget,
                    root,
                    None,
                    true,
                    &mut state,
                    &mut cancelled,
                )? {
                    items.push(Item::Impact {
                        node: path.nodes.last().unwrap().clone(),
                        direct: path.steps.len() == 1,
                        path,
                    });
                }
            }
        }
        Operation::Graph => {
            if request.include_paths {
                if !request.local {
                    return Err(GraphError::InvalidParams(
                        "include_paths требует local=true и roots".into(),
                    ));
                }
                for root in &request.roots {
                    resolve(&Endpoint::Ref(root.clone()), &view)?;
                    for path in walk(
                        &view,
                        request,
                        &budget,
                        root,
                        None,
                        false,
                        &mut state,
                        &mut cancelled,
                    )? {
                        items.push(Item::Path { path });
                    }
                }
            } else {
                let mut ids: BTreeSet<String> = if request.local {
                    request
                        .roots
                        .iter()
                        .map(|r| resolve(&Endpoint::Ref(r.clone()), &view).map(|_| r.id.clone()))
                        .collect::<Result<_, _>>()?
                } else {
                    view.nodes.keys().cloned().collect()
                };
                if request.local {
                    let mut frontier = ids.clone();
                    for _ in 0..budget.max_depth {
                        let mut next = BTreeSet::new();
                        for id in &frontier {
                            if state.stop(&budget) {
                                break;
                            }
                            state.visits += 1;
                            let mut fanout = 0;
                            for index in view.adjacency.get(id).into_iter().flatten() {
                                let edge = &view.edges[*index];
                                if cancelled() {
                                    return Err(GraphError::Cancelled);
                                }
                                if state.stop(&budget) {
                                    break;
                                }
                                state.edges += 1;
                                if !matches(edge, request) {
                                    continue;
                                }
                                let candidate = if edge.from == *id
                                    && request.direction != Direction::Incoming
                                {
                                    Some(&edge.to)
                                } else if edge.to == *id && request.direction != Direction::Outgoing
                                {
                                    Some(&edge.from)
                                } else {
                                    None
                                };
                                if let Some(candidate) = candidate {
                                    fanout += 1;
                                    if fanout > budget.max_fanout {
                                        state.reasons.insert("max_fanout".into());
                                        break;
                                    }
                                    if !ids.contains(candidate) {
                                        next.insert(candidate.clone());
                                    }
                                }
                            }
                        }
                        if ids.len() + next.len() > budget.max_nodes as usize {
                            state.reasons.insert("max_nodes".into());
                        }
                        let room = (budget.max_nodes as usize).saturating_sub(ids.len());
                        frontier = next.into_iter().take(room).collect();
                        ids.extend(frontier.iter().cloned());
                    }
                }
                let kind_allowed = |kind| match request.view {
                    View::Documents => kind == Kind::Document,
                    View::Sections => matches!(kind, Kind::Document | Kind::Section),
                    View::Chunks => matches!(kind, Kind::Document | Kind::Chunk),
                    View::Entities => kind == Kind::Entity,
                    View::Mixed => true,
                };
                ids.retain(|id| kind_allowed(view.nodes[id].reference.kind));
                let mut projected = BTreeMap::<String, Edge>::new();
                for edge in &view.edges {
                    if cancelled() {
                        return Err(GraphError::Cancelled);
                    }
                    if state.start.elapsed().as_millis() >= u128::from(budget.timeout_ms) {
                        state.reasons.insert("timeout_ms".into());
                        break;
                    }
                    if !matches(edge, request) {
                        continue;
                    }
                    let mut edge = edge.clone();
                    if request.view == View::Documents && edge.predicate == Predicate::LinksTo {
                        let (from, to) = (&view.nodes[&edge.from], &view.nodes[&edge.to]);
                        if from.source_ids.len() != 1 || to.source_ids.len() != 1 {
                            continue;
                        }
                        edge.from = from.source_ids[0].clone();
                        edge.to = to.source_ids[0].clone();
                        if edge.from == edge.to {
                            continue;
                        }
                        // Сохранять независимые evidence; не выдавать число ссылок за число источников.
                        edge.id = format!("projection:{}:{}", edge.from, edge.to);
                    }
                    if ids.contains(&edge.from) && ids.contains(&edge.to) {
                        if let Some(previous) = projected.get_mut(&edge.id) {
                            previous.evidence.extend(edge.evidence);
                            previous.evidence.sort();
                            previous.evidence.dedup();
                        } else {
                            projected.insert(edge.id.clone(), edge);
                        }
                    }
                }
                // Ребро несёт endpoints в нормализованном словаре даже на первой странице.
                let mut represented = BTreeSet::new();
                for edge in projected.into_values() {
                    represented.insert(edge.from.clone());
                    represented.insert(edge.to.clone());
                    items.push(Item::Edge { edge });
                }
                items.extend(
                    ids.iter()
                        .filter(|id| !represented.contains(*id))
                        .map(|id| Item::Node {
                            node: view.nodes[id].clone(),
                        }),
                );
            }
        }
        Operation::Context => {
            let roles = request.context_roles();
            let query = request.query.as_ref().unwrap();
            let mut scanned = Vec::new();
            for node in view
                .nodes
                .values()
                .filter(|n| n.roles.iter().any(|role| roles.contains(role)))
            {
                if cancelled() {
                    return Err(GraphError::Cancelled);
                }
                if state.stop(&budget) {
                    break;
                }
                state.visits += 1;
                scanned.push(node);
            }
            for role in roles {
                let mut candidates = Vec::new();
                for node in scanned.iter().filter(|n| n.roles.contains(&role)) {
                    for ev in node.evidence.iter().filter_map(|id| view.evidence.get(id)) {
                        if cancelled() {
                            return Err(GraphError::Cancelled);
                        }
                        if state.start.elapsed().as_millis() >= u128::from(budget.timeout_ms) {
                            state.reasons.insert("timeout_ms".into());
                            break;
                        }
                        let score = evidence_scores
                            .get(&ev.id)
                            .filter(|score| score.is_finite())
                            .map_or(0, |score| (score.max(0.0) * 1000.0) as usize)
                            + relevant(&ev.quote, query);
                        if score > 0 {
                            candidates.push((score, ev.id.clone()));
                        }
                    }
                }
                candidates.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                candidates.dedup_by(|a, b| a.1 == b.1);
                let evidence: Vec<_> = candidates.into_iter().take(5).map(|(_, id)| id).collect();
                if evidence.is_empty() {
                    reply.missing_roles.push(role);
                } else {
                    items.push(Item::RoleGroup { role, evidence });
                }
            }
            reply.budget_mode = "estimated".into();
            if let Some(tokens) = request.token_budget {
                budget.max_bytes = budget.max_bytes.min(tokens.saturating_mul(3));
                reply.applied.max_bytes = budget.max_bytes;
            }
        }
        Operation::Explore => match request.analysis {
            Analysis::Topics => {
                let mut groups = BTreeMap::<String, Vec<&SourceMeta>>::new();
                let membership = if request.grouping == Grouping::Communities {
                    communities(&view, &budget, &mut state, &mut cancelled)?
                        .into_iter()
                        .flat_map(|(label, ids)| ids.into_iter().map(move |id| (id, label.clone())))
                        .collect::<BTreeMap<_, _>>()
                } else {
                    BTreeMap::new()
                };
                for source in view.sources.values() {
                    if cancelled() {
                        return Err(GraphError::Cancelled);
                    }
                    if state.visits >= budget.max_nodes {
                        state.reasons.insert("max_nodes".into());
                        break;
                    }
                    if state.start.elapsed().as_millis() >= u128::from(budget.timeout_ms) {
                        state.reasons.insert("timeout_ms".into());
                        break;
                    }
                    state.visits += 1;
                    if request.topic.as_ref().is_some_and(|topic| {
                        relevant(&source.title, topic) == 0
                            && !view.evidence.values().any(|e| {
                                e.document.id == source.reference.id
                                    && relevant(&e.quote, topic) > 0
                            })
                    }) {
                        continue;
                    }
                    let label = if request.grouping == Grouping::Modules {
                        source.module.clone()
                    } else {
                        match membership.get(&source.reference.id) {
                            Some(label) => label.clone(),
                            None => continue,
                        }
                    };
                    groups.entry(label).or_default().push(source);
                }
                for (key, members) in groups {
                    let labels: Vec<_> = members.iter().take(3).map(|s| s.title.as_str()).collect();
                    let label = if request.grouping == Grouping::Modules {
                        key.clone()
                    } else {
                        labels.join(" / ")
                    };
                    let member_ids: Vec<_> =
                        members.iter().map(|s| s.reference.id.as_str()).collect();
                    let id = blake3::hash(&serde_json::to_vec(&(
                        "communities-v1",
                        request.grouping,
                        member_ids,
                    ))?)
                    .to_hex()
                    .to_string();
                    items.push(Item::Topic {
                        id,
                        label,
                        representatives: members
                            .iter()
                            .take(3)
                            .map(|s| s.reference.clone())
                            .collect(),
                        members: members.iter().map(|s| s.reference.clone()).collect(),
                    });
                }
                reply.method = if request.grouping==Grouping::Modules {"explicit_module_groups"} else {"document_communities_modularity_v1; title representatives; no generated summaries"}.into();
            }
            Analysis::Connectivity => {
                for node in view
                    .nodes
                    .values()
                    .filter(|n| n.reference.kind == Kind::Document)
                {
                    if cancelled() {
                        return Err(GraphError::Cancelled);
                    }
                    if state.stop(&budget) {
                        break;
                    }
                    state.visits += 1;
                    let connected = view
                        .edges
                        .iter()
                        .filter(|e| e.predicate == Predicate::LinksTo)
                        .any(|e| {
                            view.nodes[&e.from].source_ids.contains(&node.reference.id)
                                || view.nodes[&e.to].source_ids.contains(&node.reference.id)
                        });
                    if !connected {
                        items.push(Item::Audit { kind: "structural_isolate".into(), node: Some(node.reference.clone()), evidence: node.evidence.clone(), detail: "Нет индексированных Markdown-ссылок; смысловая изоляция не доказана".into() });
                    }
                }
                for link in &data.broken_links {
                    if view.nodes.contains_key(&link.source)
                        && view.evidence.contains_key(&link.evidence)
                    {
                        items.push(Item::Audit {
                            kind: link.reason.clone(),
                            node: Some(view.nodes[&link.source].reference.clone()),
                            evidence: vec![link.evidence.clone()],
                            detail: "Ссылка не разрешена в индексированной документации".into(),
                        });
                    }
                }
            }
            Analysis::Coverage => {
                for node in view
                    .nodes
                    .values()
                    .filter(|n| n.reference.kind == Kind::Entity)
                {
                    if cancelled() {
                        return Err(GraphError::Cancelled);
                    }
                    if state.stop(&budget) {
                        break;
                    }
                    state.visits += 1;
                    if !view.edges.iter().any(|e| {
                        e.predicate == Predicate::Verifies
                            && e.to == node.reference.id
                            && e.polarity == Polarity::Positive
                    }) {
                        items.push(Item::Audit { kind: "verification_not_found_candidate".into(), node: Some(node.reference.clone()), evidence: node.evidence.clone(), detail: "В обработанных явными правилами отношениях проверка не найдена; её отсутствие не доказано".into() });
                    }
                }
            }
            Analysis::ConflictCandidates => {
                let mut groups = BTreeMap::<String, Vec<&Edge>>::new();
                for edge in view.edges.iter().filter(|e| e.predicate.semantic()) {
                    if cancelled() {
                        return Err(GraphError::Cancelled);
                    }
                    if state.stop(&budget) {
                        break;
                    }
                    state.edges += 1;
                    let key = serde_json::to_string(&(
                        &edge.from,
                        &edge.to,
                        edge.predicate,
                        &edge.qualifiers,
                    ))?;
                    groups.entry(key).or_default().push(edge);
                }
                for edges in groups.into_values() {
                    if !edges.iter().any(|e| e.polarity == Polarity::Positive)
                        || !edges.iter().any(|e| e.polarity == Polarity::Negative)
                    {
                        continue;
                    }
                    let evidence: BTreeSet<_> = edges
                        .iter()
                        .flat_map(|e| e.evidence.iter().cloned())
                        .collect();
                    items.push(Item::Conflict {
                        assertions: edges.iter().map(|e| e.id.clone()).collect(),
                        evidence: evidence.into_iter().collect(),
                        candidate: true,
                    });
                }
            }
        },
    }
    if cancelled() {
        return Err(GraphError::Cancelled);
    }
    if offset > items.len() {
        return Err(GraphError::CursorMismatch);
    }
    let total = items.len();
    let cap = (limit as usize).min(budget.max_nodes as usize);
    reply.items = items.into_iter().skip(offset).take(cap).collect();
    if reply.candidates.len() > cap {
        reply.candidates.truncate(cap);
        reply.limit_reasons.push("max_nodes".into());
    }
    if total > offset + reply.items.len() {
        reply.truncated = true;
        reply.limit_reasons.push("limit".into());
        if state.reasons.is_empty()
            && request.operation != Operation::Paths
            && request.operation != Operation::Impact
        {
            reply.next_cursor = Some(encode(&Cursor {
                snapshot: data.snapshot.clone(),
                query: fingerprint.clone(),
                offset: offset + reply.items.len(),
            })?);
        }
    }
    reply.limit_reasons.extend(state.reasons);
    reply.truncated |= !reply.limit_reasons.is_empty();
    reply.elapsed_ms = started.elapsed().as_millis() as u64;
    loop {
        materialize(&mut reply, &view);
        if request.operation == Operation::Context {
            reply.missing_roles = request.context_roles().into_iter().filter(|role| {
                !reply.items.iter().any(|item| matches!(item, Item::RoleGroup { role: actual, .. } if actual == role))
            }).collect();
        }
        reply.used_bytes = 0;
        for _ in 0..4 {
            let bytes = serde_json::to_vec(&reply)?.len();
            if bytes == reply.used_bytes {
                break;
            }
            reply.used_bytes = bytes;
        }
        if reply.used_bytes <= budget.max_bytes as usize
            && reply.nodes.len() <= budget.max_nodes as usize
            && reply.assertions.len() <= budget.max_edges as usize
        {
            break;
        }
        let reason = if reply.nodes.len() > budget.max_nodes as usize {
            "max_nodes"
        } else if reply.assertions.len() > budget.max_edges as usize {
            "max_edges"
        } else {
            "max_bytes"
        };
        let trimmed_role = if request.operation == Operation::Context && reason == "max_bytes" {
            let largest = reply
                .items
                .iter_mut()
                .filter_map(|item| match item {
                    Item::RoleGroup { evidence, .. } if evidence.len() > 1 => Some(evidence),
                    _ => None,
                })
                .max_by_key(|evidence| evidence.len());
            largest.is_some_and(|evidence| evidence.pop().is_some())
        } else {
            false
        };
        if !trimmed_role && reply.items.pop().is_none() && reply.candidates.pop().is_none() {
            return Err(GraphError::InvalidParams(ENVELOPE_DOES_NOT_FIT.into()));
        }
        reply.truncated = true;
        if !reply.limit_reasons.iter().any(|r| r == reason) {
            reply.limit_reasons.push(reason.into());
        }
        reply.next_cursor = if reply.items.is_empty()
            || matches!(
                request.operation,
                Operation::Paths | Operation::Impact | Operation::Context
            )
            || !reply
                .limit_reasons
                .iter()
                .all(|r| r == "max_bytes" || r == "limit")
        {
            None
        } else {
            Some(encode(&Cursor {
                snapshot: data.snapshot.clone(),
                query: fingerprint.clone(),
                offset: offset + reply.items.len(),
            })?)
        };
    }
    Ok(reply)
}

fn materialize(reply: &mut GraphReply, view: &Allowed<'_>) {
    let mut nodes = BTreeSet::new();
    let mut edges = BTreeMap::new();
    let mut proof = BTreeSet::new();
    let mut add_path = |path: &GraphPath| {
        nodes.extend(path.nodes.iter().map(|r| r.id.clone()));
        proof.extend(path.evidence.iter().cloned());
        for step in &path.steps {
            if let Some(edge) = view.edges.iter().find(|e| e.id == step.edge) {
                edges.insert(edge.id.clone(), edge.clone());
            }
        }
    };
    for item in &reply.items {
        match item {
            Item::Path { path } | Item::Impact { path, .. } => add_path(path),
            _ => {}
        }
    }
    for item in &reply.items {
        match item {
            Item::Node { node } => {
                nodes.insert(node.reference.id.clone());
            }
            Item::Edge { edge } | Item::Relation { assertion: edge } => {
                edges.insert(edge.id.clone(), edge.clone());
            }
            Item::RoleGroup { evidence, .. }
            | Item::Audit { evidence, .. }
            | Item::Conflict { evidence, .. } => proof.extend(evidence.iter().cloned()),
            Item::Topic { members, .. } => nodes.extend(members.iter().map(|r| r.id.clone())),
            _ => {}
        }
    }
    for node in &reply.candidates {
        nodes.insert(node.reference.id.clone());
    }
    for edge in edges.values() {
        nodes.insert(edge.from.clone());
        nodes.insert(edge.to.clone());
        proof.extend(edge.evidence.iter().cloned());
    }
    for id in &nodes {
        if let Some(n) = view.nodes.get(id) {
            proof.extend(n.evidence.iter().cloned());
        }
    }
    reply.nodes = nodes
        .iter()
        .filter_map(|id| view.nodes.get(id).cloned())
        .collect();
    reply.assertions = edges.into_values().collect();
    reply.evidence = proof
        .iter()
        .filter_map(|id| view.evidence.get(id).cloned())
        .collect();
    let sources: BTreeSet<_> = reply
        .evidence
        .iter()
        .map(|e| &e.document.id)
        .chain(reply.nodes.iter().flat_map(|n| n.source_ids.iter()))
        .collect();
    reply.sources = sources
        .into_iter()
        .filter_map(|id| view.sources.get(id).map(|s| (*s).clone()))
        .collect();
    for layer in reply.layers.values_mut() {
        layer
            .source_revisions
            .retain(|path, _| reply.sources.iter().any(|s| &s.path == path));
    }
}
