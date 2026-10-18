use std::fs;
use std::path::Path;

use super::*;
use crate::indexing::chunking::ChunkingParams;
use crate::indexing::indexer::{IndexSettings, UpdateOutcome, update_index};
use crate::models::ModelFingerprint;
use crate::storage::Journal;

const DIM: usize = 32;

/// Мешок слов: каждое слово после стемминга падает в одну из `DIM` корзин. Близость выходит
/// осмысленной без модели.
struct BagEmbedder {
    fingerprint: ModelFingerprint,
}

impl BagEmbedder {
    fn new(model: &str) -> Self {
        Self {
            fingerprint: ModelFingerprint {
                source: "builtin".into(),
                model: model.into(),
                dim: DIM,
                doc_prefix: String::new(),
            },
        }
    }
}

impl Embedder for BagEmbedder {
    fn fingerprint(&self) -> &ModelFingerprint {
        &self.fingerprint
    }

    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let stemmer = TextStemmer::new();
        Ok(texts
            .iter()
            .map(|text| {
                let mut vector = vec![0.0; DIM];
                for word in stemmer.stem_text(text).split_whitespace() {
                    let bucket = blake3::hash(word.as_bytes()).as_bytes()[0] as usize % DIM;
                    vector[bucket] += 1.0;
                }
                vector
            })
            .collect())
    }
}

/// Оценивает документ по тому, где он стоит в пачке: последний — выше всех.
struct ReversingReranker;

impl Reranker for ReversingReranker {
    fn rerank(&mut self, _query: &str, documents: &[&str]) -> Result<Vec<f32>, RerankError> {
        Ok((0..documents.len())
            .map(|place| 0.2 + place as f32 / 10.0)
            .collect())
    }
}

/// Высокая оценка только у документа со словом «Корень».
struct KeywordReranker;

impl Reranker for KeywordReranker {
    fn rerank(&mut self, _query: &str, documents: &[&str]) -> Result<Vec<f32>, RerankError> {
        Ok(documents
            .iter()
            .map(|doc| {
                if doc.contains("Корень") {
                    0.9
                } else {
                    0.05
                }
            })
            .collect())
    }
}

struct BrokenReranker;

impl Reranker for BrokenReranker {
    fn rerank(&mut self, _query: &str, _documents: &[&str]) -> Result<Vec<f32>, RerankError> {
        Err(RerankError::Rerank("сервис недоступен".into()))
    }
}

struct WrongCountReranker;

impl Reranker for WrongCountReranker {
    fn rerank(&mut self, _query: &str, _documents: &[&str]) -> Result<Vec<f32>, RerankError> {
        Ok(vec![0.5])
    }
}

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn project() -> (tempfile::TempDir, Index, BagEmbedder) {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "docs/search/hybrid.md",
        "# Гибридный поиск\n\nКандидаты из полнотекстового и векторного поиска сливаются через RRF.\n",
    );
    write(
        dir.path(),
        "docs/search/identifier.md",
        "# Идентификаторы\n\nПараметр zq_widget_limit задаёт предел.\n",
    );
    write(
        dir.path(),
        "docs/models/embed.md",
        "# Модели\n\nEmbed-модель считает векторы для чанков поиска.\n",
    );
    write(
        dir.path(),
        "docs/root.md",
        "# Корень\n\nДокумент корневого модуля про поиск и переиндексацию.\n",
    );
    let mut index = Index::open(&dir.path().join("index.sqlite"), Journal::Wal).unwrap();
    let mut embedder = BagEmbedder::new("bag");
    let settings = IndexSettings {
        chunking: ChunkingParams::default(),
        docs_root: "docs".into(),
    };
    let outcome = update_index(dir.path(), &mut index, &settings, &mut embedder).unwrap();
    assert!(matches!(outcome, UpdateOutcome::Updated(_)), "{outcome:?}");
    (dir, index, embedder)
}

fn request(query: &str) -> SearchRequest {
    SearchRequest {
        query: query.into(),
        ..SearchRequest::default()
    }
}

fn run(
    index: &Index,
    embedder: &mut BagEmbedder,
    request: &SearchRequest,
) -> Result<SearchResponse, SearchError> {
    search(index, embedder, None, &SearchSettings::default(), request)
}

fn paths(response: &SearchResponse) -> Vec<&str> {
    response.hits.iter().map(|hit| hit.path.as_str()).collect()
}

