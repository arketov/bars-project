//! Нарезка на корпусе `docs/` самого bars: свойства, которые должны держаться на любом документе.

use std::fs;
use std::path::{Path, PathBuf};

use bars_core::indexing::chunking::{ChunkingParams, chunk_document};

const OTHER_HEADERS: &str = "\nother headers:\n";

fn markdown_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("каталог читается") {
        let path = entry.expect("запись каталога").path();
        if path.is_dir() {
            markdown_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "md") {
            out.push(path);
        }
    }
}

fn corpus() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    let mut files = Vec::new();
    markdown_files(&root, &mut files);
    assert!(files.len() > 20, "корпус docs/ пуст: {}", files.len());
    files
        .into_iter()
        .map(|path| {
            let name = path
                .strip_prefix(&root)
                .expect("файл внутри docs/")
                .to_string_lossy()
                .replace('\\', "/");
            (name, fs::read_to_string(&path).expect("файл читается"))
        })
        .collect()
}

fn check(params: ChunkingParams) {
    for (path, source) in corpus() {
        let line_count = source.lines().count();
        let doc = chunk_document(&path, &source, &params);
        assert!(!doc.title.is_empty(), "{path}: пустой заголовок документа");
        assert!(!doc.chunks.is_empty(), "{path}: нет чанков");

        for (ordinal, chunk) in doc.chunks.iter().enumerate() {
            let at = format!("{path}, чанк {ordinal}");
            assert_eq!(chunk.ordinal, ordinal, "{at}");
            assert!(
                chunk.first_line >= 1 && chunk.first_line <= chunk.last_line,
                "{at}: строки {}–{}",
                chunk.first_line,
                chunk.last_line
            );
            assert!(
                chunk.last_line <= line_count,
                "{at}: строка за концом файла"
            );

            let body = chunk.text.split(OTHER_HEADERS).next().unwrap_or("");
            assert!(
                body.chars().count() <= params.max_chars,
                "{at}: {} символов при максимуме {}",
                body.chars().count(),
                params.max_chars
            );
            assert!(
                chunk
                    .embed_text
                    .starts_with(&format!("path: {path}\n{}\n", chunk.heading_chain)),
                "{at}: текст для эмбеддинга без пути и цепочки"
            );
            assert_eq!(chunk.embed_hash.len(), 64, "{at}");
        }

        // Чанки идут по порядку документа.
        let starts: Vec<usize> = doc.chunks.iter().map(|c| c.first_line).collect();
        assert!(
            starts.windows(2).all(|w| w[0] <= w[1]),
            "{path}: {starts:?}"
        );
    }
}

#[test]
fn corpus_with_default_params() {
    check(ChunkingParams::default());
}

#[test]
fn corpus_with_small_params() {
    check(ChunkingParams {
        min_chars: 80,
        max_chars: 400,
    });
}

#[test]
fn corpus_is_deterministic() {
    let params = ChunkingParams::default();
    for (path, source) in corpus() {
        assert_eq!(
            chunk_document(&path, &source, &params),
            chunk_document(&path, &source, &params),
            "{path}"
        );
    }
}
