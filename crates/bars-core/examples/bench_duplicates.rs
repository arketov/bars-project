//! Задержка и состав `duplicates` на готовом индексе (только чтение):
//! `cargo run --release -p bars-core --example bench_duplicates -- <корень проекта> [повторов]`.

use std::path::PathBuf;
use std::time::Instant;

use bars_core::search::{DuplicatesRequest, SearchSettings, duplicates};
use bars_core::storage::{Index, index_path};

fn main() {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().expect("корень проекта"));
    let runs: usize = args
        .next()
        .map_or(5, |value| value.parse().expect("число повторов"));
    let index = Index::open_read_only(&index_path(&root)).expect("индекс проекта");
    let settings = SearchSettings::default();
    let request = DuplicatesRequest {
        results: Some(settings.max_results),
        ..DuplicatesRequest::default()
    };
    let mut millis = Vec::new();
    let mut last = None;
    for _ in 0..runs {
        let started = Instant::now();
        last = Some(duplicates(&index, &settings, &request).expect("аудит"));
        millis.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    millis.sort_by(f64::total_cmp);
    let response = last.expect("хотя бы один прогон");
    println!(
        "chunks checked {}, skipped short {}, found {:?}, total {}",
        response.chunks_checked, response.chunks_skipped_short, response.counts, response.total
    );
    println!(
        "ms: min {:.0}, median {:.0}, max {:.0}",
        millis[0],
        millis[millis.len() / 2],
        millis[millis.len() - 1]
    );
    for item in response.items.iter().take(10) {
        let places: Vec<String> = item
            .chunks
            .iter()
            .map(|chunk| format!("{}:{}", chunk.path, chunk.first_line))
            .collect();
        println!(
            "{} {:.3} {}",
            item.kind.as_str(),
            item.score,
            places.join(" | ")
        );
    }
}
