//! Близость документов `document-centroid-v1` по готовым векторам чанков, без вызова модели.
//!
//! Вес чанка — длина его исходного текста в Unicode-символах; диапазон, покрытый несколькими чанками,
//! делится между ними поровну. Документ с недостающим вектором содержательного чанка не получает приблизительный
//! centroid: он выпадает из слоя и попадает в покрытие (physical-graph.md, «Документная близость без новой модели»).

use super::similarity::{KnnOptions, StopReason, VectorNode, build_knn};
use crate::storage::Index;
use bars_graph::physical::DocumentPair;
use bars_graph::{Dataset, GraphError, Kind};
use std::collections::{BTreeSet, HashMap};

pub const METHOD: &str = "document-centroid-v1";
const K: usize = 5;
const TAU: f32 = 0.5;
/// Документов до 2000 (предел среза) дают не более 4 млн сравнений.
const MAX_COMPARISONS: u64 = 25_000_000;

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentSimilarity {
    pub pairs: Vec<DocumentPair>,
    /// Документов в графе и документов, получивших представление.
    pub documents: usize,
    pub covered: usize,
    /// Документы без представления: путь от docs-root и причина.
    pub skipped: Vec<(String, String)>,
    pub stopped: Option<StopReason>,
    /// Завершённых строк kNN и всех строк, когда расчёт прерван.
    pub rows: (usize, usize),
}

#[derive(Clone)]
pub struct DocumentRepresentations {
    pub vectors: Vec<VectorNode>,
    pub skipped: Vec<(String, String)>,
}

/// Доли символов исходного текста, приходящиеся на каждый чанк документа; пересечения учитываются один раз.
pub fn chunk_weights(text: &str, spans: &[Vec<(usize, usize)>]) -> Vec<f64> {
    let mut cuts = BTreeSet::new();
    for chunk in spans {
        for &(a, b) in chunk {
            if a < b {
                cuts.insert(a);
                cuts.insert(b);
            }
        }
    }
    let cuts: Vec<usize> = cuts.into_iter().collect();
    let mut weights = vec![0.0; spans.len()];
    for window in cuts.windows(2) {
        let (a, b) = (window[0], window[1]);
        let owners: Vec<usize> = spans
            .iter()
            .enumerate()
            .filter(|(_, chunk)| chunk.iter().any(|&(x, y)| x <= a && b <= y))
            .map(|(i, _)| i)
            .collect();
        if owners.is_empty() {
            continue;
        }
        let chars = text.get(a..b).map_or(b - a, |s| s.chars().count()) as f64;
        for i in &owners {
            weights[*i] += chars / owners.len() as f64;
        }
    }
    weights
}

fn normalize(vector: &[f32]) -> Option<Vec<f64>> {
    if vector.iter().any(|x| !x.is_finite()) {
        return None;
    }
    let norm = vector
        .iter()
        .map(|&x| f64::from(x).powi(2))
        .sum::<f64>()
        .sqrt();
    (norm > 0.0).then(|| vector.iter().map(|&x| f64::from(x) / norm).collect())
}

/// Строит пары близости документов: k=5, tau=0.5, объединение top-k каждого конца.
pub fn document_similarity(
    index: &Index,
    data: &Dataset,
    mut cancelled: impl FnMut() -> bool,
) -> Result<DocumentSimilarity, GraphError> {
    let representations = document_representations(index, data, &mut cancelled)?;
    similarity_from_representations(data, &representations, cancelled)
}

