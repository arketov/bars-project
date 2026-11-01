use std::fs;
use std::path::Path;

use super::*;
use crate::indexing::chunking::ChunkingParams;
use crate::indexing::indexer::{IndexSettings, UpdateOutcome, update_index};
use crate::models::ModelFingerprint;
use crate::storage::Journal;

const DIM: usize = 32;

#[test]
fn module_candidates_count_the_unique_pool_before_threshold_and_limit() {
    let (_dir, index, mut embedder) = project();
    let mut r = request("поиск");
    r.results = Some(1);
    r.min_score = Some(0.1);
    let expected = [(".".into(), 1), ("models".into(), 1), ("search".into(), 2)].into();
    let reranked = search(
        &index,
        &mut embedder,
        Some(&mut KeywordReranker),
        &SearchSettings::default(),
        &r,
    )
    .unwrap();
    assert_eq!(reranked.candidate_modules, expected);
    assert_eq!(reranked.hits.len(), 1);
    assert_eq!(reranked.hits[0].module, ".");
    let fallback = search(
        &index,
        &mut embedder,
        Some(&mut BrokenReranker),
        &SearchSettings::default(),
        &r,
    )
    .unwrap();
    assert_eq!(fallback.candidate_modules, expected);
    assert!(fallback.rerank_error.is_some());
    r.modules = vec!["models".into()];
    r.files = vec!["root.md".into()];
    let scoped = run(&index, &mut embedder, &r).unwrap();
    assert_eq!(
        scoped.candidate_modules,
        [(".".into(), 1), ("models".into(), 1)].into()
    );
}

#[test]
fn search_pins_snapshot_before_lazy_fts_open_during_schema_publication() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    use std::time::Duration;

    let (_dir, index, mut embedder) = project();
    let path = index.conn().path().unwrap().to_string();
    drop(index);
    let reader = Index::open_read_only(Path::new(&path)).unwrap();
    let writer = rusqlite::Connection::open(&path).unwrap();
    let (start_tx, start_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let publish = std::thread::spawn(move || {
        let mut generation = 0;
        while start_rx.recv_timeout(Duration::from_secs(2)).is_ok() {
            writer
                .execute_batch(&format!(
                    "CREATE TABLE schema_publication_{generation}(id INTEGER)"
                ))
                .unwrap();
            generation += 1;
            done_tx.send(()).unwrap();
        }
        generation
    });
    let active = Arc::new(AtomicBool::new(false));
    let flag = active.clone();
    reader
        .conn()
        .authorizer(Some(move |context: AuthContext<'_>| {
            if let AuthAction::Read { table_name, .. } = context.action
                && table_name == "chunks_fts_config"
            {
                flag.store(true, Ordering::SeqCst);
            }
            Authorization::Allow
        }))
        .unwrap();
    // Переключаем схему точно при исполнении внутренних FTS-запросов, без гонки по sleep.
    let mut publications = 0;
    reader
        .conn()
        .progress_handler(
            1,
            Some(move || {
                if active.load(Ordering::SeqCst) && publications < 512 {
                    publications += 1;
                    start_tx.send(()).unwrap();
                    done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                }
                false
            }),
        )
        .unwrap();
    let response = run(&reader, &mut embedder, &request("векторный"));
    reader
        .conn()
        .progress_handler(0, None::<fn() -> bool>)
        .unwrap();
    reader
        .conn()
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .unwrap();
    assert!(
        publish.join().unwrap() > 0,
        "публикация действительно пересеклась с FTS"
    );
    assert!(!response.unwrap().hits.is_empty());
    assert!(reader.conn().is_autocommit(), "поиск освободил свой снимок");
}

#[test]
fn search_preserves_caller_transaction() {
    let (_dir, index, mut embedder) = project();
    let transaction = index.conn().unchecked_transaction().unwrap();
    assert!(
        !run(&index, &mut embedder, &request("векторный"))
            .unwrap()
            .hits
            .is_empty()
    );
    assert!(!index.conn().is_autocommit());
    transaction.rollback().unwrap();
    assert!(index.conn().is_autocommit());
}

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
fn graph_expansion_precedes_rerank_and_keeps_scope_and_threshold() {
    let (_dir, index, mut embedder) = project();
    let root:i64=index.conn().query_row("SELECT c.id FROM chunks c JOIN documents d ON d.id=c.document_id WHERE d.path='docs/root.md'",[],|r|r.get(0)).unwrap();
    let mut expand = |_: &Index, _: &[i64], _: usize| Ok(vec![root]);
    let mut r = request("векторы");
    r.candidates = Some(1);
    r.rerank_candidates = Some(10);
    r.results = Some(1);
    r.min_score = Some(0.1);
    let out = search_expanded(
        &index,
        &mut embedder,
        Some(&mut KeywordReranker),
        &SearchSettings::default(),
        &r,
        Some(&mut expand),
    )
    .unwrap();
    assert_eq!(out.hits.len(), 1);
    assert_eq!(out.hits[0].path, "docs/root.md");
    assert_eq!(out.hits[0].score, 0.9);
    r.min_score = Some(0.95);
    assert!(
        search_expanded(
            &index,
            &mut embedder,
            Some(&mut KeywordReranker),
            &SearchSettings::default(),
            &r,
            Some(&mut expand)
        )
        .unwrap()
        .hits
        .is_empty()
    );
    r.min_score = Some(0.1);
    r.modules = vec!["models".into()];
    assert!(
        search_expanded(
            &index,
            &mut embedder,
            Some(&mut KeywordReranker),
            &SearchSettings::default(),
            &r,
            Some(&mut expand)
        )
        .unwrap()
        .hits
        .is_empty()
    );
}