#[test]
fn bounded_cosines_match_full_scan_and_keep_text_candidates() {
    let (_dir, index, _) = project();
    let query = vec![0.125_f32; DIM];
    let scope = Scope::build(&index, &request("поиск")).unwrap();
    let mut statement = index.conn().prepare(
        "SELECT c.id, v.vector FROM chunks c JOIN vectors v ON v.embed_hash=c.embed_hash"
    ).unwrap();
    let full: HashMap<i64, f32> = statement.query_map([], |row| {
        let id = row.get(0)?;
        let blob: Vec<u8> = row.get(1)?;
        let vector = crate::storage::decode_vector(&blob);
        Ok((id, vector.iter().zip(&query).map(|(a,b)| a*b).sum()))
    }).unwrap().collect::<Result<_,_>>().unwrap();
    let mut ids: Vec<_> = full.keys().copied().collect();
    ids.sort();
    for k in [0, 1, 2, 100] {
        let text: Vec<_> = ids.iter().take(k).copied().collect();
        let exact: Vec<_> = ids.iter().rev().take(k).copied().collect();
        let bounded = scope.cosines(&index, &query, k, &text, &exact).unwrap();
        assert!(bounded.len() <= 3*k);
        assert_eq!(top_by_score(&bounded,k), top_by_score(&full,k));
        for (id,score) in &bounded { assert_eq!(score.to_bits(), full[id].to_bits()); }
        for id in text.iter().chain(&exact) { assert_eq!(bounded[id].to_bits(), full[id].to_bits()); }
    }
    assert!(matches!(scope.cosines(&index, &query[..DIM-1], 1, &[], &[]),
        Err(SearchError::VectorSize { expected, got }) if expected == DIM-1 && got == DIM));
}

#[test]
fn exact_identifier_is_found_first() {
    let (_dir, index, mut embedder) = project();

    let response = run(&index, &mut embedder, &request("zq_widget_limit")).unwrap();

    assert_eq!(response.hits[0].path, "docs/search/identifier.md");
}

#[test]
fn hit_carries_everything_the_requirements_ask_for() {
    let (_dir, index, mut embedder) = project();

    let response = run(&index, &mut embedder, &request("zq_widget_limit")).unwrap();

    let hit = &response.hits[0];
    assert_eq!(hit.module, "search");
    // Без frontmatter `title` — имя файла.
    assert_eq!(hit.title, "identifier");
    assert!(hit.heading_chain.contains("Идентификаторы"));
    assert!(hit.first_line >= 1 && hit.last_line >= hit.first_line);
    assert!(hit.text.contains("zq_widget_limit"));
    assert_eq!(hit.score_kind, ScoreKind::Cosine);
    assert!((0.0..=1.0).contains(&hit.score));
}

#[test]
fn stemming_connects_word_forms() {
    let (_dir, index, mut embedder) = project();

    let response = run(&index, &mut embedder, &request("переиндексация")).unwrap();

    assert_eq!(response.hits[0].path, "docs/root.md");
}

#[test]
fn module_filter_keeps_only_that_module() {
    let (_dir, index, mut embedder) = project();
    let mut req = request("поиск");
    req.modules = vec!["models".into()];

    let response = run(&index, &mut embedder, &req).unwrap();

    assert_eq!(paths(&response), ["docs/models/embed.md"]);
}

#[test]
fn root_module_is_selected_by_a_dot() {
    let (_dir, index, mut embedder) = project();
    let mut req = request("поиск");
    req.modules = vec![".".into()];

    let response = run(&index, &mut embedder, &req).unwrap();

    assert_eq!(paths(&response), ["docs/root.md"]);
}

#[test]
fn file_filter_is_relative_to_the_docs_root() {
    let (_dir, index, mut embedder) = project();
    let mut req = request("поиск");
    req.files = vec!["search/hybrid.md".into()];

    let response = run(&index, &mut embedder, &req).unwrap();

    assert_eq!(paths(&response), ["docs/search/hybrid.md"]);
}

#[test]
fn modules_and_files_are_united() {
    let (_dir, index, mut embedder) = project();
    let mut req = request("поиск");
    req.modules = vec!["models".into()];
    req.files = vec!["search/hybrid.md".into()];

    let response = run(&index, &mut embedder, &req).unwrap();

    let mut found = paths(&response);
    found.sort_unstable();
    assert_eq!(found, ["docs/models/embed.md", "docs/search/hybrid.md"]);
}

