//! Точный ограниченный kNN над существующими embeddings, без вызова модели.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap};
use std::fmt;

/// ID задаёт вызывающий слой; это не публичный SQLite rowid.
pub struct VectorNode {
    pub id: String,
    pub vector: Vec<f32>,
}

#[derive(Debug, Clone, Copy)]
pub struct KnnOptions {
    pub k: usize,
    pub min_cosine: f32,
    /// Количество скалярных произведений, а не число итоговых рёбер.
    pub max_comparisons: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SimilarNeighbor {
    pub id: String,
    /// Cosine в [-1, 1], не вероятность истинности связи.
    pub cosine: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    ComparisonLimit,
    Cancelled,
}

impl StopReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::ComparisonLimit => "достигнут предел сравнений",
            Self::Cancelled => "расчёт остановлен",
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct KnnGraph {
    /// Только полностью проверенные source-узлы; target может ещё не иметь строки.
    pub neighbors: BTreeMap<String, Vec<SimilarNeighbor>>,
    pub total_nodes: usize,
    pub comparisons: u64,
    pub stop_reason: Option<StopReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidInput(pub String);

impl fmt::Display for InvalidInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for InvalidInput {}

/// Худший сосед наверху: меньший cosine, при равенстве — больший ID.
#[derive(Clone, Copy)]
struct Candidate {
    index: usize,
    cosine: f32,
}

impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Candidate {}
impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cosine
            .total_cmp(&self.cosine)
            .then(self.index.cmp(&other.index))
    }
}

/// Область должна быть отфильтрована ДО вызова: запрещённые узлы не кандидаты в kNN.
///
/// Частичная строка не публикуется как top-k: при отмене/лимите сохраняются только
/// строки, сравнившие source со ВСЕМИ разрешёнными targets. Число завершённых строк
/// вместе с total_nodes сообщает покрытие, а не полноту предметного знания.
pub fn build_knn(
    nodes: &[VectorNode],
    options: KnnOptions,
    cancelled: impl FnMut() -> bool,
) -> Result<KnnGraph, InvalidInput> {
    build_knn_observed(nodes, options, cancelled, |_, _| {})
}

