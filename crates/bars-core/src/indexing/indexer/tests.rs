use std::time::{Duration, SystemTime};

use super::*;
use crate::models::ModelFingerprint;
use crate::storage::index_path;

const DIM: usize = 4;

/// Детерминированная подделка модели: вектор из хеша текста, счётчики вызовов.
struct FakeEmbedder {
    fingerprint: ModelFingerprint,
    calls: usize,
    vectors_computed: usize,
    fail_on_call: Option<usize>,
    returned_dim: usize,
}

impl FakeEmbedder {
    fn new(model: &str) -> Self {
        Self {
            fingerprint: ModelFingerprint {
                source: "builtin".into(),
                model: model.into(),
                dim: DIM,
                doc_prefix: String::new(),
            },
            calls: 0,
            vectors_computed: 0,
            fail_on_call: None,
            returned_dim: DIM,
        }
    }
}

impl Embedder for FakeEmbedder {
    fn fingerprint(&self) -> &ModelFingerprint {
        &self.fingerprint
    }

    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        self.calls += 1;
        if self.fail_on_call == Some(self.calls) {
            return Err(EmbedError::Embed("сбой подделки".into()));
        }
        self.vectors_computed += texts.len();
        Ok(texts
            .iter()
            .map(|text| {
                let hash = blake3::hash(text.as_bytes());
                (0..self.returned_dim)
                    .map(|i| f32::from(hash.as_bytes()[i % 32]) + 1.0)
                    .collect()
            })
            .collect())
    }
}

fn settings() -> IndexSettings {
    IndexSettings {
        chunking: ChunkingParams::default(),
        docs_root: "docs".into(),
    }
}

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// Раздел длиннее минимального размера чанка: не склеивается с соседом.
fn section(title: &str, word: &str) -> String {
    format!("## {title}\n\n{}\n\n", format!("{word} ").repeat(60))
}

fn doc(title: &str, words: &[&str]) -> String {
    let mut text = format!("# {title}\n\n");
    for (i, word) in words.iter().enumerate() {
        text.push_str(&section(&format!("Раздел {i}"), word));
    }
    text
}

fn set_mtime(root: &Path, relative: &str, seconds: u64) {
    let file = fs::File::options()
        .write(true)
        .open(root.join(relative))
        .unwrap();
    file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds))
        .unwrap();
}

fn open(root: &Path) -> Index {
    Index::open(&index_path(root), Journal::Wal).unwrap()
}

fn updated(root: &Path, index: &mut Index, embedder: &mut FakeEmbedder) -> UpdateReport {
    match update_index(root, index, &settings(), embedder).unwrap() {
        UpdateOutcome::Updated(report) => report,
        other => panic!("ждали обновление, получили {other:?}"),
    }
}

fn chunk_count_of(path: &str, text: &str) -> usize {
    chunk_document(path, text, &ChunkingParams::default())
        .chunks
        .len()
}

fn paths(index: &Index) -> Vec<String> {
    index
        .documents()
        .unwrap()
        .into_iter()
        .map(|document| document.path)
        .collect()
}

#[test]
fn first_run_embeds_every_chunk_and_records_layout() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (doc("A", &["альфа", "бета"]), doc("B", &["гамма"]));
    write(dir.path(), "docs/a.md", &a);
    write(dir.path(), "docs/mod/b.md", &b);
    let mut index = open(dir.path());
    let mut model = FakeEmbedder::new("m");

    let report = updated(dir.path(), &mut index, &mut model);

    let chunks = chunk_count_of("docs/a.md", &a) + chunk_count_of("docs/mod/b.md", &b);
    assert_eq!((report.added, report.changed, report.unchanged), (2, 0, 0));
    assert_eq!(report.chunks_embedded, chunks);
    assert_eq!(report.chunks_reused, 0);
    assert!(report.full_check);
    assert_eq!(index.document_count().unwrap(), 2);
    assert_eq!(index.chunk_count().unwrap(), chunks);
    assert_eq!(index.vector_count().unwrap(), chunks);
    assert_eq!(index.stored_layout().unwrap(), Some(settings().layout()));
    assert!(index.meta("updated_at").unwrap().is_some());
}

