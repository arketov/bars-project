//! Возобновляемый точный kNN. Незавершённые top-k не предназначены для публикации.
use super::similarity::{InvalidInput, SimilarNeighbor, VectorNode};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

pub const METHOD: &str = "exact-pairs-f64-v1:k5:tau0.5";

#[derive(Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub fingerprint: String,
    pub ids: Vec<String>,
    pub hashes: Vec<String>,
    pub rows: Vec<Vec<SimilarNeighbor>>,
    pub affected: Vec<bool>,
    pub source: usize,
    pub target: usize,
    pub compared: u64,
    pub total: u64,
    pub elapsed_ms: u64,
}

impl Checkpoint {
    fn valid_cursor(&self) -> bool {
        let n = self.ids.len();
        if self.source > n || self.affected.len() != n || !self.ids.windows(2).all(|w| w[0] < w[1])
        {
            return false;
        }
        let mut remaining = self.affected.iter().filter(|&&a| a).count() as u64;
        let mut processed = 0;
        let mut total = 0;
        for (i, &affected) in self.affected.iter().enumerate() {
            remaining -= u64::from(affected);
            let pairs = if affected {
                (n - i - 1) as u64
            } else {
                remaining
            };
            total += pairs;
            if i < self.source {
                processed += pairs;
            }
        }
        if self.source < n {
            if self.target <= self.source || self.target > n {
                return false;
            }
            processed += if self.affected[self.source] {
                (self.target - self.source - 1) as u64
            } else {
                self.affected[self.source + 1..self.target]
                    .iter()
                    .filter(|&&a| a)
                    .count() as u64
            };
        }
        self.total == total && self.compared == processed
    }
}

pub struct Calculation {
    pub checkpoint: Checkpoint,
    vectors: Vec<Vec<f32>>,
}

impl Calculation {
    #[cfg(test)]
    pub(crate) fn vectors_address(&self) -> usize {
        self.vectors.as_ptr() as usize
    }
    pub(crate) fn estimated_bytes(&self) -> usize {
        self.vectors
            .iter()
            .map(|v| v.capacity() * std::mem::size_of::<f32>())
            .sum::<usize>()
            + self.checkpoint.ids.len() * 5 * 128
            + self
                .checkpoint
                .ids
                .iter()
                .map(|id| id.capacity() + 64)
                .sum::<usize>()
    }
    pub fn new(
        nodes: &[VectorNode],
        config: &str,
        resume: Option<Checkpoint>,
        previous: Option<Checkpoint>,
    ) -> Result<Self, InvalidInput> {
        Self::from_vectors(nodes.to_vec(), config, resume, previous)
    }

