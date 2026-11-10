//! Структура документа на корпусе `docs/` самого bars: номера строк совпадают с диском,
//! заголовки совпадают с теми, что видит нарезка, три сценария Т-26.

use std::path::Path;

use bars_core::doc_path::resolve_doc_path;
use bars_core::indexing::chunking::{ChunkingParams, chunk_document};
use bars_core::indexing::walk::{read_utf8, read_utf8_path, walk_docs};
use bars_core::outline::{OutlineQuery, Under, outline};

fn project() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn outline_lines_match_the_files_and_the_chunker() {
    let walk = walk_docs(&project(), Path::new("docs")).expect("корень docs/ обходится");
    let mut total = 0;
    for file in &walk.files {
        let source = read_utf8(file).expect("документ читается как UTF-8");
        let disk: Vec<&str> = source.lines().collect();
        let headings = outline(&source, &OutlineQuery::default()).unwrap();
        let chunks = chunk_document(&file.path, &source, &ChunkingParams::default()).chunks;

        for heading in &headings {
            let line = disk[heading.line - 1].trim_start();
            let marks = "#".repeat(heading.level);
            assert!(
                line.starts_with(&format!("{marks} ")) && line.contains(&heading.text),
                "{}:{}: на диске {line:?}, outline {heading}",
                file.path,
                heading.line
            );
            // Если чанк начинается с этого заголовка, его цепочка кончается тем же текстом.
            for chunk in chunks.iter().filter(|c| c.first_line == heading.line) {
                assert!(
                    chunk.heading_chain.ends_with(&heading.text),
                    "{}:{}: цепочка {:?}, заголовок {:?}",
                    file.path,
                    heading.line,
                    chunk.heading_chain,
                    heading.text
                );
            }
        }
        total += headings.len();
    }
    assert!(total > 100, "заголовков найдено {total}");
}

#[test]
fn t26_scenarios_on_a_real_document() {
    let project = project();
    let path = resolve_doc_path(
        &project,
        Path::new("docs"),
        "features/agent-tools/mcp-tools.md",
    )
    .expect("путь от корня документации");
    // Путь из выдачи поиска ведёт в тот же файл (OQ-35).
    assert_eq!(
        resolve_doc_path(
            &project,
            Path::new("docs"),
            "docs/features/agent-tools/mcp-tools.md"
        )
        .unwrap(),
        path
    );
    let source = read_utf8_path(&path).unwrap();

    let all = outline(&source, &OutlineQuery::default()).unwrap();
    assert_eq!(all[0].level, 1);

    let outline_heading = all
        .iter()
        .find(|h| h.text.starts_with("Графовые инструменты"))
        .expect("раздел графовых инструментов есть");
    let second = outline(
        &source,
        &OutlineQuery {
            levels: vec![2],
            under: None,
        },
    )
    .unwrap();
    assert!(second.iter().all(|h| h.level == 2));

    let by_chain = Under::Chain(format!("{}/{}", all[0].text, outline_heading.text));
    let inside = |under| {
        outline(
            &source,
            &OutlineQuery {
                levels: vec![],
                under: Some(under),
            },
        )
        .unwrap()
    };
    let inside_chain = inside(by_chain);
    let inside_line = inside(Under::Line(outline_heading.line));
    assert_eq!(inside_chain, inside_line);
    // Это последний раздел документа без подразделов.
    assert!(inside_line.is_empty());

    // Внутри верхнего заголовка — все остальные.
    let inside_root = inside(Under::Line(all[0].line));
    assert_eq!(inside_root.len(), all.len() - 1);
}
