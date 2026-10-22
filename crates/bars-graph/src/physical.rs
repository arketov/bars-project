//! Физическая проекция графа для окна: документы, чанки и три вида рёбер (`physical-v1`).
//!
//! Чистая логика без векторов и моделей: близость документов приходит готовой от вызывающего слоя
//! (`bars-core`), ссылки и близость чанков берутся из готового набора. Контракт — docs/features/graphrag/physical-graph.md.

use crate::{Dataset, GraphError, Kind, LayerState, Node, Predicate, Scope, SourceDocument};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub const PHYSICAL_METHOD: &str = "physical-v1";
pub const SCHEMA_VERSION: u32 = 1;

/// Пределы одного среза: документы и раскрытые чанки считаются отдельно, чтобы раскрытие не вытесняло показанные документы.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhysicalLimits {
    pub max_documents: usize,
    pub max_chunks: usize,
    pub max_edges: usize,
    pub max_bytes: usize,
}

impl PhysicalLimits {
    /// Начальные значения окна.
    pub const INITIAL: Self = Self {
        max_documents: 500,
        max_chunks: 500,
        max_edges: 4000,
        max_bytes: 1 << 20,
    };
    /// Максимум, до которого окно поднимает пределы командой «Показать ещё».
    pub const MAX: Self = Self {
        max_documents: 2000,
        max_chunks: 2000,
        max_edges: 10_000,
        max_bytes: 2 << 20,
    };

    /// Приводит запрошенные значения к допустимым: ноль и превышение максимума не принимаются молча.
    pub fn validated(self) -> Result<Self, GraphError> {
        let check = |name: &str, value: usize, max: usize| {
            if value == 0 || value > max {
                Err(GraphError::InvalidParams(format!(
                    "{name} должен быть в 1…{max}"
                )))
            } else {
                Ok(())
            }
        };
        check("max_documents", self.max_documents, Self::MAX.max_documents)?;
        check("max_chunks", self.max_chunks, Self::MAX.max_chunks)?;
        check("max_edges", self.max_edges, Self::MAX.max_edges)?;
        check("max_bytes", self.max_bytes, Self::MAX.max_bytes)?;
        Ok(self)
    }
}

impl Default for PhysicalLimits {
    fn default() -> Self {
        Self::INITIAL
    }
}

/// Документ, чанки которого показаны на карте (первые `shown` в порядке нарезки).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpandedDocument {
    pub document: String,
    pub shown: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalRequest {
    #[serde(default)]
    pub scope: Scope,
    #[serde(default)]
    pub limits: PhysicalLimits,
    #[serde(default)]
    pub expanded: Vec<ExpandedDocument>,
    /// Снимок, на который опирается клиент; расхождение — `snapshot_expired`.
    #[serde(default)]
    pub snapshot: Option<String>,
}

/// Пара документов с готовой близостью; `a` и `b` — `ObjectRef.id` документов.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentPair {
    pub a: String,
    pub b: String,
    pub cosine: f32,
}

#[derive(Debug, Clone)]
pub struct Layer {
    pub state: LayerState,
    pub note: String,
}