#[test]
fn graph_context_reuses_one_query_embedding_and_respects_role_scope() {
    use crate::graph::*;
    struct CountQuery<'a> {
        inner: &'a mut BagEmbedder,
        calls: usize,
    }
    impl Embedder for CountQuery<'_> {
        fn fingerprint(&self) -> &ModelFingerprint {
            self.inner.fingerprint()
        }
        fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
            self.inner.embed(texts)
        }
        fn embed_query(&mut self, text: &str) -> Result<Vec<f32>, EmbedError> {
            self.calls += 1;
            self.inner.embed_query(text)
        }
    }
    let (dir, mut index, mut embedder) = project();
    write(
        dir.path(),
        "docs/models/embed.md",
        "# Ограничения модели\n\nEmbed-модель считает векторы для чанков поиска.\n",
    );
    update_index(
        dir.path(),
        &mut index,
        &IndexSettings {
            chunking: ChunkingParams::default(),
            docs_root: "docs".into(),
        },
        &mut embedder,
    )
    .unwrap();
    build_index_graph(dir.path(), &mut index, || false).unwrap();
    let data = store::load(index.conn()).unwrap();
    let request = GraphRequest {
        operation: Operation::Context,
        query: Some("модель векторы".into()),
        scope: Scope {
            modules: vec!["models".into()],
            files: vec![],
        },
        budgets: Budgets {
            max_nodes: Some(2000),
            timeout_ms: Some(5000),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut counted = CountQuery {
        inner: &mut embedder,
        calls: 0,
    };
    let (scores, limited) = context::evidence_scores(
        &index,
        &data,
        &mut counted,
        &SearchSettings::default(),
        &request,
    )
    .unwrap();
    assert_eq!(counted.calls, 1);
    assert!(!limited);
    assert!(!scores.is_empty());
    for case in 0..3 {
        let mut invalid = request.clone();
        match case {
            0 => invalid.query = Some(" ".into()),
            1 => invalid.scope.modules = vec!["missing".into()],
            _ => invalid.limit = Some(0),
        }
        assert!(
            context::evidence_scores(
                &index,
                &data,
                &mut counted,
                &SearchSettings::default(),
                &invalid
            )
            .is_err()
        );
    }
    assert_eq!(
        counted.calls, 1,
        "неверный вход не запускает query embedding"
    );
    let out = query::execute_ranked(&data, &request, &scores, std::time::Duration::ZERO, || {
        false
    })
    .unwrap();
    assert!(!out.items.is_empty());
    assert!(out.sources.iter().all(|s| s.module == "models"));
    let readonly = Index::open_read_only(&dir.path().join("index.sqlite")).unwrap();
    assert!(
        readonly
            .conn()
            .execute("DELETE FROM documents", [])
            .is_err()
    );
}

#[test]
fn bounded_cosines_match_full_scan_and_keep_text_candidates() {
    let (_dir, index, _) = project();
    let query = vec![0.125_f32; DIM];
    let scope = Scope::build(&index, &request("поиск")).unwrap();
    let mut statement = index
        .conn()
        .prepare("SELECT c.id, v.vector FROM chunks c JOIN vectors v ON v.embed_hash=c.embed_hash")
        .unwrap();
    let full: HashMap<i64, f32> = statement
        .query_map([], |row| {
            let id = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            let vector = crate::storage::decode_vector(&blob);
            Ok((id, vector.iter().zip(&query).map(|(a, b)| a * b).sum()))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut ids: Vec<_> = full.keys().copied().collect();
    ids.sort();
    for k in [0, 1, 2, 100] {
        let text: Vec<_> = ids.iter().take(k).copied().collect();
        let exact: Vec<_> = ids.iter().rev().take(k).copied().collect();
        let bounded = scope.cosines(&index, &query, k, &text, &exact).unwrap();
        assert!(bounded.len() <= 3 * k);
        assert_eq!(top_by_score(&bounded, k), top_by_score(&full, k));
        for (id, score) in &bounded {
            assert_eq!(score.to_bits(), full[id].to_bits());
        }
        for id in text.iter().chain(&exact) {
            assert_eq!(bounded[id].to_bits(), full[id].to_bits());
        }
    }
    assert!(
        matches!(scope.cosines(&index, &query[..DIM-1], 1, &[], &[]),
        Err(SearchError::VectorSize { expected, got }) if expected == DIM-1 && got == DIM)
    );
}

#[test]
fn streamed_cosines_keep_shared_vectors_and_skip_missing_and_orphan_vectors() {
    let (_dir, index, _) = project();
    index.conn().execute_batch(
        "UPDATE chunks SET embed_hash=(SELECT c.embed_hash FROM chunks c JOIN documents d ON d.id=c.document_id WHERE d.path='docs/search/hybrid.md')
         WHERE document_id=(SELECT id FROM documents WHERE path='docs/models/embed.md');
         DELETE FROM vectors WHERE embed_hash=(SELECT c.embed_hash FROM chunks c JOIN documents d ON d.id=c.document_id WHERE d.path='docs/root.md');
         INSERT INTO vectors VALUES('orphan',zeroblob(4));",
    ).unwrap();
    let query = vec![0.125_f32; DIM];
    let all = Scope::build(&index, &request("поиск"))
        .unwrap()
        .cosines(&index, &query, 100, &[], &[])
        .unwrap();
    assert_eq!(all.len(), 3);
    let mut scoped = request("поиск");
    scoped.modules = vec!["search".into()];
    let selected = Scope::build(&index, &scoped)
        .unwrap()
        .cosines(&index, &query, 100, &[], &[])
        .unwrap();
    assert_eq!(selected.len(), 2);
    for (id, score) in selected {
        assert_eq!(score.to_bits(), all[&id].to_bits());
    }
    let scores: Vec<_> = all.values().map(|score| score.to_bits()).collect();
    assert!(
        scores
            .iter()
            .any(|score| scores.iter().filter(|s| *s == score).count() >= 2)
    );
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
fn file_filter_accepts_the_path_printed_in_results() {
    let (_dir, index, mut embedder) = project();
    let mut req = request("поиск");
    req.files = vec!["docs/search/hybrid.md".into(), ".\\docs\\root.md".into()];

    let response = run(&index, &mut embedder, &req).unwrap();

    let mut found = paths(&response);
    found.sort_unstable();
    assert_eq!(found, ["docs/root.md", "docs/search/hybrid.md"]);
}

#[test]
fn unknown_scope_errors_say_how_to_fix_them() {
    let (_dir, index, mut embedder) = project();
    let mut module = request("поиск");
    module.modules = vec!["nope".into()];
    let mut file = request("поиск");
    file.files = vec!["hybrid.md".into()];

    let Err(SearchError::InvalidParams(message)) = run(&index, &mut embedder, &module) else {
        panic!("ожидалась ошибка модуля");
    };
    assert!(
        message.contains("есть: ") && message.contains("models"),
        "{message}"
    );
    let Err(SearchError::InvalidParams(message)) = run(&index, &mut embedder, &file) else {
        panic!("ожидалась ошибка файла");
    };
    assert!(message.contains("возможно: search/hybrid.md"), "{message}");
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

mod similar_chunks {
    use super::*;
    use crate::search::{SimilarRequest, similar};

    fn settings() -> IndexSettings {
        IndexSettings {
            chunking: ChunkingParams::default(),
            docs_root: "docs".into(),
        }
    }

    fn id_of(index: &Index, embedder: &mut BagEmbedder, query: &str) -> String {
        let response = run(index, embedder, &request(query)).unwrap();
        response.hits[0].public_id.clone()
    }

    fn ask(index: &Index, id: &str) -> Result<crate::search::SimilarResponse, SearchError> {
        similar(
            index,
            &SearchSettings::default(),
            &SimilarRequest {
                id: id.into(),
                ..SimilarRequest::default()
            },
        )
    }

    #[test]
    fn search_hits_carry_an_id_the_similar_tool_accepts() {
        let (_dir, index, mut embedder) = project();
        let id = id_of(&index, &mut embedder, "Кандидаты RRF слияние");
        let (number, check) = id.split_once('-').unwrap();
        assert!(number.parse::<i64>().unwrap() > 0);
        assert_eq!(check.len(), 8);
        let response = ask(&index, &id).unwrap();
        assert_eq!(response.source.public_id, id);
        assert_eq!(response.source.path, "docs/search/hybrid.md");
    }

    #[test]
    fn neighbours_exclude_the_source_and_are_ordered_by_cosine() {
        let (_dir, index, mut embedder) = project();
        let id = id_of(&index, &mut embedder, "Кандидаты RRF слияние");
        let response = ask(&index, &id).unwrap();
        assert_eq!(response.hits.len(), 3);
        assert!(response.hits.iter().all(|hit| hit.public_id != id));
        assert!(
            response
                .hits
                .iter()
                .all(|hit| hit.score_kind == ScoreKind::Cosine && (0.0..=1.0).contains(&hit.score))
        );
        assert!(
            response
                .hits
                .windows(2)
                .all(|pair| pair[0].score >= pair[1].score)
        );
        assert!(!response.clamped);
    }

    #[test]
    fn limit_is_applied_and_clamped_to_the_maximum() {
        let (_dir, index, mut embedder) = project();
        let id = id_of(&index, &mut embedder, "Кандидаты RRF слияние");
        let one = similar(
            &index,
            &SearchSettings::default(),
            &SimilarRequest {
                id: id.clone(),
                results: Some(1),
                ..SimilarRequest::default()
            },
        )
        .unwrap();
        assert_eq!(one.hits.len(), 1);
        let huge = similar(
            &index,
            &SearchSettings::default(),
            &SimilarRequest {
                id,
                results: Some(500),
                ..SimilarRequest::default()
            },
        )
        .unwrap();
        assert!(huge.clamped);
        assert_eq!(huge.results, 20);
    }

    #[test]
    fn module_filter_limits_candidates_but_not_the_source() {
        let (_dir, index, mut embedder) = project();
        let id = id_of(&index, &mut embedder, "Кандидаты RRF слияние");
        let response = similar(
            &index,
            &SearchSettings::default(),
            &SimilarRequest {
                id: id.clone(),
                modules: vec!["models".into()],
                ..SimilarRequest::default()
            },
        )
        .unwrap();
        assert_eq!(response.source.module, "search");
        assert_eq!(response.hits.len(), 1);
        assert_eq!(response.hits[0].module, "models");
        let error = similar(
            &index,
            &SearchSettings::default(),
            &SimilarRequest {
                id,
                modules: vec!["нет-такого".into()],
                ..SimilarRequest::default()
            },
        )
        .unwrap_err();
        assert!(matches!(error, SearchError::InvalidParams(_)));
    }

    #[test]
    fn malformed_unknown_and_forged_ids_are_rejected() {
        let (_dir, index, mut embedder) = project();
        let id = id_of(&index, &mut embedder, "Кандидаты RRF слияние");
        for bad in [
            "",
            "12",
            "x-00000000",
            "5-xyz",
            "5-0000000",
            "-5-00000000",
            "0-00000000",
            "5-ABCDEF00",
        ] {
            assert!(
                matches!(ask(&index, bad), Err(SearchError::InvalidParams(_))),
                "{bad}"
            );
        }
        assert!(matches!(
            ask(&index, "99999-00000000"),
            Err(SearchError::ChunkNotFound(_))
        ));
        let (number, _) = id.split_once('-').unwrap();
        assert!(matches!(
            ask(&index, &format!("{number}-00000000")),
            Err(SearchError::ChunkNotFound(_))
        ));
    }

    #[test]
    fn editing_the_document_makes_its_ids_stale_even_if_the_row_number_is_reused() {
        let (dir, mut index, mut embedder) = project();
        // Документ индексируется последним: после правки его чанк получает тот же номер строки.
        let stale = id_of(&index, &mut embedder, "zq_widget_limit предел");
        let other = id_of(&index, &mut embedder, "Кандидаты RRF слияние");
        assert!(ask(&index, &stale).is_ok());
        write(
            dir.path(),
            "docs/search/identifier.md",
            "# Идентификаторы

Совсем другой текст про файлы и папки проекта.
",
        );
        update_index(dir.path(), &mut index, &settings(), &mut embedder).unwrap();
        assert!(matches!(
            ask(&index, &stale),
            Err(SearchError::ChunkNotFound(_))
        ));
        // ID чанка из нетронутого документа остаётся рабочим.
        assert!(ask(&index, &other).is_ok());
        // Свежая выдача снова даёт рабочий ID; номер строки тот же, контроль другой.
        let fresh = id_of(&index, &mut embedder, "Совсем другой текст файлы папки");
        assert_eq!(
            fresh.split_once('-').unwrap().0,
            stale.split_once('-').unwrap().0
        );
        assert_ne!(fresh, stale);
        assert!(ask(&index, &fresh).is_ok());
    }
}