#[test]
fn second_run_touches_nothing() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/a.md", &doc("A", &["альфа"]));
    write(dir.path(), "docs/b.md", &doc("B", &["бета"]));
    let mut index = open(dir.path());
    let mut model = FakeEmbedder::new("m");
    updated(dir.path(), &mut index, &mut model);
    let calls = model.calls;

    let report = updated(dir.path(), &mut index, &mut model);

    assert_eq!((report.added, report.changed, report.unchanged), (0, 0, 2));
    assert!(!report.full_check);
    assert_eq!(model.calls, calls);
}

#[test]
fn editing_one_section_embeds_only_its_chunks() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "docs/a.md",
        &doc("A", &["альфа", "бета", "гамма"]),
    );
    set_mtime(dir.path(), "docs/a.md", 1_000_000_000);
    let mut index = open(dir.path());
    let mut model = FakeEmbedder::new("m");
    updated(dir.path(), &mut index, &mut model);
    let before = model.vectors_computed;

    let edited = doc("A", &["альфа", "дельта", "гамма"]);
    write(dir.path(), "docs/a.md", &edited);
    set_mtime(dir.path(), "docs/a.md", 1_000_000_100);
    let report = updated(dir.path(), &mut index, &mut model);

    assert_eq!((report.added, report.changed, report.unchanged), (0, 1, 0));
    assert_eq!(report.chunks_embedded, 1);
    assert_eq!(model.vectors_computed - before, 1);
    assert_eq!(
        report.chunks_reused,
        chunk_count_of("docs/a.md", &edited) - 1
    );
    // Вектор старого раздела убран как осиротевший.
    assert_eq!(index.vector_count().unwrap(), index.chunk_count().unwrap());
}

#[test]
fn same_size_edit_is_found_through_mtime_and_hash() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/a.md", &doc("A", &["альфа"]));
    set_mtime(dir.path(), "docs/a.md", 1_000_000_000);
    let mut index = open(dir.path());
    let mut model = FakeEmbedder::new("m");
    updated(dir.path(), &mut index, &mut model);

    write(dir.path(), "docs/a.md", &doc("A", &["бетта"])); // та же длина
    set_mtime(dir.path(), "docs/a.md", 1_000_000_100);
    let report = updated(dir.path(), &mut index, &mut model);

    assert_eq!(report.changed, 1);
    assert!(report.chunks_embedded >= 1);
}

#[test]
fn rewritten_identical_content_is_not_embedded_again() {
    let dir = tempfile::tempdir().unwrap();
    let text = doc("A", &["альфа"]);
    write(dir.path(), "docs/a.md", &text);
    set_mtime(dir.path(), "docs/a.md", 1_000_000_000);
    let mut index = open(dir.path());
    let mut model = FakeEmbedder::new("m");
    updated(dir.path(), &mut index, &mut model);
    let calls = model.calls;

    write(dir.path(), "docs/a.md", &text);
    set_mtime(dir.path(), "docs/a.md", 1_000_000_100);
    let report = updated(dir.path(), &mut index, &mut model);

    assert_eq!((report.changed, report.unchanged), (0, 1));
    assert_eq!(model.calls, calls);
    // Время изменения запомнено: следующая проверка не читает файл.
    assert_eq!(
        index.documents().unwrap()[0].modified_ns,
        Some(1_000_000_100 * 1_000_000_000)
    );
}

#[test]
fn repeated_section_is_embedded_once() {
    let dir = tempfile::tempdir().unwrap();
    // Первый раздел другой: вступление склеивается с ним, и повторы остаются чистыми копиями.
    let repeated = format!(
        "# A

{}{}{}",
        section("Первый", "альфа"),
        section("Повтор", "эхоэхоэхо"),
        section("Повтор", "эхоэхоэхо")
    );
    write(dir.path(), "docs/a.md", &repeated);
    let mut index = open(dir.path());
    let mut model = FakeEmbedder::new("m");

    let report = updated(dir.path(), &mut index, &mut model);

    let chunks = chunk_count_of("docs/a.md", &repeated);
    assert_eq!(chunks, 3);
    assert_eq!(index.chunk_count().unwrap(), 3);
    assert_eq!(index.vector_count().unwrap(), 2);
    assert_eq!(report.chunks_embedded, 2);
    assert_eq!(model.vectors_computed, 2);
    assert_eq!(report.chunks_reused, 1);
}