/// Состояния слоёв, известные вызывающему: проекция лишь добавляет к ним усечение.
#[derive(Debug, Clone)]
pub struct LayerInputs {
    pub markdown_link: Layer,
    pub document_similarity: Layer,
    pub chunk_similarity: Layer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhysicalNodeKind {
    Document,
    Chunk,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhysicalNode {
    pub id: String,
    pub revision: String,
    pub kind: PhysicalNodeKind,
    pub label: String,
    /// Путь от корня проекта: по нему окно открывает документ.
    pub path: String,
    pub docs_path: String,
    pub module: String,
    /// Документ-владелец; у документа — он сам.
    pub owner: String,
    /// У документа — число всех его чанков; у чанка 0.
    pub chunks: usize,
    pub ordinal: Option<usize>,
    pub first_line: usize,
    pub last_line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhysicalEdgeKind {
    MarkdownLink,
    DocumentSimilarity,
    ChunkSimilarity,
}

/// Ребро; концы — индексы в `nodes` того же среза (так срез втрое короче, чем с 64-значными ID).
/// У ссылки `from → to` — направление ссылки.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhysicalEdge {
    pub kind: PhysicalEdgeKind,
    pub from: usize,
    pub to: usize,
    /// Исходный cosine; у ссылки нет.
    pub raw: Option<f32>,
    /// `s`: clamp(cosine, 0, 1) для близости, 1 для ссылки как факта.
    pub strength: f32,
    /// Строки текста источника, где стоит ссылка.
    pub lines: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhysicalLayer {
    pub state: LayerState,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhysicalLayers {
    pub markdown_link: PhysicalLayer,
    pub document_similarity: PhysicalLayer,
    pub chunk_similarity: PhysicalLayer,
}

/// Известные итоги области до применения пределов.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalTotals {
    pub documents: usize,
    pub chunks: usize,
    pub links: usize,
    pub document_pairs: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalShown {
    pub documents: usize,
    pub chunks: usize,
    pub edges: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhysicalGraphSlice {
    pub schema_version: u32,
    pub method: String,
    pub snapshot: String,
    pub scope: Scope,
    pub nodes: Vec<PhysicalNode>,
    pub edges: Vec<PhysicalEdge>,
    pub layers: PhysicalLayers,
    pub totals: PhysicalTotals,
    pub shown: PhysicalShown,
    pub applied: PhysicalLimits,
    pub truncated: bool,
    pub limit_reasons: Vec<String>,
    pub warnings: Vec<String>,
    pub used_bytes: usize,
}

/// Ребро до назначения индексов: концы — `ObjectRef.id`.
struct RawEdge {
    kind: PhysicalEdgeKind,
    from: String,
    to: String,
    raw: Option<f32>,
    strength: f32,
    lines: Vec<usize>,
}

fn pair_key(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

/// Детерминированный обход модулей по кругу, внутри модуля — по пути от корня документации.
fn round_robin<'a>(sources: &[&'a SourceDocument], limit: usize) -> Vec<&'a SourceDocument> {
    let mut groups: BTreeMap<&str, Vec<&SourceDocument>> = BTreeMap::new();
    for source in sources {
        groups.entry(&source.meta.module).or_default().push(source);
    }
    for list in groups.values_mut() {
        list.sort_by(|a, b| a.meta.docs_path.cmp(&b.meta.docs_path));
    }
    let lists: Vec<_> = groups.into_values().collect();
    let mut picked = Vec::new();
    let mut round = 0;
    while picked.len() < limit {
        let mut any = false;
        for list in &lists {
            if round < list.len() && picked.len() < limit {
                picked.push(list[round]);
                any = true;
            }
        }
        if !any {
            break;
        }
        round += 1;
    }
    picked
}

fn kind_label(kind: PhysicalEdgeKind) -> &'static str {
    match kind {
        PhysicalEdgeKind::MarkdownLink => "ссылок",
        PhysicalEdgeKind::DocumentSimilarity => "связей близости документов",
        PhysicalEdgeKind::ChunkSimilarity => "связей близости чанков",
    }
}

/// Строит срез: документы по кругу модулей, раскрытые чанки, три вида рёбер с приоритетом «ссылки, близость документов, близость чанков».
pub fn project(
    data: &Dataset,
    pairs: &[DocumentPair],
    request: &PhysicalRequest,
    inputs: LayerInputs,
) -> Result<PhysicalGraphSlice, GraphError> {
    let limits = request.limits.validated()?;
    if let Some(snapshot) = &request.snapshot
        && snapshot != &data.snapshot
    {
        return Err(GraphError::SnapshotExpired);
    }
    let mut warnings = data.warnings.clone();
    let in_scope: Vec<&SourceDocument> = data
        .sources
        .iter()
        .filter(|s| request.scope.allows(&s.meta))
        .collect();
    let scope_docs: HashSet<&str> = in_scope
        .iter()
        .map(|s| s.meta.reference.id.as_str())
        .collect();
    let shown_sources = round_robin(&in_scope, limits.max_documents);
    let shown_docs: HashSet<&str> = shown_sources
        .iter()
        .map(|s| s.meta.reference.id.as_str())
        .collect();

    // Владелец узла любого вида: документ, раздел и чанк приводятся к документу.
    let mut owner_of: HashMap<&str, &str> = HashMap::new();
    let mut chunks_of: HashMap<&str, Vec<&Node>> = HashMap::new();
    for node in &data.nodes {
        match node.reference.kind {
            Kind::Document => {
                owner_of.insert(&node.reference.id, &node.reference.id);
            }
            Kind::Section | Kind::Chunk => {
                if let Some(owner) = node.source_ids.first() {
                    owner_of.insert(&node.reference.id, owner);
                }
                if node.reference.kind == Kind::Chunk
                    && let Some(parent) = &node.parent
                {
                    chunks_of.entry(parent).or_default().push(node);
                }
            }
            _ => {}
        }
    }
    let evidence_line: HashMap<&str, usize> = data
        .evidence
        .iter()
        .map(|e| (e.id.as_str(), e.span.first_line))
        .collect();
    let evidence_span: HashMap<&str, (usize, usize)> = data
        .evidence
        .iter()
        .map(|e| (e.id.as_str(), (e.span.first_line, e.span.last_line)))
        .collect();

    // Ссылки: направленная пара документов, строки всех упоминаний; ссылка внутри документа — не ребро.
    let mut links: BTreeMap<(&str, &str), BTreeSet<usize>> = BTreeMap::new();
    for edge in data
        .edges
        .iter()
        .filter(|e| e.predicate == Predicate::LinksTo)
    {
        let (Some(&from), Some(&to)) = (
            owner_of.get(edge.from.as_str()),
            owner_of.get(edge.to.as_str()),
        ) else {
            continue;
        };
        if from == to || !scope_docs.contains(from) || !scope_docs.contains(to) {
            continue;
        }
        let lines = links.entry((from, to)).or_default();
        for proof in &edge.evidence {
            if let Some(line) = evidence_line.get(proof.as_str()) {
                lines.insert(*line);
            }
        }
    }
    let links_total = links.len();

    let order: HashMap<&str, usize> = shown_sources
        .iter()
        .enumerate()
        .map(|(i, s)| (s.meta.reference.id.as_str(), i))
        .collect();
    let mut link_edges: Vec<RawEdge> = links
        .iter()
        .filter(|((a, b), _)| shown_docs.contains(a) && shown_docs.contains(b))
        .map(|((a, b), lines)| RawEdge {
            kind: PhysicalEdgeKind::MarkdownLink,
            from: (*a).to_string(),
            to: (*b).to_string(),
            raw: None,
            strength: 1.0,
            lines: lines.iter().copied().collect(),
        })
        .collect();
    link_edges.sort_by_key(|e| (order[e.from.as_str()], order[e.to.as_str()]));

    // Близость документов: неориентированная пара, сила — максимум из двух направлений kNN.
    let mut sims: BTreeMap<(String, String), f32> = BTreeMap::new();
    for pair in pairs {
        if pair.a == pair.b
            || !scope_docs.contains(pair.a.as_str())
            || !scope_docs.contains(pair.b.as_str())
        {
            continue;
        }
        let slot = sims
            .entry(pair_key(&pair.a, &pair.b))
            .or_insert(pair.cosine);
        *slot = slot.max(pair.cosine);
    }
    let pairs_total = sims.len();
    let mut doc_sim_edges: Vec<RawEdge> = sims
        .into_iter()
        .filter(|((a, b), _)| shown_docs.contains(a.as_str()) && shown_docs.contains(b.as_str()))
        .map(|((a, b), cosine)| RawEdge {
            kind: PhysicalEdgeKind::DocumentSimilarity,
            raw: Some(cosine),
            strength: cosine.clamp(0.0, 1.0),
            from: a,
            to: b,
            lines: vec![],
        })
        .collect();
    doc_sim_edges.sort_by(|x, y| {
        y.raw
            .partial_cmp(&x.raw)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| x.from.cmp(&y.from))
            .then_with(|| x.to.cmp(&y.to))
    });

    // Узлы: документы по кругу модулей, затем чанки раскрытых документов в порядке нарезки.
    let mut nodes: Vec<PhysicalNode> = shown_sources
        .iter()
        .map(|s| PhysicalNode {
            id: s.meta.reference.id.clone(),
            revision: s.meta.reference.revision.clone(),
            kind: PhysicalNodeKind::Document,
            label: s.meta.title.clone(),
            path: s.meta.path.clone(),
            docs_path: s.meta.docs_path.clone(),
            module: s.meta.module.clone(),
            owner: s.meta.reference.id.clone(),
            chunks: chunks_of
                .get(s.meta.reference.id.as_str())
                .map_or(0, Vec::len),
            ordinal: None,
            first_line: 1,
            last_line: 1,
        })
        .collect();
    let chunks_total = in_scope
        .iter()
        .map(|s| {
            chunks_of
                .get(s.meta.reference.id.as_str())
                .map_or(0, Vec::len)
        })
        .sum();
    let source_by_id: HashMap<&str, &SourceDocument> = shown_sources
        .iter()
        .map(|s| (s.meta.reference.id.as_str(), *s))
        .collect();
    let mut shown_chunks: HashSet<&str> = HashSet::new();
    let mut chunk_cut = false;
    let mut seen_expanded = HashSet::new();
    for expanded in &request.expanded {
        let Some(source) = source_by_id.get(expanded.document.as_str()) else {
            warnings.push(format!(
                "Раскрытие пропущено: документ {} вне среза",
                expanded.document
            ));
            continue;
        };
        if !seen_expanded.insert(expanded.document.as_str()) {
            continue;
        }
        let list = chunks_of
            .get(expanded.document.as_str())
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        for (ordinal, node) in list.iter().enumerate().take(expanded.shown) {
            if shown_chunks.len() >= limits.max_chunks {
                chunk_cut = true;
                break;
            }
            let (first, last) = node
                .evidence
                .iter()
                .filter_map(|id| evidence_span.get(id.as_str()))
                .fold((usize::MAX, 0), |(lo, hi), (a, b)| (lo.min(*a), hi.max(*b)));
            shown_chunks.insert(&node.reference.id);
            nodes.push(PhysicalNode {
                id: node.reference.id.clone(),
                revision: node.reference.revision.clone(),
                kind: PhysicalNodeKind::Chunk,
                label: node.label.clone(),
                path: source.meta.path.clone(),
                docs_path: source.meta.docs_path.clone(),
                module: source.meta.module.clone(),
                owner: expanded.document.clone(),
                chunks: 0,
                ordinal: Some(ordinal),
                first_line: if first == usize::MAX { 1 } else { first },
                last_line: last.max(if first == usize::MAX { 1 } else { first }),
            });
        }
    }
    let mut chunk_sims: BTreeMap<(String, String), f32> = BTreeMap::new();
    for edge in data
        .edges
        .iter()
        .filter(|e| e.predicate == Predicate::SimilarTo)
    {
        let Some(cosine) = edge.score else { continue };
        if edge.from == edge.to
            || !shown_chunks.contains(edge.from.as_str())
            || !shown_chunks.contains(edge.to.as_str())
        {
            continue;
        }
        let slot = chunk_sims
            .entry(pair_key(&edge.from, &edge.to))
            .or_insert(cosine);
        *slot = slot.max(cosine);
    }
    let mut chunk_sim_edges: Vec<RawEdge> = chunk_sims
        .into_iter()
        .map(|((a, b), cosine)| RawEdge {
            kind: PhysicalEdgeKind::ChunkSimilarity,
            raw: Some(cosine),
            strength: cosine.clamp(0.0, 1.0),
            from: a,
            to: b,
            lines: vec![],
        })
        .collect();
    chunk_sim_edges.sort_by(|x, y| {
        y.raw
            .partial_cmp(&x.raw)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| x.from.cmp(&y.from))
            .then_with(|| x.to.cmp(&y.to))
    });