    /// Продукт передаёт владение, чтобы нормализация не удваивала память векторов.
    pub fn from_vectors(
        mut nodes: Vec<VectorNode>,
        config: &str,
        resume: Option<Checkpoint>,
        previous: Option<Checkpoint>,
    ) -> Result<Self, InvalidInput> {
        nodes.sort_by(|a, b| a.id.cmp(&b.id));
        let dimension = nodes.first().map_or(0, |n| n.vector.len());
        let mut vectors = Vec::new();
        let mut ids = Vec::new();
        let mut hashes = Vec::new();
        for mut node in nodes {
            if node.id.is_empty() || ids.last() == Some(&node.id) {
                return Err(InvalidInput("Повторяющийся или пустой ID чанка".into()));
            }
            if dimension == 0
                || node.vector.len() != dimension
                || node.vector.iter().any(|x| !x.is_finite())
            {
                return Err(InvalidInput(
                    "Некорректная размерность или значение вектора".into(),
                ));
            }
            let norm = node
                .vector
                .iter()
                .map(|&x| f64::from(x).powi(2))
                .sum::<f64>()
                .sqrt();
            if norm == 0.0 {
                return Err(InvalidInput("Нулевой вектор не имеет cosine".into()));
            }
            let mut hash = blake3::Hasher::new();
            for x in &node.vector {
                hash.update(&x.to_le_bytes());
            }
            hashes.push(hash.finalize().to_hex().to_string());
            ids.push(node.id);
            for value in &mut node.vector {
                *value = (f64::from(*value) / norm) as f32;
            }
            vectors.push(node.vector);
        }
        let fingerprint = blake3::hash(format!("{METHOD}:{config}:{dimension}").as_bytes())
            .to_hex()
            .to_string();
        let valid = |c: &Checkpoint| {
            c.fingerprint == fingerprint
                && c.ids.len() == c.hashes.len()
                && c.ids.len() == c.rows.len()
                && c.ids.len() == c.affected.len()
                && c.valid_cursor()
                && c.rows.iter().all(|row| {
                    row.len() <= 5
                        && row
                            .iter()
                            .all(|n| n.cosine.is_finite() && c.ids.binary_search(&n.id).is_ok())
                })
        };
        if let Some(c) = resume.filter(|c| {
            valid(c)
                && c.ids == ids
                && c.hashes == hashes
                && (c.source == ids.len() || c.target <= ids.len())
                && c.compared <= c.total
        }) {
            return Ok(Self {
                checkpoint: c,
                vectors,
            });
        }
        let n = ids.len();
        let mut rows = vec![Vec::new(); n];
        let mut affected = vec![true; n];
        if let Some(old) = previous.filter(|c| valid(c) && c.source == c.ids.len()) {
            let current: BTreeMap<_, _> = ids.iter().zip(&hashes).collect();
            let old_hashes: BTreeMap<_, _> = old.ids.iter().zip(&old.hashes).collect();
            let changed: BTreeSet<_> = old_hashes
                .iter()
                .filter(|(id, hash)| current.get(**id) != Some(*hash))
                .map(|(id, _)| id.as_str())
                .collect();
            // Обратный индекс направленных списков: удалённый сосед требует полной строки.
            let mut reverse: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
            for (id, row) in old.ids.iter().zip(&old.rows) {
                for neighbor in row {
                    reverse.entry(&neighbor.id).or_default().push(id);
                }
            }
            let invalid: BTreeSet<_> = changed
                .iter()
                .flat_map(|id| reverse.get(id).into_iter().flatten().copied())
                .collect();
            for (i, id) in ids.iter().enumerate() {
                if old_hashes.get(id) == current.get(id) && !invalid.contains(id.as_str()) {
                    let position = old
                        .ids
                        .binary_search(id)
                        .map_err(|_| InvalidInput("Неверный порядок основания kNN".into()))?;
                    affected[i] = false;
                    rows[i] = old.rows[position].clone();
                }
            }
        }
        let stable = affected.iter().filter(|&&a| !a).count() as u64;
        let n64 = n as u64;
        let total = n64 * n64.saturating_sub(1) / 2 - stable * stable.saturating_sub(1) / 2;
        Ok(Self {
            checkpoint: Checkpoint {
                fingerprint,
                ids,
                hashes,
                rows,
                affected,
                source: 0,
                target: 1,
                compared: 0,
                total,
                elapsed_ms: 0,
            },
            vectors,
        })
    }

    /// false — квант закончился/отмена; checkpoint уже содержит все завершённые пары.
    pub fn advance(&mut self, quantum: Duration, mut cancelled: impl FnMut() -> bool) -> bool {
        let started = Instant::now();
        let c = &mut self.checkpoint;
        let n = c.ids.len();
        let affected: Vec<_> = c
            .affected
            .iter()
            .enumerate()
            .filter_map(|(i, &a)| a.then_some(i))
            .collect();
        let mut batch = 0;
        while c.source < n {
            if batch == 0 && (cancelled() || started.elapsed() >= quantum) {
                return false;
            }
            if c.target >= n {
                c.source += 1;
                c.target = c.source + 1;
                continue;
            }
            if !c.affected[c.source] {
                let next = affected.partition_point(|&i| i < c.target);
                c.target = affected.get(next).copied().unwrap_or(n);
                if c.target == n {
                    continue;
                }
            }
            let (a, b) = (c.source, c.target);
            if c.affected[a] || c.affected[b] {
                let cosine = self.vectors[a]
                    .iter()
                    .zip(&self.vectors[b])
                    .map(|(&x, &y)| f64::from(x) * f64::from(y))
                    .sum::<f64>()
                    .clamp(-1.0, 1.0) as f32;
                if cosine >= 0.5 {
                    insert(&mut c.rows[a], &c.ids[b], cosine);
                    insert(&mut c.rows[b], &c.ids[a], cosine);
                }
                c.compared += 1;
            }
            c.target += 1;
            batch = (batch + 1) % 64;
        }
        true
    }

    pub fn neighbors(&self) -> BTreeMap<String, Vec<SimilarNeighbor>> {
        self.checkpoint
            .ids
            .iter()
            .cloned()
            .zip(self.checkpoint.rows.iter().cloned())
            .collect()
    }
}

