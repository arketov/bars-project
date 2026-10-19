use std::fs;

use super::*;
use crate::indexing::chunking::ChunkingParams;
use crate::indexing::indexer::{IndexSettings, update_index};
use crate::models::{EmbedError, Embedder, ModelFingerprint};
use crate::storage::Journal;

const DIM: usize = 16;

/// Мешок слов; слова-маркеры «темаа» и «темаб» перевешивают остальное, поэтому два текста с одним
/// маркером близки по смыслу при любых других словах.
struct TopicEmbedder {
    fingerprint: ModelFingerprint,
}

impl Embedder for TopicEmbedder {
    fn fingerprint(&self) -> &ModelFingerprint {
        &self.fingerprint
    }

    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        Ok(texts
            .iter()
            .map(|text| {
                let mut vector = vec![0.0; DIM];
                for word in text.to_lowercase().split(|c: char| !c.is_alphanumeric()) {
                    if word.is_empty() {
                        continue;
                    }
                    let weight = if word == "темаа" || word == "темаб" { 400.0 } else { 1.0 };
                    let bucket = if word == "темаа" {
                        0
                    } else if word == "темаб" {
                        1
                    } else {
                        2 + blake3::hash(word.as_bytes()).as_bytes()[0] as usize % (DIM - 2)
                    };
                    vector[bucket] += weight;
                }
                vector
            })
            .collect())
    }
}

/// Проект из файлов `(путь от docs, текст)`; индекс построен настоящим индексатором.
fn project(files: &[(&str, String)]) -> (tempfile::TempDir, Index) {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let full = dir.path().join("docs").join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, format!("# Раздел\n\n{text}\n")).unwrap();
    }
    let mut index = Index::open(&dir.path().join("index.sqlite"), Journal::Wal).unwrap();
    let mut embedder = TopicEmbedder {
        fingerprint: ModelFingerprint {
            source: "builtin".into(),
            model: "topic".into(),
            dim: DIM,
            doc_prefix: String::new(),
        },
    };
    let settings = IndexSettings {
        chunking: ChunkingParams::default(),
        docs_root: "docs".into(),
    };
    update_index(dir.path(), &mut index, &settings, &mut embedder).unwrap();
    (dir, index)
}

fn letters(mut number: usize) -> String {
    let mut out = String::new();
    loop {
        out.push(char::from_u32('а' as u32 + (number % 26) as u32).unwrap());
        number /= 26;
        if number == 0 {
            return out;
        }
    }
}

