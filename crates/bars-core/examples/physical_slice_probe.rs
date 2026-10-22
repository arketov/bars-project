//! Проба физической проекции на готовом индексе: только чтение, без моделей и переиндексации.
//!
//! `cargo run --release -p bars-core --example physical_slice_probe -- <проект> [<проект>…]`
//! С `PHYSICAL_PROBE_DUMP=<папка>` выгружает срезы целиком (`<имя проекта>.json` и `-expanded.json`).
//! Печатает JSON: число документов и чанков, покрытие centroid, распределение cosine, размер среза
//! по умолчанию и время каждого шага. Индекс открывается `SQLITE_OPEN_READ_ONLY`.

use bars_core::graph::centroid::document_similarity;
use bars_core::graph::physical::{
    ExpandedDocument, Layer, LayerInputs, PhysicalLimits, PhysicalRequest, project,
};
use bars_core::graph::{LayerState, store};
use bars_core::storage::{Index, index_path};
use serde_json::json;
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let mut report = Vec::new();
    for arg in std::env::args().skip(1) {
        let root = PathBuf::from(&arg);
        let index =
            Index::open_read_only(&index_path(&root)).expect("индекс открыт только для чтения");
        let started = Instant::now();
        let data = match store::load(index.conn()) {
            Ok(data) => data,
            Err(error) => {
                report.push(json!({"project": arg, "error": error.to_string()}));
                continue;
            }
        };
        let load_ms = started.elapsed().as_millis();
        let started = Instant::now();
        let similarity = document_similarity(&index, &data, || false).expect("centroid");
        let centroid_ms = started.elapsed().as_millis();
        let mut buckets = [0usize; 10];
        for pair in &similarity.pairs {
            let slot = ((pair.cosine.clamp(0.0, 0.999) * 10.0) as usize).min(9);
            buckets[slot] += 1;
        }
        let ready = |note: &str| Layer {
            state: LayerState::Ready,
            note: note.into(),
        };
        let inputs = || LayerInputs {
            markdown_link: ready("ссылки"),
            document_similarity: ready("близость"),
            chunk_similarity: ready("чанки"),
        };
        let started = Instant::now();
        let slice = project(
            &data,
            &similarity.pairs,
            &PhysicalRequest::default(),
            inputs(),
        )
        .expect("срез");
        let slice_ms = started.elapsed().as_millis();
        let biggest = slice
            .nodes
            .iter()
            .max_by_key(|n| n.chunks)
            .map(|n| (n.id.clone(), n.label.clone(), n.chunks));
        let dump = std::env::var_os("PHYSICAL_PROBE_DUMP").map(PathBuf::from);
        let name = root
            .file_name()
            .map_or_else(|| "project".into(), |n| n.to_string_lossy().to_string());
        if let Some(dir) = &dump {
            std::fs::create_dir_all(dir).expect("папка выгрузки");
            std::fs::write(
                dir.join(format!("{name}.json")),
                serde_json::to_vec(&slice).unwrap(),
            )
            .expect("выгрузка среза");
        }
        let expanded = biggest.as_ref().map(|(id, _, _)| {
            let request = PhysicalRequest {
                expanded: vec![ExpandedDocument {
                    document: id.clone(),
                    shown: 50,
                }],
                ..Default::default()
            };
            let started = Instant::now();
            let slice = project(&data, &similarity.pairs, &request, inputs()).expect("раскрытие");
            if let Some(dir) = &dump {
                std::fs::write(
                    dir.join(format!("{name}-expanded.json")),
                    serde_json::to_vec(&slice).unwrap(),
                )
                .expect("выгрузка раскрытия");
            }
            (
                slice.shown.chunks,
                slice.edges.len(),
                slice.used_bytes,
                started.elapsed().as_millis(),
            )
        });
        let title = |id: &str| {
            data.sources
                .iter()
                .find(|s| s.meta.reference.id == id)
                .map_or(id.to_string(), |s| s.meta.docs_path.clone())
        };
        let mut sorted = similarity.pairs.clone();
        sorted.sort_by(|a, b| b.cosine.total_cmp(&a.cosine));
        let sample = |list: &[bars_core::graph::physical::DocumentPair]| {
            list.iter()
                .map(|p| {
                    json!([
                        title(&p.a),
                        title(&p.b),
                        (p.cosine * 1000.0).round() / 1000.0
                    ])
                })
                .collect::<Vec<_>>()
        };
        let weakest: Vec<_> = sorted.iter().rev().take(5).cloned().collect();
        let links = slice
            .edges
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    bars_core::graph::physical::PhysicalEdgeKind::MarkdownLink
                )
            })
            .count();
        report.push(json!({
            "project": arg,
            "snapshot": data.snapshot,
            "documents": similarity.documents,
            "covered": similarity.covered,
            "skipped": similarity.skipped.iter().take(10).collect::<Vec<_>>(),
            "pairs_directed": similarity.pairs.len(),
            "cosine_buckets_0_to_1_step_0_1": buckets,
            "strongest": sample(&sorted[..sorted.len().min(5)]),
            "weakest": sample(&weakest),
            "default_slice": {
                "nodes": slice.nodes.len(),
                "edges": slice.edges.len(),
                "links": links,
                "totals": slice.totals,
                "truncated": slice.truncated,
                "limit_reasons": slice.limit_reasons,
                "used_bytes": slice.used_bytes,
                "limits": PhysicalLimits::INITIAL,
            },
            "expanded_biggest_document": expanded,
            "biggest_document": biggest,
            "ms": {"load": load_ms, "centroid_knn": centroid_ms, "project": slice_ms},
        }));
    }
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
