//! Точный top-K без хранения оценок всех N чанков.
use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap, HashSet};

#[derive(Clone, Copy, Debug)]
struct Scored {
    id: i64,
    score: f32,
}

impl Ord for Scored {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .total_cmp(&other.score)
            .then(other.id.cmp(&self.id))
    }
}
impl PartialOrd for Scored {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for Scored {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Scored {}

pub(crate) struct VectorTop {
    limit: usize,
    top: BinaryHeap<Reverse<Scored>>,
    tracked: HashSet<i64>,
    scores: HashMap<i64, f32>,
}

impl VectorTop {
    pub(crate) fn new(limit: usize, tracked: impl IntoIterator<Item = i64>) -> Self {
        Self {
            limit,
            top: BinaryHeap::new(),
            tracked: tracked.into_iter().collect(),
            scores: HashMap::new(),
        }
    }

    pub(crate) fn push(&mut self, id: i64, score: f32) {
        if self.tracked.contains(&id) {
            self.scores.insert(id, score);
        }
        let entry = Scored { id, score };
        if self.top.len() < self.limit {
            self.top.push(Reverse(entry));
        } else if let Some(mut worst) = self.top.peek_mut()
            && entry > worst.0
        {
            *worst = Reverse(entry);
        }
    }

    pub(crate) fn finish(mut self) -> HashMap<i64, f32> {
        for Reverse(entry) in self.top {
            self.scores.insert(entry.id, entry.score);
        }
        self.scores
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_full_sort_and_keeps_text_scores_outside_top() {
        let mut values: Vec<_> = (0..10_000_i64)
            .map(|id| (id, ((id * 7919) % 101) as f32 / 100.))
            .collect();
        values.extend([
            (10_000, -0.0),
            (10_001, 0.0),
            (10_002, f32::NAN),
            (10_003, f32::INFINITY),
        ]);
        for limit in [0, 1, 15, 30, values.len(), values.len() + 1] {
            for reversed in [false, true] {
                if reversed {
                    values.reverse();
                }
                let mut collector = VectorTop::new(limit, [0, 3, 10_000]);
                for &(id, score) in &values {
                    collector.push(id, score);
                }
                let actual = collector.finish();
                let mut expected = values.clone();
                expected.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
                let best: Vec<_> = expected.iter().take(limit).copied().collect();
                assert!(actual.len() <= limit + 3);
                for (id, score) in best
                    .iter()
                    .chain(values.iter().filter(|(id, _)| [0, 3, 10_000].contains(id)))
                {
                    assert_eq!(actual[id].to_bits(), score.to_bits());
                }
                let mut ordered: Vec<_> = actual.into_iter().collect();
                ordered.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
                assert_eq!(
                    ordered.iter().take(limit).map(|v| v.0).collect::<Vec<_>>(),
                    best.iter().map(|v| v.0).collect::<Vec<_>>()
                );
            }
        }
    }
}