#[test]
fn deleted_file_leaves_the_index_with_its_vectors_cleaned() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/keep.md", &doc("K", &["альфа"]));
    write(dir.path(), "docs/gone/g.md", &doc("G", &["бета"]));
    let mut index = open(dir.path());
    let mut model = FakeEmbedder::new("m");
    updated(dir.path(), &mut index, &mut model);

    fs::remove_dir_all(dir.path().join("docs/gone")).unwrap();
    let report = updated(dir.path(), &mut index, &mut model);

    assert_eq!((report.removed, report.unchanged), (1, 1));
    assert_eq!(index.document_count().unwrap(), 1);
    let modules: Vec<String> = index
        .conn()
        .prepare("SELECT DISTINCT module FROM documents")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(modules, vec!["."]);
    assert_eq!(index.vector_count().unwrap(), index.chunk_count().unwrap());
}

#[test]
fn changed_model_asks_for_reindex_without_calling_the_model() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/a.md", &doc("A", &["альфа"]));
    let mut index = open(dir.path());
    updated(dir.path(), &mut index, &mut FakeEmbedder::new("old"));
    let mut other = FakeEmbedder::new("new");

    let outcome = update_index(dir.path(), &mut index, &settings(), &mut other).unwrap();

    assert_eq!(
        outcome,
        UpdateOutcome::ReindexRequired(ReindexReason::ModelChanged)
    );
    assert_eq!(other.calls, 0);
    assert_eq!(index.document_count().unwrap(), 1);
}

#[test]
fn outdated_schema_asks_for_reindex() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/a.md", &doc("A", &["альфа"]));
    let mut index = open(dir.path());
    let mut model = FakeEmbedder::new("m");
    updated(dir.path(), &mut index, &mut model);
    index.set_meta("schema_version", "0").unwrap();

    let outcome = update_index(dir.path(), &mut index, &settings(), &mut model).unwrap();

    assert_eq!(
        outcome,
        UpdateOutcome::ReindexRequired(ReindexReason::SchemaOutdated)
    );
}

#[test]
fn changed_chunking_rechunks_everything_but_reuses_vectors() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/a.md", &doc("A", &["альфа", "бета"]));
    let mut index = open(dir.path());
    let mut model = FakeEmbedder::new("m");
    updated(dir.path(), &mut index, &mut model);
    let calls = model.calls;

    let mut changed = settings();
    changed.chunking.min_chars = 100; // разделы длиннее обоих порогов: чанки те же
    let report = match update_index(dir.path(), &mut index, &changed, &mut model).unwrap() {
        UpdateOutcome::Updated(report) => report,
        other => panic!("{other:?}"),
    };

    assert!(report.full_check);
    assert_eq!((report.changed, report.unchanged), (1, 0));
    assert_eq!(report.chunks_embedded, 0);
    assert_eq!(report.chunks_reused, index.chunk_count().unwrap());
    assert_eq!(model.calls, calls);
    assert_eq!(index.stored_layout().unwrap(), Some(changed.layout()));

    let again = match update_index(dir.path(), &mut index, &changed, &mut model).unwrap() {
        UpdateOutcome::Updated(report) => report,
        other => panic!("{other:?}"),
    };
    assert!(!again.full_check);
    assert_eq!(again.unchanged, 1);
}

#[test]
fn invalid_utf8_is_reported_and_dropped_from_the_index() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/good.md", &doc("G", &["альфа"]));
    write(dir.path(), "docs/turns_bad.md", &doc("T", &["бета"]));
    let mut index = open(dir.path());
    let mut model = FakeEmbedder::new("m");
    updated(dir.path(), &mut index, &mut model);

    fs::write(dir.path().join("docs/turns_bad.md"), [0xff, 0xfe, 0x41]).unwrap();
    set_mtime(dir.path(), "docs/turns_bad.md", 1_000_000_100);
    fs::write(dir.path().join("docs/new_bad.md"), [0xff, 0xff]).unwrap();
    let report = updated(dir.path(), &mut index, &mut model);

    assert_eq!(report.removed, 1);
    let mut problems: Vec<&str> = report.problems.iter().map(|p| p.path.as_str()).collect();
    problems.sort_unstable();
    assert_eq!(problems, ["docs/new_bad.md", "docs/turns_bad.md"]);
    assert_eq!(paths(&index), ["docs/good.md"]);
}