#[test]
fn empty_selection_searches_the_whole_project() {
    let (_dir, index, mut embedder) = project();

    let response = run(&index, &mut embedder, &request("поиск")).unwrap();

    assert_eq!(response.hits.len(), 4);
}

#[test]
fn unknown_module_or_file_is_invalid_params() {
    let (_dir, index, mut embedder) = project();
    let mut module = request("поиск");
    module.modules = vec!["nope".into()];
    let mut file = request("поиск");
    file.files = vec!["search/nope.md".into()];

    assert!(matches!(
        run(&index, &mut embedder, &module),
        Err(SearchError::InvalidParams(_))
    ));
    assert!(matches!(
        run(&index, &mut embedder, &file),
        Err(SearchError::InvalidParams(_))
    ));
}

#[test]
fn query_without_words_is_invalid_params() {
    let (_dir, index, mut embedder) = project();

    for query in ["", "   ", "?!-"] {
        assert!(
            matches!(
                run(&index, &mut embedder, &request(query)),
                Err(SearchError::InvalidParams(_))
            ),
            "{query:?}"
        );
    }
}

#[test]
fn fts_syntax_in_the_query_is_plain_text() {
    let (_dir, index, mut embedder) = project();

    let response = run(&index, &mut embedder, &request("AND OR NEAR(\" - * поиск:"));

    assert!(response.is_ok(), "{response:?}");
}

#[test]
fn changed_model_requires_reindexing() {
    let (_dir, index, _embedder) = project();
    let mut other = BagEmbedder::new("other");

    let result = run(&index, &mut other, &request("поиск"));

    assert!(matches!(result, Err(SearchError::ReindexRequired)));
}

#[test]
fn empty_index_gives_no_results() {
    let dir = tempfile::tempdir().unwrap();
    let index = Index::open(&dir.path().join("index.sqlite"), Journal::Wal).unwrap();
    let mut embedder = BagEmbedder::new("bag");

    let response = run(&index, &mut embedder, &request("поиск")).unwrap();

    assert!(response.hits.is_empty());
}

#[test]
fn explicit_result_count_is_honoured_and_default_comes_from_settings() {
    let (_dir, index, mut embedder) = project();
    let mut two = request("поиск");
    two.results = Some(2);

    assert_eq!(run(&index, &mut embedder, &two).unwrap().hits.len(), 2);
    let settings = SearchSettings {
        results: 3,
        ..SearchSettings::default()
    };
    let by_default = search(&index, &mut embedder, None, &settings, &request("поиск")).unwrap();
    assert_eq!(by_default.hits.len(), 3);
}

#[test]
fn threshold_drops_results_below_it_and_is_reported() {
    let (_dir, index, mut embedder) = project();
    let mut strict = request("поиск");
    strict.min_score = Some(1.0);

    let response = run(&index, &mut embedder, &strict).unwrap();

    assert!(response.hits.is_empty());
    assert_eq!(response.applied.min_score, 1.0);
}

#[test]
fn agent_threshold_outside_the_limits_is_clamped() {
    let (_dir, index, mut embedder) = project();
    let settings = SearchSettings {
        agent_min_score: (0.0, 0.5),
        ..SearchSettings::default()
    };
    let mut req = request("поиск");
    req.min_score = Some(0.99);

    let response = search(&index, &mut embedder, None, &settings, &req).unwrap();

    assert_eq!(response.applied.min_score, 0.5);
    assert!(response.applied.clamped);
    assert!(response.hits.iter().all(|hit| hit.score >= 0.5));
}

#[test]
fn rerank_orders_by_its_score_and_labels_it() {
    let (_dir, index, mut embedder) = project();
    let mut req = request("поиск");
    req.results = Some(4);
    let plain = run(&index, &mut embedder, &req).unwrap();
    let mut reranker = ReversingReranker;

    let reranked = search(
        &index,
        &mut embedder,
        Some(&mut reranker),
        &SearchSettings::default(),
        &req,
    )
    .unwrap();

    assert_eq!(reranked.hits.len(), 4);
    assert_eq!(reranked.hits[0].path, plain.hits[3].path);
    assert_eq!(reranked.hits[3].path, plain.hits[0].path);
    assert!(
        reranked
            .hits
            .iter()
            .all(|hit| hit.score_kind == ScoreKind::Rerank)
    );
    assert_eq!(reranked.applied.score_kind, ScoreKind::Rerank);
    assert!(reranked.rerank_error.is_none());
}

