//! Только чтение готовых векторов: эквивалентность ядра, продолжение и точная правка.
#[path = "../../../../crates/bars-core/src/graph/similarity.rs"]
pub mod similarity;
#[path = "../../../../crates/bars-core/src/graph/similarity_job.rs"]
pub mod similarity_job;
#[cfg(test)]
mod graph {
    pub use crate::similarity;
}
use rusqlite::{Connection, OpenFlags};
use similarity::{KnnOptions, VectorNode, build_knn};
use similarity_job::Calculation;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut reports = Vec::new();
    for path in std::env::args().skip(1) {
        let index = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let mut statement = index.prepare("SELECT d.path,c.ordinal,v.vector FROM chunks c JOIN documents d ON d.id=c.document_id JOIN vectors v ON v.embed_hash=c.embed_hash ORDER BY d.path,c.ordinal")?;
        let mut nodes = Vec::new();
        for row in statement.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, u32>(1)?,
                r.get::<_, Vec<u8>>(2)?,
            ))
        })? {
            let (doc, ordinal, bytes) = row?;
            {
                if bytes.len() % 4 != 0 {
                    return Err("Неверный размер вектора".into());
                }
                let vector = bytes
                    .chunks_exact(4)
                    .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                    .collect();
                nodes.push(VectorNode {
                    id: format!("{doc}:{ordinal}"),
                    vector,
                });
            }
        }
        let started = Instant::now();
        let mut full = Calculation::new(&nodes, "probe", None, None)?;
        let mut quanta = 0;
        loop {
            quanta += 1;
            if full.advance(Duration::from_millis(20), || false) {
                break;
            }
        }
        let full_ms = started.elapsed().as_millis();
        let baseline = build_knn(
            &nodes,
            KnnOptions {
                k: 5,
                min_cosine: 0.5,
                max_comparisons: u64::MAX,
            },
            || false,
        )?;
        assert_eq!(full.neighbors(), baseline.neighbors);
        let mut interrupted = Calculation::new(&nodes, "probe", None, None)?;
        let cancel_started = Instant::now();
        let complete = interrupted.advance(Duration::from_secs(30), || {
            cancel_started.elapsed() >= Duration::from_millis(20)
        });
        let cancel_ms = cancel_started.elapsed().as_millis();
        let checkpoint = serde_json::from_slice(&serde_json::to_vec(&interrupted.checkpoint)?)?;
        let mut resumed = Calculation::new(&nodes, "probe", Some(checkpoint), None)?;
        assert!(resumed.advance(Duration::from_secs(300), || false));
        assert_eq!(resumed.neighbors(), full.neighbors());
        if let Some(first) = nodes.first_mut() {
            first.vector[0] += 0.2;
        }
        let started = Instant::now();
        let mut edited = Calculation::new(&nodes, "probe", None, Some(full.checkpoint.clone()))?;
        assert!(edited.advance(Duration::from_secs(300), || false));
        let edit_ms = started.elapsed().as_millis();
        let baseline = build_knn(
            &nodes,
            KnnOptions {
                k: 5,
                min_cosine: 0.5,
                max_comparisons: u64::MAX,
            },
            || false,
        )?;
        assert_eq!(edited.neighbors(), baseline.neighbors);
        reports.push(serde_json::json!({ "index": path, "chunks": nodes.len(), "dimension": nodes.first().map(|n| n.vector.len()), "full_ms": full_ms, "quanta": quanta, "pairs": full.checkpoint.compared, "edit_ms": edit_ms, "edit_pairs": edited.checkpoint.compared, "cancel_ms": cancel_ms, "completed_before_cancel": complete, "oracle_equal": true, "resume_equal": true, "edit_equal": true }));
    }
    println!("{}", serde_json::to_string_pretty(&reports)?);
    Ok(())
}