/// Общие представления физического графа и вида запроса; модели не вызываются.
pub fn document_representations(
    index: &Index,
    data: &Dataset,
    mut cancelled: impl FnMut() -> bool,
) -> Result<DocumentRepresentations, GraphError> {
    let evidence: HashMap<&str, (usize, usize)> = data
        .evidence
        .iter()
        .map(|e| (e.id.as_str(), (e.span.start, e.span.end)))
        .collect();
    let mut chunk_nodes: HashMap<&str, Vec<&bars_graph::Node>> = HashMap::new();
    for node in data
        .nodes
        .iter()
        .filter(|n| n.reference.kind == Kind::Chunk)
    {
        if let Some(parent) = &node.parent {
            chunk_nodes.entry(parent).or_default().push(node);
        }
    }
    let mut statement = index.conn().prepare(
        "SELECT c.embed_hash FROM chunks c JOIN documents d ON d.id=c.document_id WHERE d.path=?1 ORDER BY c.ordinal",
    )?;
    let mut centroids: Vec<VectorNode> = Vec::new();
    let mut skipped = Vec::new();
    let mut dimension = 0;
    for source in &data.sources {
        if cancelled() {
            return Err(GraphError::Cancelled);
        }
        let id = source.meta.reference.id.as_str();
        let skip = |reason: &str| (source.meta.docs_path.clone(), reason.to_string());
        let nodes = chunk_nodes.get(id).map(Vec::as_slice).unwrap_or(&[]);
        let hashes: Vec<String> = statement
            .query_map([&source.meta.path], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        if nodes.is_empty() {
            skipped.push(skip("нет чанков"));
            continue;
        }
        if hashes.len() != nodes.len() {
            skipped.push(skip("нарезка графа не совпадает с индексом"));
            continue;
        }
        let spans: Vec<Vec<(usize, usize)>> = nodes
            .iter()
            .map(|n| {
                n.evidence
                    .iter()
                    .filter_map(|e| evidence.get(e.as_str()).copied())
                    .collect()
            })
            .collect();
        let weights = chunk_weights(&source.text, &spans);
        let total: f64 = weights.iter().sum();
        if total <= 0.0 {
            skipped.push(skip("нет содержательного текста"));
            continue;
        }
        let mut sum: Vec<f64> = Vec::new();
        let mut reason = None;
        for (hash, weight) in hashes.iter().zip(&weights) {
            if *weight <= 0.0 {
                continue;
            }
            let Some(vector) = index
                .vector(hash)
                .map_err(|e| GraphError::InvalidData(e.to_string()))?
            else {
                reason = Some("у содержательного чанка нет вектора");
                break;
            };
            let Some(unit) = normalize(&vector) else {
                reason = Some("вектор нулевой или нечисловой");
                break;
            };
            if sum.is_empty() {
                sum = vec![0.0; unit.len()];
            } else if sum.len() != unit.len() {
                reason = Some("векторы разной размерности");
                break;
            }
            for (acc, x) in sum.iter_mut().zip(&unit) {
                *acc += weight * x;
            }
        }
        if let Some(reason) = reason {
            skipped.push(skip(reason));
            continue;
        }
        let Some(centroid) =
            normalize(&sum.iter().map(|&x| (x / total) as f32).collect::<Vec<_>>())
        else {
            skipped.push(skip("centroid нулевой"));
            continue;
        };
        if dimension == 0 {
            dimension = centroid.len();
        } else if dimension != centroid.len() {
            skipped.push(skip("размерность отличается от остальных документов"));
            continue;
        }
        centroids.push(VectorNode {
            id: id.to_string(),
            vector: centroid.iter().map(|&x| x as f32).collect(),
        });
    }
    drop(statement);
    Ok(DocumentRepresentations { vectors: centroids, skipped })
}

pub fn similarity_from_representations(
    data: &Dataset,
    representations: &DocumentRepresentations,
    mut cancelled: impl FnMut() -> bool,
) -> Result<DocumentSimilarity, GraphError> {
    let covered = representations.vectors.len();
    let knn = build_knn(
        &representations.vectors,
        KnnOptions {
            k: K,
            min_cosine: TAU,
            max_comparisons: MAX_COMPARISONS,
        },
        &mut cancelled,
    )
    .map_err(|e| GraphError::InvalidData(e.to_string()))?;
    if knn.stop_reason == Some(StopReason::Cancelled) {
        return Err(GraphError::Cancelled);
    }
    let rows = (knn.neighbors.len(), knn.total_nodes);
    let pairs = knn
        .neighbors
        .into_iter()
        .flat_map(|(a, list)| {
            list.into_iter().map(move |n| DocumentPair {
                a: a.clone(),
                b: n.id,
                cosine: n.cosine,
            })
        })
        .collect();
    Ok(DocumentSimilarity {
        pairs,
        documents: data.sources.len(),
        covered,
        skipped: representations.skipped.clone(),
        stopped: knn.stop_reason,
        rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_is_counted_once_and_split_equally() {
        let text = "абвгдежзик";
        // Байты: чанк 0 — 0..12 (6 символов), чанк 1 — 8..20 (6 символов); общие 2 символа делятся пополам.
        let spans = vec![vec![(0, 12)], vec![(8, 20)]];
        let weights = chunk_weights(text, &spans);
        assert_eq!(weights.len(), 2);
        assert!((weights[0] - 5.0).abs() < 1e-9, "{weights:?}");
        assert!((weights[1] - 5.0).abs() < 1e-9, "{weights:?}");
        assert!((weights.iter().sum::<f64>() - text.chars().count() as f64).abs() < 1e-9);
    }

    #[test]
    fn empty_or_repeated_spans_do_not_inflate_weight() {
        let text = "abcdef";
        let spans = vec![vec![(0, 3), (0, 3), (2, 2)], vec![]];
        let weights = chunk_weights(text, &spans);
        assert!((weights[0] - 3.0).abs() < 1e-9, "{weights:?}");
        assert_eq!(weights[1], 0.0);
    }

    #[test]
    fn normalize_rejects_zero_and_non_finite() {
        assert!(normalize(&[0.0, 0.0]).is_none());
        assert!(normalize(&[f32::NAN, 1.0]).is_none());
        let unit = normalize(&[3.0, 4.0]).unwrap();
        assert!((unit[0] - 0.6).abs() < 1e-9 && (unit[1] - 0.8).abs() < 1e-9);
    }
}