#[test]
fn rerank_threshold_filters_and_default_count_is_the_reranked_one() {
    let (_dir, index, mut embedder) = project();
    let mut reranker = KeywordReranker;

    let response = search(
        &index,
        &mut embedder,
        Some(&mut reranker),
        &SearchSettings::default(),
        &request("поиск"),
    )
    .unwrap();

    // Порог реранка 0.1 оставляет единственный документ с оценкой 0.9.
    assert_eq!(paths(&response), ["docs/root.md"]);
    assert_eq!(response.hits[0].score, 0.9);
    assert_eq!(response.applied.min_score, 0.1);
    assert_eq!(response.applied.results, 3);
}

#[test]
fn failed_rerank_falls_back_to_cosine_and_says_why() {
    let (_dir, index, mut embedder) = project();

    for reranker in [
        &mut BrokenReranker as &mut dyn Reranker,
        &mut WrongCountReranker,
    ] {
        let response = search(
            &index,
            &mut embedder,
            Some(reranker),
            &SearchSettings::default(),
            &request("поиск"),
        )
        .unwrap();

        assert!(response.rerank_error.is_some());
        assert!(!response.hits.is_empty());
        assert!(
            response
                .hits
                .iter()
                .all(|hit| hit.score_kind == ScoreKind::Cosine)
        );
        assert_eq!(response.applied.score_kind, ScoreKind::Cosine);
        assert_eq!(response.applied.results, 5);
    }
}

#[test]
fn identifier_goes_to_fts_both_as_a_phrase_and_as_words() {
    let query = fts_query("zq_widget_limit").unwrap();
    assert_eq!(
        query.all,
        "\"zq widget limit\" OR \"zq\" OR \"widget\" OR \"limit\""
    );
    assert_eq!(query.identifiers.as_deref(), Some("\"zq widget limit\""));
    // Обычные слова и повтор слов фразы не порождают.
    let plain = fts_query("настройки поиска настройки").unwrap();
    assert_eq!(plain.all, "\"настройк\" OR \"поиск\"");
    assert_eq!(plain.identifiers, None);
    assert_eq!(fts_query("?! --"), None);
}

#[test]
fn only_words_with_inner_separators_count_as_identifiers() {
    for raw in ["agent_min_score", "settings.toml", "a/x.md", "(docs_root),"] {
        assert!(is_identifier(raw), "{raw}");
    }
    // Точка в конце предложения и дефис в слове — не идентификатор.
    for raw in ["поиск.", "по-русски", "слово", "..."] {
        assert!(!is_identifier(raw), "{raw}");
    }
    // Дефисное слово идёт в FTS фразой, но первым не поднимается.
    let query = fts_query("по-русски").unwrap();
    assert!(query.all.starts_with("\"по русск\""));
    assert_eq!(query.identifiers, None);
}

#[test]
fn chunks_with_the_identifier_go_first_and_keep_their_order() {
    // 5 и 9 содержат идентификатор; 9 выше в слиянии. 2 и 7 — без него.
    let fused = vec![2, 9, 7, 5];
    assert_eq!(exact_first(&[5, 9], fused), [9, 5, 2, 7]);
    // Совпадение, которого нет среди слитых кандидатов, не теряется и идёт после слитых точных.
    assert_eq!(exact_first(&[8, 9], vec![2, 9]), [9, 8, 2]);
    assert_eq!(exact_first(&[], vec![2, 9]), [2, 9]);
}

#[test]
fn fusion_rewards_agreement_between_the_two_lists() {
    // 7 есть в обоих списках, 1 и 3 — по одному.
    let fused = fuse(&[1, 7, 2], &[3, 7, 4]);

    assert_eq!(fused[0], 7);
    assert_eq!(fused.len(), 5);
    // Из одиночных выше те, что стоят первыми в своём списке; при равенстве — тот, кто в
    // полнотекстовом списке.
    assert_eq!(&fused[1..3], [1, 3]);
}

#[test]
fn equal_fusion_scores_are_decided_by_the_full_text_place() {
    // Оба в обоих списках, места зеркальны: суммы равны. Номер чанка 10 меньше, но 20 первый в
    // полнотекстовом списке.
    assert_eq!(fuse(&[20, 10], &[10, 20]), [20, 10]);
}