/// Связный по виду текст без цифр; у разных `seed` общих слов нет.
fn prose(seed: &str, words: usize) -> String {
    (0..words)
        .map(|at| format!("{seed}{}", letters(at)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn audit(index: &Index) -> DuplicatesResponse {
    duplicates(index, &SearchSettings::default(), &DuplicatesRequest::default()).unwrap()
}

fn base() -> String {
    format!(
        "{} Таймаут запроса равен 10 секундам и не повторяется. {}",
        prose("альфа", 20),
        prose("бета", 20)
    )
}

#[test]
fn exact_copies_ignore_case_spacing_and_markup() {
    let text = base();
    let (_dir, index) = project(&[
        ("a/one.md", text.clone()),
        ("b/two.md", format!("**{}**", text.to_uppercase().replace(' ', "  "))),
        ("c/other.md", prose("гамма", 40)),
    ]);
    let response = audit(&index);
    assert_eq!(response.items.len(), 1, "{response:?}");
    let item = &response.items[0];
    assert_eq!(item.kind, DuplicateKind::Exact);
    assert_eq!(item.chunks.len(), 2);
    assert_eq!(item.score, 1.0);
    assert!(!item.repeated_block);
    assert!(item.differences.is_none());
    assert_eq!(response.counts.exact, 1);
    assert_eq!(response.chunks_checked, 3);
}

#[test]
fn identical_block_in_four_sections_is_a_repeated_block_group() {
    let text = base();
    let files: Vec<(String, String)> = (0..4).map(|n| (format!("m/f{n}.md"), text.clone())).collect();
    let refs: Vec<(&str, String)> = files.iter().map(|(p, t)| (p.as_str(), t.clone())).collect();
    let (_dir, index) = project(&refs);
    let response = audit(&index);
    assert_eq!(response.items.len(), 1);
    assert_eq!(response.items[0].chunks.len(), 4);
    assert!(response.items[0].repeated_block);
}

#[test]
fn changed_number_is_a_near_copy_that_shows_the_number() {
    let text = base();
    let (_dir, index) = project(&[
        ("a/one.md", text.clone()),
        ("b/two.md", text.replace("10 секундам", "30 секундам")),
    ]);
    let response = audit(&index);
    assert_eq!(response.items.len(), 1, "{response:?}");
    let item = &response.items[0];
    assert_eq!(item.kind, DuplicateKind::Near);
    assert!(item.score >= 0.6 && item.score < 1.0);
    let differences = item.differences.as_ref().unwrap();
    assert_eq!(differences.numbers_only_first, ["10"]);
    assert_eq!(differences.numbers_only_second, ["30"]);
    assert_eq!(
        differences.changes,
        [Change { first: "10".into(), second: "30".into() }]
    );
    assert!(!item.shared.is_empty());
}

#[test]
fn dropped_negation_is_a_near_copy_that_shows_the_word() {
    let text = base();
    let (_dir, index) = project(&[
        ("a/one.md", text.clone()),
        ("b/two.md", text.replace(" и не повторяется", " и повторяется")),
    ]);
    let response = audit(&index);
    assert_eq!(response.items.len(), 1, "{response:?}");
    let differences = response.items[0].differences.as_ref().unwrap();
    assert_eq!(differences.negations, (1, 0));
    assert_eq!(
        differences.changes,
        [Change { first: "не".into(), second: String::new() }]
    );
}

#[test]
fn half_of_a_text_is_a_partial_copy() {
    let first = format!("{} {}", prose("дельта", 40), prose("эпсилон", 40));
    let second = format!("{} {}", prose("зета", 30), prose("эпсилон", 40));
    let (_dir, index) = project(&[("a/one.md", first), ("b/two.md", second)]);
    let response = audit(&index);
    assert_eq!(response.items.len(), 1, "{response:?}");
    let item = &response.items[0];
    assert_eq!(item.kind, DuplicateKind::Partial);
    assert!(item.score >= 0.25 && item.score < 0.9);
    assert!(item.shared[0].starts_with("эпсилон"));
    assert!(item.differences.as_ref().unwrap().changes.is_empty());
}

#[test]
fn a_section_inside_a_bigger_one_is_contained() {
    let small = prose("тета", 40);
    let big = format!("{} {} {}", prose("йота", 40), small, prose("каппа", 40));
    let (_dir, index) = project(&[("a/small.md", small), ("b/big.md", big)]);
    let response = audit(&index);
    assert_eq!(response.items.len(), 1, "{response:?}");
    assert_eq!(response.items[0].kind, DuplicateKind::Contained);
}

#[test]
fn shared_template_does_not_make_different_sections_duplicates() {
    let template = prose("шаблон", 40);
    let files: Vec<(String, String)> = (0..5)
        .map(|n| (format!("m/t{n}.md"), format!("{template} {}", prose(&format!("тело{}", letters(n)), 40))))
        .collect();
    let refs: Vec<(&str, String)> = files.iter().map(|(p, t)| (p.as_str(), t.clone())).collect();
    let (_dir, index) = project(&refs);
    let response = audit(&index);
    assert!(response.items.is_empty(), "{response:?}");
    assert_eq!(response.total, 0);
}

#[test]
fn semantic_candidate_needs_both_meaning_and_some_shared_words() {
    let phrase = "порядок работы сервиса задан отдельным решением владельца проекта сегодня";
    let (_dir, index) = project(&[
        ("a/one.md", format!("темаа {} {phrase} {}", prose("ро", 20), prose("сигма", 20))),
        ("b/two.md", format!("темаа {} {phrase} {}", prose("тау", 20), prose("фи", 20))),
        ("c/three.md", format!("темаб {} {phrase} {}", prose("хи", 20), prose("пси", 20))),
        ("d/four.md", format!("темаа {} {}", prose("омега", 20), prose("ню", 20))),
    ]);
    let response = audit(&index);
    assert_eq!(response.items.len(), 1, "{response:?}");
    let item = &response.items[0];
    assert_eq!(item.kind, DuplicateKind::Semantic);
    assert!(item.score >= 0.8);
    let paths: Vec<&str> = item.chunks.iter().map(|chunk| chunk.path.as_str()).collect();
    assert_eq!(paths, ["docs/a/one.md", "docs/b/two.md"]);
    assert!(item.shared.is_empty());
}

#[test]
fn module_filter_limits_the_area_and_unknown_module_is_invalid() {
    let text = base();
    let (_dir, index) = project(&[
        ("a/one.md", text.clone()),
        ("b/two.md", text.clone()),
        ("b/three.md", prose("лямбда", 40)),
    ]);
    let ask = |modules: &[&str]| {
        duplicates(
            &index,
            &SearchSettings::default(),
            &DuplicatesRequest {
                modules: modules.iter().map(|module| module.to_string()).collect(),
                ..DuplicatesRequest::default()
            },
        )
    };
    assert!(ask(&["a"]).unwrap().items.is_empty());
    assert_eq!(ask(&["a", "b"]).unwrap().items.len(), 1);
    assert!(matches!(ask(&["nope"]), Err(SearchError::InvalidParams(_))));
}

#[test]
fn short_sections_are_skipped_and_reported() {
    let (_dir, index) = project(&[
        ("a/one.md", "Коротко про таймаут.".into()),
        ("b/two.md", "Коротко про таймаут.".into()),
        ("c/three.md", prose("мю", 40)),
    ]);
    let response = audit(&index);
    assert!(response.items.is_empty());
    assert_eq!(response.chunks_skipped_short, 2);
    assert_eq!(response.chunks_checked, 1);
}

#[test]
fn exact_goes_first_then_by_strength_and_limit_is_clamped() {
    let text = base();
    let same = prose("кси", 40);
    let (_dir, index) = project(&[
        ("a/one.md", text.clone()),
        ("b/two.md", text.replace("10 секундам", "30 секундам")),
        ("c/three.md", same.clone()),
        ("d/four.md", same),
    ]);
    let response = audit(&index);
    let kinds: Vec<DuplicateKind> = response.items.iter().map(|item| item.kind).collect();
    assert_eq!(kinds, [DuplicateKind::Exact, DuplicateKind::Near]);
    assert_eq!(response.total, 2);

    let one = duplicates(
        &index,
        &SearchSettings::default(),
        &DuplicatesRequest { results: Some(1), ..DuplicatesRequest::default() },
    )
    .unwrap();
    assert_eq!(one.items.len(), 1);
    assert_eq!(one.total, 2);
    assert!(!one.clamped);

    let huge = duplicates(
        &index,
        &SearchSettings::default(),
        &DuplicatesRequest { results: Some(100_000), ..DuplicatesRequest::default() },
    )
    .unwrap();
    assert!(huge.clamped);
    assert_eq!(huge.results, SearchSettings::default().max_results);
}

#[test]
fn own_text_drops_headings_and_the_service_header_list() {
    let text = "## Заголовок\nТекст раздела\n```\n# комментарий в коде\n```\nother headers:\n### Вложенный\n";
    let own = own_text(&format!("\n{text}"));
    assert!(own.contains("Текст раздела"));
    assert!(own.contains("# комментарий в коде"));
    assert!(!own.contains("Заголовок"));
    assert!(!own.contains("Вложенный"));
}
