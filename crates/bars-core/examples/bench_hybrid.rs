//! Замер: даст ли второй эмбеддер на процессоре выигрыш рядом с видеокартой.
//! `cargo run --release -p bars-core --example bench_hybrid -- <gpu|cpu|hybrid> <чанков>`.
//! Документы берутся из `docs/`, как их режет индексатор; каждый документ — один вызов `embed`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::Instant;

use bars_core::indexing::chunking::{ChunkingParams, chunk_document};
use bars_core::models::{Accelerator, BuiltinEmbedder, Embedder};

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

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| "hybrid".into());
    let limit: usize = args.next().and_then(|v| v.parse().ok()).unwrap_or(100);

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
    let chars: usize = docs.iter().flatten().map(|t| t.chars().count()).sum();
    eprintln!(
        "документов {}, чанков {total}, символов {chars}",
        docs.len()
    );

    let workers: Vec<Accelerator> = match mode.as_str() {
        "gpu" => vec![Accelerator::Auto],
        "cpu" => vec![Accelerator::Cpu],
        "hybrid" => vec![Accelerator::Auto, Accelerator::Cpu],
        other => panic!("режим: gpu | cpu | hybrid, а не {other}"),
    };

    let models = PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(".bars/models");
    let docs = Arc::new(docs);
    let next = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(workers.len() + 1));

    let handles: Vec<_> = workers
        .into_iter()
        .map(|accelerator| {
            let (docs, next, barrier, models) =
                (docs.clone(), next.clone(), barrier.clone(), models.clone());
            std::thread::spawn(move || {
                let mut embedder = BuiltinEmbedder::bge_m3(&models, accelerator).unwrap();
                let device = embedder.device();
                // Прогрев: первая пачка на DirectML компилирует шейдеры.
                let warm: Vec<&str> = docs[0].iter().take(2).map(String::as_str).collect();
                embedder.embed(&warm).unwrap();
                barrier.wait();
                let mut done_chunks = 0;
                let mut done_docs = 0;
                loop {
                    let index = next.fetch_add(1, Ordering::SeqCst);
                    let Some(texts) = docs.get(index) else { break };
                    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
                    embedder.embed(&refs).unwrap();
                    done_chunks += texts.len();
                    done_docs += 1;
                }
                (device, done_docs, done_chunks)
            })
        })
        .collect();

    barrier.wait();
    let started = Instant::now();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let secs = started.elapsed().as_secs_f64();
    for (device, d, c) in &results {
        println!("  {device:?}: документов {d}, чанков {c}");
    }
    println!(
        "{mode}: {total} чанков за {secs:.1} с = {:.2} чанка/с",
        total as f64 / secs
    );
}