fn insert(row: &mut Vec<SimilarNeighbor>, id: &str, cosine: f32) {
    if row.len() == 5
        && row.last().is_some_and(|worst| {
            cosine < worst.cosine || (cosine == worst.cosine && id >= worst.id.as_str())
        })
    {
        return;
    }
    if row.iter().any(|n| n.id == id) {
        return;
    }
    row.push(SimilarNeighbor {
        id: id.into(),
        cosine,
    });
    row.sort_by(|a, b| b.cosine.total_cmp(&a.cosine).then(a.id.cmp(&b.id)));
    row.truncate(5);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::similarity::{KnnOptions, build_knn};

    fn nodes(count: usize) -> Vec<VectorNode> {
        (0..count)
            .map(|i| VectorNode {
                id: format!("{i:04}"),
                vector: (0..32)
                    .map(|j| ((i * 137 + j * 43 + i * j) % 97) as f32 - 33.)
                    .collect(),
            })
            .collect()
    }
    fn finish(nodes: &[VectorNode], previous: Option<Checkpoint>) -> Calculation {
        let mut c = Calculation::new(nodes, "test", None, previous).unwrap();
        assert!(c.advance(Duration::from_secs(30), || false));
        c
    }
    fn oracle(nodes: &[VectorNode]) -> BTreeMap<String, Vec<SimilarNeighbor>> {
        build_knn(
            nodes,
            KnnOptions {
                k: 5,
                min_cosine: 0.5,
                max_comparisons: u64::MAX,
            },
            || false,
        )
        .unwrap()
        .neighbors
    }

    #[test]
    fn pair_kernel_matches_previous_formula_and_deterministic_ties() {
        for count in [0, 1, 2, 35, 150] {
            let mut nodes = nodes(count);
            if count > 2 {
                nodes[1].vector = nodes[0].vector.clone();
            }
            let full = finish(&nodes, None);
            assert_eq!(full.neighbors(), oracle(&nodes));
            assert_eq!(
                full.checkpoint.compared,
                (count * count.saturating_sub(1) / 2) as u64
            );
            nodes.reverse();
            assert_eq!(full.neighbors(), finish(&nodes, None).neighbors());
        }
    }

    #[test]
    fn quantum_cancellation_and_serialized_restart_keep_exact_result() {
        let nodes = nodes(100);
        let mut c = Calculation::new(&nodes, "test", None, None).unwrap();
        assert!(!c.advance(Duration::ZERO, || false));
        let mut polls = 0;
        assert!(!c.advance(Duration::from_secs(30), || {
            polls += 1;
            polls == 4
        }));
        assert!(c.checkpoint.compared > 0 && c.checkpoint.compared < c.checkpoint.total);
        let count = c.checkpoint.compared;
        let restored =
            serde_json::from_str(&serde_json::to_string(&c.checkpoint).unwrap()).unwrap();
        let mut c = Calculation::new(&nodes, "test", Some(restored), None).unwrap();
        assert_eq!(c.checkpoint.compared, count);
        assert!(c.advance(Duration::from_secs(30), || false));
        assert_eq!(c.neighbors(), oracle(&nodes));
        assert_eq!(c.checkpoint.compared, c.checkpoint.total);
    }

    #[test]
    fn repeated_edit_insert_delete_and_empty_rows_match_full_rebuild() {
        let mut nodes = nodes(100);
        let mut previous = finish(&nodes, None).checkpoint;
        for turn in 0..20 {
            nodes[turn].vector = vec![turn as f32 + 1.; 32];
            if turn % 3 == 0 {
                nodes.remove(90 - turn);
            }
            if turn % 4 == 0 {
                nodes.push(VectorNode {
                    id: format!("new{turn}"),
                    vector: vec![-1.; 32],
                });
            }
            let c = finish(&nodes, Some(previous));
            assert_eq!(c.neighbors(), oracle(&nodes), "turn {turn}");
            assert!(c.checkpoint.compared < (nodes.len() * (nodes.len() - 1) / 2) as u64);
            previous = c.checkpoint;
        }
    }

    #[test]
    fn full_pairs_exceed_the_former_million_comparison_limit() {
        let nodes: Vec<_> = (0..1450)
            .map(|i| VectorNode {
                id: format!("{i:04}"),
                vector: vec![1., (i % 5) as f32],
            })
            .collect();
        let c = finish(&nodes, None);
        assert_eq!(c.checkpoint.compared, 1_050_525);
        assert_eq!(c.neighbors().len(), 1450);
        assert!(c.checkpoint.rows.iter().all(|row| row.len() == 5));
    }

    #[test]
    fn changed_vector_config_dimension_or_inputs_reject_checkpoint() {
        let mut nodes = nodes(20);
        let c = finish(&nodes, None);
        let changed = Calculation::new(
            &nodes,
            "new-model",
            Some(c.checkpoint.clone()),
            Some(c.checkpoint.clone()),
        )
        .unwrap();
        assert_eq!(changed.checkpoint.compared, 0);
        assert!(changed.checkpoint.affected.iter().all(|a| *a));
        nodes[0].vector[0] += 1.;
        let changed = Calculation::new(&nodes, "test", Some(c.checkpoint.clone()), None).unwrap();
        assert_eq!(changed.checkpoint.compared, 0);
        for n in &mut nodes {
            n.vector.push(0.);
        }
        let changed = Calculation::new(&nodes, "test", None, Some(c.checkpoint)).unwrap();
        assert!(changed.checkpoint.affected.iter().all(|a| *a));
    }
}