    // Рёбра: сначала ссылки, затем близость документов, затем близость чанков; в каждом виде обрезается слабейшее.
    let available = [
        (PhysicalEdgeKind::MarkdownLink, link_edges.len()),
        (PhysicalEdgeKind::DocumentSimilarity, doc_sim_edges.len()),
        (PhysicalEdgeKind::ChunkSimilarity, chunk_sim_edges.len()),
    ];
    let mut room = limits.max_edges;
    let mut raw_edges: Vec<RawEdge> = Vec::new();
    let mut cut: HashMap<PhysicalEdgeKind, usize> = HashMap::new();
    for (batch, (kind, count)) in [link_edges, doc_sim_edges, chunk_sim_edges]
        .into_iter()
        .zip(available)
    {
        let take = batch.len().min(room);
        room -= take;
        if take < count {
            cut.insert(kind, count - take);
        }
        raw_edges.extend(batch.into_iter().take(take));
    }
    let index_of: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();
    let edges: Vec<PhysicalEdge> = raw_edges
        .into_iter()
        .map(|e| PhysicalEdge {
            kind: e.kind,
            from: index_of[e.from.as_str()],
            to: index_of[e.to.as_str()],
            raw: e.raw,
            strength: e.strength,
            lines: e.lines,
        })
        .collect();
    drop(index_of);
    let mut slice = PhysicalGraphSlice {
        schema_version: SCHEMA_VERSION,
        method: PHYSICAL_METHOD.into(),
        snapshot: data.snapshot.clone(),
        scope: request.scope.clone(),
        shown: PhysicalShown {
            documents: shown_sources.len(),
            chunks: shown_chunks.len(),
            edges: edges.len(),
        },
        totals: PhysicalTotals {
            documents: in_scope.len(),
            chunks: chunks_total,
            links: links_total,
            document_pairs: pairs_total,
        },
        nodes,
        edges,
        layers: PhysicalLayers {
            markdown_link: PhysicalLayer {
                state: inputs.markdown_link.state,
                note: inputs.markdown_link.note.clone(),
            },
            document_similarity: PhysicalLayer {
                state: inputs.document_similarity.state,
                note: inputs.document_similarity.note.clone(),
            },
            chunk_similarity: PhysicalLayer {
                state: inputs.chunk_similarity.state,
                note: inputs.chunk_similarity.note.clone(),
            },
        },
        applied: limits,
        truncated: false,
        limit_reasons: vec![],
        warnings,
        used_bytes: 0,
    };