#[test]
fn embedding_failure_keeps_written_documents_and_resumes_later() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (doc("A", &["альфа"]), doc("B", &["бета"]));
    write(dir.path(), "docs/a.md", &a);
    write(dir.path(), "docs/b.md", &b);
    let mut index = open(dir.path());
    let mut model = FakeEmbedder::new("m");
    model.fail_on_call = Some(2); // a.md записан, на b.md модель падает

    let error = update_index(dir.path(), &mut index, &settings(), &mut model).unwrap_err();

    assert!(matches!(error, IndexingError::Embed(EmbedError::Embed(_))));
    assert_eq!(paths(&index), ["docs/a.md"]);
    assert_eq!(index.stored_layout().unwrap(), None);

    model.fail_on_call = None;
    let report = updated(dir.path(), &mut index, &mut model);

    assert!(report.full_check);
    assert_eq!((report.added, report.changed), (1, 1));
    assert_eq!(report.chunks_embedded, chunk_count_of("docs/b.md", &b));
    assert_eq!(report.chunks_reused, chunk_count_of("docs/a.md", &a));
}

#[test]
fn wrong_vector_dimension_is_an_error_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/a.md", &doc("A", &["альфа"]));
    let mut index = open(dir.path());
    let mut model = FakeEmbedder::new("m");
    model.returned_dim = DIM + 1;

    let error = update_index(dir.path(), &mut index, &settings(), &mut model).unwrap_err();

    assert!(matches!(
        error,
        IndexingError::Embed(EmbedError::Dimension { expected: DIM, .. })
    ));
    assert_eq!(index.document_count().unwrap(), 0);
    assert_eq!(index.vector_count().unwrap(), 0);
}

#[test]
fn missing_docs_root_is_an_error_and_keeps_the_index() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/a.md", &doc("A", &["альфа"]));
    let mut index = open(dir.path());
    let mut model = FakeEmbedder::new("m");
    updated(dir.path(), &mut index, &mut model);

    fs::remove_dir_all(dir.path().join("docs")).unwrap();
    let error = update_index(dir.path(), &mut index, &settings(), &mut model).unwrap_err();

    assert!(matches!(
        error,
        IndexingError::Walk(WalkError::DocsRootUnavailable(..))
    ));
    assert_eq!(index.document_count().unwrap(), 1);
}

#[test]
fn problem_path_covers_itself_and_its_subtree_only() {
    let problems = [WalkProblem {
        path: "docs/mod".into(),
        reason: "нет доступа".into(),
    }];
    assert!(is_covered_by_problem("docs/mod", &problems));
    assert!(is_covered_by_problem("docs/mod/a/b.md", &problems));
    assert!(!is_covered_by_problem("docs/module/a.md", &problems));
    assert!(!is_covered_by_problem("docs/other.md", &problems));
}

fn built_project(dir: &Path) -> PathBuf {
    write(dir, "docs/a.md", &doc("A", &["альфа", "бета"]));
    write(dir, "docs/m/b.md", &doc("B", &["гамма"]));
    let mut index = open(dir);
    updated(dir, &mut index, &mut FakeEmbedder::new("old"));
    index.close().unwrap();
    index_path(dir)
}

