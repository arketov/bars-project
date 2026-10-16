//! Замер ISS-09: задержка эмбеддинга запроса в покое и во время индексации на одной модели.
//! `cargo run --release -p bars-worker --example bench_search_during_index -- <чанков>`.
//! Индексация — по документу за вызов `embed`, как в индексаторе; запросы идут через ту же
//! `SharedEmbedder`, что и поиск воркера. Модель bge-m3 берётся из `~/.bars/models`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use bars_core::indexing::chunking::{ChunkingParams, chunk_document};
use bars_core::models::Embedder;
use bars_core::settings::Settings;
use bars_worker::worker::models::{ConfiguredProvider, Models};

fn markdown_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            markdown_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "md") {
            out.push(path);
        }
    }
}

fn percentile(sorted: &[f64], share: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * share).round() as usize]
}

fn report(label: &str, mut seconds: Vec<f64>) {
    seconds.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "{label}: запросов {}, медиана {:.2} с, p90 {:.2} с, максимум {:.2} с",
        seconds.len(),
        percentile(&seconds, 0.5),
        percentile(&seconds, 0.9),
        seconds.last().unwrap()
    );
}

fn main() {
    let limit: usize = std::env::args()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    let mut files = Vec::new();
    markdown_files(&root, &mut files);
    let mut docs: Vec<Vec<String>> = Vec::new();
    let mut total = 0;
    for file in files {
        let source = std::fs::read_to_string(&file).unwrap();
        let chunked = chunk_document(
            &file.display().to_string(),
            &source,
            &ChunkingParams::default(),
        );
        let texts: Vec<String> = chunked.chunks.into_iter().map(|c| c.embed_text).collect();
        if texts.is_empty() {
            continue;
        }
        total += texts.len();
        docs.push(texts);
        if total >= limit {
            break;
        }
    }
    eprintln!("документов {}, чанков {total}", docs.len());

    let models_dir = PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(".bars/models");
    let models = Models::new(Arc::new(ConfiguredProvider::new(models_dir)));
    let settings = Settings::resolve(&Default::default(), &Default::default());
    let mut searcher = models.embedder(&settings).unwrap();
    searcher.prepare().unwrap();

    let queries = [
        "как bars режет документы на чанки",
        "гибридный поиск и слияние RRF",
        "очередь индексации воркера",
        "ключи провайдеров и секреты",
    ];
    let mut idle = Vec::new();
    for round in 0..8 {
        let started = Instant::now();
        searcher
            .embed_query(queries[round % queries.len()])
            .unwrap();
        idle.push(started.elapsed().as_secs_f64());
    }
    report("в покое      ", idle);

    let mut indexer = models.embedder(&settings).unwrap();
    let done = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&done);
    let started = Instant::now();
    let indexing = std::thread::spawn(move || {
        for doc in &docs {
            let texts: Vec<&str> = doc.iter().map(String::as_str).collect();
            indexer.embed(&texts).unwrap();
        }
        flag.store(true, Ordering::SeqCst);
    });
    let mut busy = Vec::new();
    let mut round = 0;
    while !done.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(300));
        let begun = Instant::now();
        searcher
            .embed_query(queries[round % queries.len()])
            .unwrap();
        busy.push(begun.elapsed().as_secs_f64());
        round += 1;
    }
    indexing.join().unwrap();
    println!(
        "индексация {total} чанков: {:.1} с",
        started.elapsed().as_secs_f64()
    );
    report("при индексации", busy);
}
