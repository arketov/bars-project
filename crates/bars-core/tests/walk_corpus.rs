//! Обход на корпусе `docs/` самого bars: документы находятся, читаются и режутся.

use std::path::Path;

use bars_core::indexing::chunking::{ChunkingParams, chunk_document};
use bars_core::indexing::walk::{ROOT_MODULE, read_utf8, walk_docs};

#[test]
fn walks_the_bars_docs_and_feeds_chunking() {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let walk = walk_docs(&project, Path::new("docs")).expect("корень docs/ обходится");

    assert!(walk.files.len() > 20, "найдено {}", walk.files.len());
    assert!(walk.problems.is_empty(), "{:?}", walk.problems);

    let paths: Vec<&str> = walk.files.iter().map(|f| f.path.as_str()).collect();
    assert!(
        paths.windows(2).all(|w| w[0] < w[1]),
        "порядок путей нарушен"
    );
    assert!(paths.contains(&"docs/Index.md"));
    assert!(paths.contains(&"docs/architecture/stack.md"));
    assert!(
        paths
            .iter()
            .all(|p| p.starts_with("docs/") && p.ends_with(".md"))
    );

    let root_module = walk
        .files
        .iter()
        .find(|f| f.path == "docs/Index.md")
        .unwrap();
    assert_eq!(root_module.module, ROOT_MODULE);
    let stack = walk
        .files
        .iter()
        .find(|f| f.path == "docs/architecture/stack.md")
        .unwrap();
    assert_eq!(stack.module, "architecture");

    for file in &walk.files {
        let source = read_utf8(file).expect("документ читается как UTF-8");
        let document = chunk_document(&file.path, &source, &ChunkingParams::default());
        assert!(!document.chunks.is_empty(), "{}", file.path);
    }
}