    // Байтовый предел: сначала снимаются слабейшие рёбра близости чанков, затем документов, затем ссылки с конца.
    slice.used_bytes = serde_json::to_vec(&slice)?.len();
    while slice.used_bytes > limits.max_bytes && !slice.edges.is_empty() {
        let over = slice.used_bytes - limits.max_bytes;
        let average = (slice.used_bytes / (slice.nodes.len() + slice.edges.len()).max(1)).max(64);
        let drop = (over / average + 1).min(slice.edges.len());
        let kept = slice.edges.len() - drop;
        for edge in slice.edges.drain(kept..) {
            *cut.entry(edge.kind).or_default() += 1;
        }
        slice.shown.edges = slice.edges.len();
        slice.used_bytes = serde_json::to_vec(&slice)?.len();
        if drop == 0 {
            break;
        }
    }
    if slice.used_bytes > limits.max_bytes {
        return Err(GraphError::InvalidParams(
            "узлы среза не помещаются в max_bytes; уменьшите область или увеличьте предел".into(),
        ));
    }

    let mut reasons = Vec::new();
    if shown_sources.len() < in_scope.len() {
        reasons.push(format!(
            "документов показано {} из {}",
            shown_sources.len(),
            in_scope.len()
        ));
    }
    if chunk_cut {
        reasons.push(format!(
            "чанков показано {}: достигнут предел {}",
            shown_chunks.len(),
            limits.max_chunks
        ));
    }
    for kind in [
        PhysicalEdgeKind::MarkdownLink,
        PhysicalEdgeKind::DocumentSimilarity,
        PhysicalEdgeKind::ChunkSimilarity,
    ] {
        if let Some(n) = cut.get(&kind).filter(|n| **n > 0) {
            reasons.push(format!(
                "{}: не показано {n} (предел рёбер или байт)",
                kind_label(kind)
            ));
            let layer = match kind {
                PhysicalEdgeKind::MarkdownLink => &mut slice.layers.markdown_link,
                PhysicalEdgeKind::DocumentSimilarity => &mut slice.layers.document_similarity,
                PhysicalEdgeKind::ChunkSimilarity => &mut slice.layers.chunk_similarity,
            };
            if layer.state == LayerState::Ready {
                layer.state = LayerState::Partial;
            }
            layer.note = format!("{}; показана часть: усечено по пределу среза", layer.note);
        }
    }
    slice.truncated = !reasons.is_empty();
    slice.limit_reasons = reasons;
    slice.used_bytes = serde_json::to_vec(&slice)?.len();
    Ok(slice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{InputChunk, InputDocument, build};

    fn document(path: &str, module: &str, text: &str, chunks: usize) -> InputDocument {
        InputDocument {
            path: format!("docs/{path}"),
            docs_path: path.into(),
            module: module.into(),
            title: path.into(),
            text: text.into(),
            chunks: (0..chunks)
                .map(|i| InputChunk {
                    key: format!("{i}:h{i}"),
                    label: format!("раздел {i}"),
                    spans: std::iter::once(0..text.len().min(4)).collect(),
                })
                .collect(),
        }
    }

    fn inputs() -> LayerInputs {
        let ready = |note: &str| Layer {
            state: LayerState::Ready,
            note: note.into(),
        };
        LayerInputs {
            markdown_link: ready("ссылки"),
            document_similarity: ready("близость"),
            chunk_similarity: ready("чанки"),
        }
    }

    fn corpus() -> Dataset {
        build(
            "t",
            "docs",
            vec![
                document(
                    "features/a.md",
                    "features",
                    "# А\n[б](b.md) [б2](b.md#х)\n[сам](#а)\n",
                    3,
                ),
                document("features/b.md", "features", "# Б\n## Х\n[а](a.md)\n", 2),
                document("adr/c.md", "adr", "# В\nбез ссылок\n", 1),
            ],
        )
        .unwrap()
    }

    fn id(data: &Dataset, docs_path: &str) -> String {
        data.sources
            .iter()
            .find(|s| s.meta.docs_path == docs_path)
            .unwrap()
            .meta
            .reference
            .id
            .clone()
    }

    #[test]
    fn links_keep_direction_collapse_repeats_and_skip_self_loops() {
        let data = corpus();
        let slice = project(&data, &[], &PhysicalRequest::default(), inputs()).unwrap();
        let a = id(&data, "features/a.md");
        let b = id(&data, "features/b.md");
        let links: Vec<_> = slice
            .edges
            .iter()
            .filter(|e| e.kind == PhysicalEdgeKind::MarkdownLink)
            .collect();
        assert_eq!(
            links.len(),
            2,
            "A→B и B→A отдельно, повтор и self-anchor не создают рёбер"
        );
        let at = |id: &str| slice.nodes.iter().position(|n| n.id == id).unwrap();
        let (ia, ib) = (at(&a), at(&b));
        let forward = links.iter().find(|e| e.from == ia && e.to == ib).unwrap();
        assert_eq!(forward.lines, vec![2]);
        assert!(links.iter().any(|e| e.from == ib && e.to == ia));
        assert_eq!(slice.totals.links, 2);
        assert!(links.iter().all(|e| e.strength == 1.0 && e.raw.is_none()));
    }

    #[test]
    fn document_similarity_is_undirected_max_and_scoped() {
        let data = corpus();
        let (a, b, c) = (
            id(&data, "features/a.md"),
            id(&data, "features/b.md"),
            id(&data, "adr/c.md"),
        );
        let pairs = [
            DocumentPair {
                a: a.clone(),
                b: b.clone(),
                cosine: 0.7,
            },
            DocumentPair {
                a: b.clone(),
                b: a.clone(),
                cosine: 0.9,
            },
            DocumentPair {
                a: a.clone(),
                b: a.clone(),
                cosine: 1.0,
            },
            DocumentPair {
                a: a.clone(),
                b: c.clone(),
                cosine: 1.4,
            },
        ];
        let scoped = PhysicalRequest {
            scope: Scope {
                modules: vec!["features".into()],
                files: vec![],
            },
            ..Default::default()
        };
        let slice = project(&data, &pairs, &scoped, inputs()).unwrap();
        let sims: Vec<_> = slice
            .edges
            .iter()
            .filter(|e| e.kind == PhysicalEdgeKind::DocumentSimilarity)
            .collect();
        assert_eq!(sims.len(), 1);
        assert_eq!(sims[0].raw, Some(0.9));
        assert_eq!(slice.totals.documents, 2);
        let all = project(&data, &pairs, &PhysicalRequest::default(), inputs()).unwrap();
        let strong = all
            .edges
            .iter()
            .find(|e| e.raw == Some(1.4))
            .expect("cosine вне [0,1] хранится как есть, сила ограничена");
        assert_eq!(strong.strength, 1.0);
    }

    #[test]
    fn document_budget_walks_modules_round_robin_and_marks_truncation() {
        let data = corpus();
        let request = PhysicalRequest {
            limits: PhysicalLimits {
                max_documents: 2,
                ..PhysicalLimits::INITIAL
            },
            ..Default::default()
        };
        let slice = project(&data, &[], &request, inputs()).unwrap();
        let labels: Vec<_> = slice.nodes.iter().map(|n| n.module.as_str()).collect();
        assert_eq!(
            labels,
            ["adr", "features"],
            "по одному документу на модуль по кругу"
        );
        assert!(slice.truncated);
        assert_eq!(slice.shown.documents, 2);
        assert_eq!(slice.totals.documents, 3);
        assert!(
            slice
                .edges
                .iter()
                .all(|e| e.from < slice.nodes.len() && e.to < slice.nodes.len())
        );
    }

    #[test]
    fn expansion_adds_chunks_in_order_and_cap_does_not_evict_documents() {
        let data = corpus();
        let a = id(&data, "features/a.md");
        let request = PhysicalRequest {
            expanded: vec![ExpandedDocument {
                document: a.clone(),
                shown: 2,
            }],
            ..Default::default()
        };
        let slice = project(&data, &[], &request, inputs()).unwrap();
        let chunks: Vec<_> = slice
            .nodes
            .iter()
            .filter(|n| n.kind == PhysicalNodeKind::Chunk)
            .collect();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].ordinal, Some(0));
        assert!(chunks.iter().all(|n| n.owner == a));
        let document = slice.nodes.iter().find(|n| n.id == a).unwrap();
        assert_eq!(document.chunks, 3, "у документа число всех чанков");
        let capped = PhysicalRequest {
            limits: PhysicalLimits {
                max_chunks: 1,
                ..PhysicalLimits::INITIAL
            },
            ..request
        };
        let slice = project(&data, &[], &capped, inputs()).unwrap();
        assert_eq!(slice.shown.chunks, 1);
        assert_eq!(slice.shown.documents, 3);
        assert!(slice.truncated);
    }

    #[test]
    fn edge_budget_drops_weakest_similarity_after_links_and_degrades_layer() {
        let data = corpus();
        let (a, b, c) = (
            id(&data, "features/a.md"),
            id(&data, "features/b.md"),
            id(&data, "adr/c.md"),
        );
        let pairs = [
            DocumentPair {
                a: a.clone(),
                b: b.clone(),
                cosine: 0.6,
            },
            DocumentPair {
                a: a.clone(),
                b: c.clone(),
                cosine: 0.9,
            },
            DocumentPair {
                a: b.clone(),
                b: c.clone(),
                cosine: 0.8,
            },
        ];
        let request = PhysicalRequest {
            limits: PhysicalLimits {
                max_edges: 3,
                ..PhysicalLimits::INITIAL
            },
            ..Default::default()
        };
        let slice = project(&data, &pairs, &request, inputs()).unwrap();
        assert_eq!(slice.edges.len(), 3);
        let kept: Vec<_> = slice
            .edges
            .iter()
            .filter(|e| e.kind == PhysicalEdgeKind::DocumentSimilarity)
            .filter_map(|e| e.raw)
            .collect();
        assert_eq!(
            kept,
            [0.9],
            "после двух ссылок остаётся сильнейшая близость"
        );
        assert_eq!(slice.layers.document_similarity.state, LayerState::Partial);
        assert_eq!(slice.layers.markdown_link.state, LayerState::Ready);
        assert!(slice.truncated);
    }

    #[test]
    fn stale_snapshot_and_bad_limits_are_rejected() {
        let data = corpus();
        let request = PhysicalRequest {
            snapshot: Some("old".into()),
            ..Default::default()
        };
        assert!(matches!(
            project(&data, &[], &request, inputs()),
            Err(GraphError::SnapshotExpired)
        ));
        let request = PhysicalRequest {
            limits: PhysicalLimits {
                max_documents: 0,
                ..PhysicalLimits::INITIAL
            },
            ..Default::default()
        };
        assert!(matches!(
            project(&data, &[], &request, inputs()),
            Err(GraphError::InvalidParams(_))
        ));
    }

    #[test]
    fn slice_is_deterministic_and_within_byte_limit() {
        let data = corpus();
        let one = project(&data, &[], &PhysicalRequest::default(), inputs()).unwrap();
        let two = project(&data, &[], &PhysicalRequest::default(), inputs()).unwrap();
        assert_eq!(one, two);
        assert!(one.used_bytes <= PhysicalLimits::INITIAL.max_bytes);
        assert_eq!(one.method, PHYSICAL_METHOD);
    }
}