#[test]
fn rebuild_replaces_the_index_and_embeds_everything_again() {
    let dir = tempfile::tempdir().unwrap();
    let path = built_project(dir.path());
    // Остаток журнала от упавшего процесса не должен попасть в новый индекс.
    fs::write(with_suffix(&path, "-wal"), b"garbage").unwrap();
    let mut model = FakeEmbedder::new("new");

    let report = rebuild_index(dir.path(), &path, &settings(), &mut model).unwrap();

    assert_eq!(report.added, 2);
    assert_eq!(report.chunks_reused, 0);
    assert_eq!(model.vectors_computed, report.chunks_embedded);
    assert!(!with_suffix(&path, ".new").exists());
    assert!(!with_suffix(&path, ".old").exists());
    let mut index = Index::open(&path, Journal::Wal).unwrap();
    assert_eq!(index.document_count().unwrap(), 2);
    assert_eq!(index.stored_fingerprint().unwrap().unwrap().model, "new");
    // Новый индекс совместим с новой моделью и не требует полной проверки.
    let again = updated(dir.path(), &mut index, &mut model);
    assert_eq!(again.unchanged, 2);
    assert!(!again.full_check);
}

#[test]
fn failed_rebuild_keeps_the_old_index_intact() {
    let dir = tempfile::tempdir().unwrap();
    let path = built_project(dir.path());
    let mut model = FakeEmbedder::new("new");
    model.fail_on_call = Some(2);

    let error = rebuild_index(dir.path(), &path, &settings(), &mut model).unwrap_err();

    assert!(matches!(error, IndexingError::Embed(_)));
    assert!(!with_suffix(&path, ".new").exists());
    let index = Index::open(&path, Journal::Wal).unwrap();
    assert_eq!(index.document_count().unwrap(), 2);
    assert_eq!(index.stored_fingerprint().unwrap().unwrap().model, "old");
}

#[test]
fn rebuild_without_an_existing_index_just_builds_one() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/a.md", &doc("A", &["альфа"]));
    let path = index_path(dir.path());

    rebuild_index(dir.path(), &path, &settings(), &mut FakeEmbedder::new("m")).unwrap();

    let index = Index::open(&path, Journal::Wal).unwrap();
    assert_eq!(index.document_count().unwrap(), 1);
}

/// Наблюдатель, который просит остановиться после `limit` документов и записывает ход.
struct StopAfter {
    limit: usize,
    seen: std::cell::RefCell<Vec<String>>,
    total: std::cell::Cell<usize>,
    done: std::cell::Cell<usize>,
}

impl StopAfter {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            seen: Default::default(),
            total: Default::default(),
            done: Default::default(),
        }
    }
}

impl IndexObserver for StopAfter {
    fn begin(&self, documents: usize) {
        self.total.set(documents);
    }

    fn file(&self, path: &str) {
        self.seen.borrow_mut().push(path.to_string());
    }

    fn progress(&self, documents_done: usize, _chunks_done: usize) {
        self.done.set(documents_done);
    }

    fn should_stop(&self) -> bool {
        self.done.get() >= self.limit
    }
}

#[test]
fn observer_sees_every_document_and_can_stop_the_update() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/a.md", &doc("A", &["альфа"]));
    write(dir.path(), "docs/b.md", &doc("B", &["бета"]));
    write(dir.path(), "docs/c.md", &doc("C", &["гамма"]));
    let mut index = Index::open(&index_path(dir.path()), Journal::Wal).unwrap();

    let observer = StopAfter::new(2);
    let error = update_index_observed(
        dir.path(),
        &mut index,
        &settings(),
        &mut FakeEmbedder::new("m"),
        &observer,
    )
    .unwrap_err();

    assert!(matches!(error, IndexingError::Cancelled));
    assert_eq!(observer.total.get(), 3);
    assert_eq!(observer.seen.borrow().len(), 2);
    // Уже записанные документы остаются: остановка не откатывает проверку.
    assert_eq!(index.document_count().unwrap(), 2);
}

#[test]
fn cancelled_rebuild_keeps_the_old_index_and_leaves_no_new_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = built_project(dir.path());

    let error = rebuild_index_observed(
        dir.path(),
        &path,
        &settings(),
        &mut FakeEmbedder::new("new"),
        &StopAfter::new(1),
        || (),
    )
    .unwrap_err();

    assert!(matches!(error, IndexingError::Cancelled));
    assert!(!with_suffix(&path, ".new").exists());
    let index = Index::open(&path, Journal::Wal).unwrap();
    assert_eq!(index.document_count().unwrap(), 2);
    assert_eq!(index.stored_fingerprint().unwrap().unwrap().model, "old");
}