pub fn build_knn_observed(
    nodes: &[VectorNode],
    options: KnnOptions,
    mut cancelled: impl FnMut() -> bool,
    mut progress: impl FnMut(usize, usize),
) -> Result<KnnGraph, InvalidInput> {
    if options.k == 0 {
        return Err(InvalidInput("k должен быть положительным".into()));
    }
    if !options.min_cosine.is_finite() || !(-1.0..=1.0).contains(&options.min_cosine) {
        return Err(InvalidInput(
            "min_cosine должен быть конечным числом в [-1, 1]".into(),
        ));
    }
    // Канонический порядок обеспечивает одинаковый top-k при перестановке входа.
    let mut ordered: Vec<_> = nodes.iter().collect();
    ordered.sort_by(|a, b| a.id.cmp(&b.id));
    let dimension = ordered.first().map_or(0, |n| n.vector.len());
    let mut normalized = Vec::with_capacity(nodes.len());
    for (index, node) in ordered.iter().enumerate() {
        if node.id.trim().is_empty() || (index > 0 && ordered[index - 1].id == node.id) {
            return Err(InvalidInput(
                "ID узлов должны быть непустыми и уникальными".into(),
            ));
        }
        if dimension == 0 || node.vector.len() != dimension {
            return Err(InvalidInput(
                "векторы должны иметь одинаковую ненулевую размерность".into(),
            ));
        }
        if node.vector.iter().any(|x| !x.is_finite()) {
            return Err(InvalidInput("вектор содержит NaN или бесконечность".into()));
        }
        let norm = node
            .vector
            .iter()
            .map(|&x| f64::from(x).powi(2))
            .sum::<f64>()
            .sqrt();
        if norm == 0.0 {
            return Err(InvalidInput("нулевой вектор не имеет cosine".into()));
        }
        normalized.push(
            node.vector
                .iter()
                .map(|&x| (f64::from(x) / norm) as f32)
                .collect::<Vec<_>>(),
        );
    }
    let mut result = KnnGraph {
        neighbors: BTreeMap::new(),
        total_nodes: nodes.len(),
        comparisons: 0,
        stop_reason: None,
    };
    let k = options.k.min(nodes.len().saturating_sub(1));
    for (source, node) in ordered.iter().enumerate() {
        if cancelled() {
            result.stop_reason = Some(StopReason::Cancelled);
            break;
        }
        let required = nodes.len().saturating_sub(1) as u64;
        if required > options.max_comparisons.saturating_sub(result.comparisons) {
            result.stop_reason = Some(StopReason::ComparisonLimit);
            break;
        }
        let mut top: BinaryHeap<Candidate> = BinaryHeap::new();
        for target in 0..ordered.len() {
            if source == target {
                continue;
            }
            if cancelled() {
                result.stop_reason = Some(StopReason::Cancelled);
                return Ok(result);
            }
            let cosine = normalized[source]
                .iter()
                .zip(&normalized[target])
                .map(|(&a, &b)| f64::from(a) * f64::from(b))
                .sum::<f64>()
                .clamp(-1.0, 1.0) as f32;
            result.comparisons += 1;
            if cosine < options.min_cosine {
                continue;
            }
            let candidate = Candidate {
                index: target,
                cosine,
            };
            if top.len() < k {
                top.push(candidate);
            } else if let Some(mut worst) = top.peek_mut()
                && candidate < *worst
            {
                *worst = candidate;
            }
        }
        let mut entries: Vec<_> = top.into_vec();
        entries.sort();
        result.neighbors.insert(
            node.id.clone(),
            entries
                .into_iter()
                .map(|c| SimilarNeighbor {
                    id: ordered[c.index].id.clone(),
                    cosine: c.cosine,
                })
                .collect(),
        );
        progress(result.neighbors.len(), result.total_nodes);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, vector: &[f32]) -> VectorNode {
        VectorNode {
            id: id.into(),
            vector: vector.into(),
        }
    }
    fn options(k: usize) -> KnnOptions {
        KnnOptions {
            k,
            min_cosine: -1.0,
            max_comparisons: u64::MAX,
        }
    }

    #[test]
    fn normalized_cosine_order_sign_and_no_self_edge() {
        let nodes = [
            node("a", &[3., 0.]),
            node("b", &[30., 0.]),
            node("c", &[0., 2.]),
            node("d", &[-2., 0.]),
        ];
        let graph = build_knn(&nodes, options(3), || false).unwrap();
        let neighbors = &graph.neighbors["a"];
        assert_eq!(
            neighbors.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
            ["b", "c", "d"]
        );
        assert_eq!(
            neighbors.iter().map(|n| n.cosine).collect::<Vec<_>>(),
            [1., 0., -1.]
        );
        assert_eq!(graph.comparisons, 12);
        assert_eq!(graph.stop_reason, None);
    }

    #[test]
    fn ties_and_partial_coverage_are_independent_of_input_order() {
        let mut nodes = vec![
            node("c", &[1., 0.]),
            node("a", &[1., 0.]),
            node("b", &[1., 0.]),
        ];
        let graph = build_knn(&nodes, options(1), || false).unwrap();
        assert_eq!(graph.neighbors["c"][0].id, "a");
        assert_eq!(graph.neighbors["a"][0].id, "b");
        nodes.reverse();
        assert_eq!(graph, build_knn(&nodes, options(1), || false).unwrap());
        let limited = KnnOptions {
            max_comparisons: 3,
            ..options(1)
        };
        let partial = build_knn(&nodes, limited, || false).unwrap();
        assert_eq!(partial.neighbors.len(), 1);
        assert_eq!(partial.neighbors["a"], graph.neighbors["a"]);
        assert_eq!(partial.total_nodes, 3);
        assert_eq!(partial.comparisons, 2);
        assert_eq!(partial.stop_reason, Some(StopReason::ComparisonLimit));
    }

    #[test]
    fn cancellation_discards_unfinished_row() {
        let nodes = [node("a", &[1.]), node("b", &[1.]), node("c", &[1.])];
        let mut calls = 0;
        let graph = build_knn(&nodes, options(2), || {
            calls += 1;
            calls >= 3
        })
        .unwrap();
        assert_eq!(graph.comparisons, 1);
        assert!(graph.neighbors.is_empty());
        assert_eq!(graph.stop_reason, Some(StopReason::Cancelled));
    }

    #[test]
    fn invalid_vectors_and_parameters_are_rejected() {
        for vector in [vec![], vec![0., 0.], vec![f32::NAN], vec![f32::INFINITY]] {
            assert!(build_knn(&[node("a", &vector)], options(1), || false).is_err());
        }
        for nodes in [
            vec![node("a", &[1.]), node("b", &[1., 2.])],
            vec![node("a", &[1.]), node("a", &[2.])],
            vec![node("", &[1.])],
        ] {
            assert!(build_knn(&nodes, options(1), || false).is_err());
        }
        for bad in [f32::NAN, f32::INFINITY, 1.1, -1.1] {
            assert!(
                build_knn(
                    &[],
                    KnnOptions {
                        min_cosine: bad,
                        ..options(1)
                    },
                    || false
                )
                .is_err()
            );
        }
        assert!(build_knn(&[], options(0), || false).is_err());
    }

    #[test]
    fn filter_before_knn_prevents_hidden_nodes_from_consuming_top_k() {
        let allowed = [node("a", &[1., 0.]), node("b", &[0.8, 0.6])];
        let graph = build_knn(&allowed, options(1), || false).unwrap();
        assert_eq!(graph.neighbors["a"][0].id, "b");
        assert_eq!(graph.total_nodes, 2);
    }

    #[test]
    fn threshold_empty_singleton_and_extreme_finite_values() {
        let nodes = [
            node("a", &[f32::MAX, 0.]),
            node("b", &[0., f32::MIN_POSITIVE]),
        ];
        let graph = build_knn(
            &nodes,
            KnnOptions {
                min_cosine: 0.1,
                ..options(1)
            },
            || false,
        )
        .unwrap();
        assert!(graph.neighbors.values().all(Vec::is_empty));
        assert_eq!(
            build_knn(&[], options(1), || false)
                .unwrap()
                .neighbors
                .len(),
            0
        );
        assert!(
            build_knn(&[node("a", &[1.])], options(1), || false)
                .unwrap()
                .neighbors["a"]
                .is_empty()
        );
    }

    #[test]
    fn heap_top_k_matches_full_sort() {
        let nodes: Vec<_> = (0..35)
            .map(|i| {
                node(
                    &format!("{i:02}"),
                    &[
                        (i % 7 + 1) as f32,
                        ((i * 13) % 9) as f32,
                        ((i * 3) % 11) as f32,
                    ],
                )
            })
            .collect();
        let full = build_knn(&nodes, options(nodes.len()), || false).unwrap();
        for k in [1, 2, 5, 10, 34, 100] {
            let graph = build_knn(&nodes, options(k), || false).unwrap();
            for (source, neighbors) in graph.neighbors {
                assert_eq!(
                    neighbors,
                    full.neighbors[&source]
                        .iter()
                        .take(k)
                        .cloned()
                        .collect::<Vec<_>>()
                );
            }
        }
    }
}
