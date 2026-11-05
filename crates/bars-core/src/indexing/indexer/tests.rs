use std::time::{Duration, SystemTime};

use super::*;
use crate::models::ModelFingerprint;
use crate::storage::index_path;

const DIM: usize = 4;

#[test]
fn scoped_update_embeds_only_selected_changes_and_preserves_other_pending_changes() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a", "b", "c"] {
        write(
            dir.path(),
            &format!("docs/{name}.md"),
            &format!("# Исходный {name}\n"),
        );
    }
    let mut index = open(dir.path());
    let mut embedder = FakeEmbedder::new("m");
    updated(dir.path(), &mut index, &mut embedder);
    let before = embedder.vectors_computed;
    write(dir.path(), "docs/a.md", "# Изменённый первый документ\n");
    write(dir.path(), "docs/b.md", "# Изменённый второй документ\n");
    std::fs::remove_file(dir.path().join("docs/c.md")).unwrap();
    let only_a = HashSet::from(["docs/a.md".to_owned()]);
    let UpdateOutcome::Updated(report) = update_index_paths_observed(
        dir.path(),
        &mut index,
        &settings(),
        &mut embedder,
        &NoObserver,
        Some(&only_a),
    )
    .unwrap() else {
        panic!("ожидалось обновление")
    };
    assert_eq!(
        (
            report.added,
            report.changed,
            report.removed,
            report.unchanged
        ),
        (0, 1, 0, 0)
    );
    assert_eq!(embedder.vectors_computed, before + 1);
    assert_eq!(
        index.document_count().unwrap(),
        3,
        "невыбранное удаление ждёт своей очереди"
    );
    let UpdateOutcome::Updated(report) = update_index_paths_observed(
        dir.path(),
        &mut index,
        &settings(),
        &mut embedder,
        &NoObserver,
        Some(&only_a),
    )
    .unwrap() else {
        panic!("ожидалось обновление")
    };
    assert_eq!(report.unchanged, 1);
    assert_eq!(
        embedder.vectors_computed,
        before + 1,
        "повтор не пересчитывает вектор"
    );
    let report = updated(dir.path(), &mut index, &mut embedder);
    assert_eq!((report.changed, report.removed), (1, 1));
    assert_eq!(embedder.vectors_computed, before + 2);
}

#[test]
fn scoped_directory_update_includes_children_and_ignores_other_directories() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/group/a.md", "# А\n");
    write(dir.path(), "docs/other/b.md", "# Б\n");
    let mut index = open(dir.path());
    let mut embedder = FakeEmbedder::new("m");
    updated(dir.path(), &mut index, &mut embedder);
    write(dir.path(), "docs/group/new.md", "# Новый внутри группы\n");
    write(dir.path(), "docs/other/new.md", "# Новый в другой группе\n");
    let selected = HashSet::from(["docs/group".to_owned()]);
    let UpdateOutcome::Updated(report) = update_index_paths_observed(
        dir.path(),
        &mut index,
        &settings(),
        &mut embedder,
        &NoObserver,
        Some(&selected),
    )
    .unwrap() else {
        panic!("ожидалось обновление")
    };
    assert_eq!((report.added, report.unchanged), (1, 1));
    assert_eq!(index.document_count().unwrap(), 3);
    assert!(paths(&index).contains(&"docs/group/new.md".to_owned()));
    assert!(!paths(&index).contains(&"docs/other/new.md".to_owned()));
}

#[test]
fn scan_discovery_excludes_metadata_only_changes_and_includes_confirmed_deletions() {
    #[derive(Default)]
    struct Found(std::cell::RefCell<Vec<String>>);
    impl IndexObserver for Found {
        fn discovered(&self, paths: &[String]) {
            self.0.replace(paths.to_vec());
        }
    }
    let dir = tempfile::tempdir().unwrap();
    for name in ["a", "b", "c"] {
        write(dir.path(), &format!("docs/{name}.md"), "# Документ\n");
    }
    let mut index = open(dir.path());
    updated(dir.path(), &mut index, &mut FakeEmbedder::new("m"));
    let snapshot = ScanSnapshot::read(Some(&index)).unwrap();
    write(dir.path(), "docs/a.md", "# Документ\n");
    write(dir.path(), "docs/b.md", "# Изменённое содержимое\n");
    write(dir.path(), "docs/d.md", "# Новый\n");
    std::fs::remove_file(dir.path().join("docs/c.md")).unwrap();
    let observer = Found::default();
    let report = scan_index(dir.path(), &snapshot, &settings(), &observer).unwrap();
    assert_eq!(
        (
            report.added,
            report.changed,
            report.removed,
            report.unchanged
        ),
        (1, 1, 1, 1)
    );
    let mut paths = observer.0.into_inner();
    paths.sort();
    assert_eq!(paths, ["docs/b.md", "docs/c.md", "docs/d.md"]);
    assert_eq!(
        index.document_count().unwrap(),
        3,
        "scan не записывает индекс"
    );
}

#[test]
fn discovery_precedes_processing_in_priority_order_and_reports_deletions() {
    #[derive(Default)]
    struct Events {
        discovered: std::cell::RefCell<Vec<String>>,
        files: std::cell::RefCell<Vec<String>>,
        done: std::cell::RefCell<Vec<String>>,
    }
    impl IndexObserver for Events {
        fn discovered(&self, paths: &[String]) {
            self.discovered.replace(paths.to_vec());
        }
        fn priority(&self, path: &str) -> u8 {
            u8::from(path.ends_with("z.md"))
        }
        fn file(&self, path: &str) {
            assert!(self.discovered.borrow().contains(&path.to_string()));
            self.files.borrow_mut().push(path.to_string());
        }
        fn document_done(&self, path: &str, error: Option<&str>) {
            assert!(error.is_none());
            self.done.borrow_mut().push(path.to_string());
        }
    }
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/old.md", "# Удаляемый\n");
    let mut index = open(dir.path());
    let mut embedder = FakeEmbedder::new("m");
    updated(dir.path(), &mut index, &mut embedder);
    std::fs::remove_file(dir.path().join("docs/old.md")).unwrap();
    write(dir.path(), "docs/a.md", "# Первый\n");
    write(dir.path(), "docs/z.md", "# Приоритетный\n");
    let observer = Events::default();
    update_index_observed(
        dir.path(),
        &mut index,
        &settings(),
        &mut embedder,
        &observer,
    )
    .unwrap();
    let mut found = observer.discovered.into_inner();
    found.sort();
    assert_eq!(found, ["docs/a.md", "docs/old.md", "docs/z.md"]);
    assert_eq!(
        observer.files.into_inner(),
        ["docs/z.md", "docs/a.md", "docs/old.md"]
    );
    assert_eq!(
        observer.done.into_inner(),
        ["docs/z.md", "docs/a.md", "docs/old.md"]
    );
}

#[test]
fn a_scan_snapshot_survives_closing_and_replacing_the_index_file() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/a.md", "# Документ\n");
    let mut index = open(dir.path());
    updated(dir.path(), &mut index, &mut FakeEmbedder::new("m"));
    let snapshot = ScanSnapshot::read(Some(&index)).unwrap();
    index.close().unwrap();
    let path = index_path(dir.path());
    std::fs::rename(&path, path.with_extension("snapshot")).unwrap();
    write(dir.path(), "docs/b.md", "# Новый\n");
    let report = scan_index(dir.path(), &snapshot, &settings(), &FastObserver::default()).unwrap();
    assert_eq!((report.added, report.unchanged), (1, 1));
    assert!(
        !path.exists(),
        "проверка диска не должна заново открывать или создавать индекс"
    );
}

#[test]
fn scan_reports_differences_without_writing_or_creating_an_index() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/a.md", "# Старый\n\nТекст\n");
    write(dir.path(), "docs/b.md", "# Удаляемый\n");
    let mut index = open(dir.path());
    updated(dir.path(), &mut index, &mut FakeEmbedder::new("m"));
    let before = index.documents().unwrap();
    write(
        dir.path(),
        "docs/a.md",
        "# Новый\n\nСодержимое изменено и стало длиннее\n",
    );
    std::fs::remove_file(dir.path().join("docs/b.md")).unwrap();
    write(dir.path(), "docs/c.md", "# Добавленный\n");
    let reader = Index::open_read_only(&index_path(dir.path())).unwrap();
    let report = scan_index(
        dir.path(),
        &ScanSnapshot::read(Some(&reader)).unwrap(),
        &settings(),
        &FastObserver::default(),
    )
    .unwrap();
    assert_eq!((report.added, report.changed, report.removed), (1, 1, 1));
    assert_eq!(report.chunks_embedded, 0);
    assert_eq!(index.documents().unwrap(), before);
    let absent = tempfile::tempdir().unwrap();
    write(absent.path(), "docs/a.md", "# Документ\n");
    let report = scan_index(
        absent.path(),
        &ScanSnapshot::read(None).unwrap(),
        &settings(),
        &FastObserver::default(),
    )
    .unwrap();
    assert_eq!(report.added, 1);
    assert!(!index_path(absent.path()).exists());
}

#[test]
fn scan_reports_unreadable_text_without_deleting_the_published_document() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/a.md", "# Документ\n");
    let mut index = open(dir.path());
    updated(dir.path(), &mut index, &mut FakeEmbedder::new("m"));
    std::fs::write(dir.path().join("docs/a.md"), [0xff, 0xff]).unwrap();
    let reader = Index::open_read_only(&index_path(dir.path())).unwrap();
    let report = scan_index(
        dir.path(),
        &ScanSnapshot::read(Some(&reader)).unwrap(),
        &settings(),
        &FastObserver::default(),
    )
    .unwrap();
    assert_eq!(report.problems.len(), 1);
    assert_eq!(report.removed, 0);
    assert_eq!(index.document_count().unwrap(), 1);
}

fn cause(error: &IndexingError) -> &IndexingError {
    match error {
        IndexingError::Partial { source, .. } => cause(source),
        other => other,
    }
}

#[derive(Default)]
struct FastObserver {
    reports: std::cell::RefCell<Vec<UpdateReport>>,
    delays: std::cell::RefCell<Vec<u64>>,
    cancel_wait: bool,
}
impl IndexObserver for FastObserver {
    fn wait_retry(&self, delay: Duration) -> bool {
        self.delays.borrow_mut().push(delay.as_secs());
        self.cancel_wait
    }
    fn report(&self, report: &UpdateReport) {
        self.reports.borrow_mut().push(report.clone());
    }
}

struct Failing {
    model: FakeEmbedder,
    calls: usize,
    transient_left: usize,
    bad_paths: bool,
    rejected: bool,
}
impl Failing {
    fn new() -> Self {
        Self {
            model: FakeEmbedder::new("m"),
            calls: 0,
            transient_left: 0,
            bad_paths: false,
            rejected: false,
        }
    }
}
impl Embedder for Failing {
    fn fingerprint(&self) -> &ModelFingerprint {
        self.model.fingerprint()
    }
    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        self.calls += 1;
        if self.transient_left > 0
            || (self.bad_paths && texts.iter().any(|text| text.contains("bad")))
        {
            self.transient_left = self.transient_left.saturating_sub(1);
            return Err(if self.rejected {
                EmbedError::Rejected("413".into())
            } else {
                EmbedError::Embed("temporary".into())
            });
        }
        self.model.embed(texts)
    }
}

#[test]
fn stage17_retries_once_and_three_times_with_one_budget() {
    for failures in [1, 3] {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "docs/a.md", &doc("A", &["alpha"]));
        let mut index = open(dir.path());
        let mut model = Failing::new();
        model.transient_left = failures;
        let observer = FastObserver::default();
        update_index_observed(dir.path(), &mut index, &settings(), &mut model, &observer).unwrap();
        assert_eq!(model.calls, failures + 1);
        assert_eq!(
            *observer.delays.borrow(),
            if failures == 1 {
                vec![1]
            } else {
                vec![1, 2, 4]
            }
        );
        assert_eq!(index.document_count().unwrap(), 1);
    }
}

#[test]
fn stage17_skips_bad_document_preserves_version_and_retries_next_check() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/bad.md", &doc("Old", &["before"]));
    let mut index = open(dir.path());
    let mut model = Failing::new();
    update_index(dir.path(), &mut index, &settings(), &mut model).unwrap();
    let before = index.documents().unwrap()[0].clone();
    write(dir.path(), "docs/bad.md", &doc("Changed", &["different"]));
    write(dir.path(), "docs/z.md", &doc("Good", &["after"]));
    model.bad_paths = true;
    let calls = model.calls;
    let outcome = update_index_observed(
        dir.path(),
        &mut index,
        &settings(),
        &mut model,
        &FastObserver::default(),
    )
    .unwrap();
    let UpdateOutcome::Updated(report) = outcome else {
        panic!()
    };
    assert_eq!(model.calls - calls, 5);
    assert_eq!(report.problems[0].attempts, 4);
    assert!(report.problems[0].previous_version);
    assert_eq!(
        index
            .documents()
            .unwrap()
            .iter()
            .find(|d| d.path == "docs/bad.md")
            .unwrap()
            .hash,
        before.hash
    );
    model.bad_paths = false;
    let UpdateOutcome::Updated(report) =
        update_index(dir.path(), &mut index, &settings(), &mut model).unwrap()
    else {
        panic!()
    };
    assert_eq!(report.changed, 1);
    assert!(report.problems.is_empty());
}

#[test]
fn stage17_series_stops_with_report_and_cancellation_keeps_report() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["bad1", "bad2", "bad3", "z"] {
        write(
            dir.path(),
            &format!("docs/{name}.md"),
            &doc(name, &["text"]),
        );
    }
    let mut index = open(dir.path());
    let mut model = Failing::new();
    model.bad_paths = true;
    let observer = FastObserver::default();
    let error = update_index_observed(dir.path(), &mut index, &settings(), &mut model, &observer)
        .unwrap_err();
    let IndexingError::Partial { report, source } = error else {
        panic!()
    };
    assert!(matches!(*source, IndexingError::ConsecutiveFailures));
    assert!(report.aborted);
    assert_eq!(report.problems.len(), 3);
    assert_eq!(model.calls, 12);
    assert_eq!(index.document_count().unwrap(), 0);
    let observer = FastObserver {
        cancel_wait: true,
        ..Default::default()
    };
    let error = update_index_observed(dir.path(), &mut index, &settings(), &mut model, &observer)
        .unwrap_err();
    assert!(matches!(cause(&error), IndexingError::Cancelled));
}

#[test]
fn stage17_rejected_document_is_not_retried_and_layout_is_not_advanced() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/bad.md", &doc("Bad", &["text"]));
    let mut index = open(dir.path());
    let mut model = Failing::new();
    model.bad_paths = true;
    model.rejected = true;
    let UpdateOutcome::Updated(report) = update_index_observed(
        dir.path(),
        &mut index,
        &settings(),
        &mut model,
        &FastObserver::default(),
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(report.problems[0].attempts, 1);
    assert_eq!(model.calls, 1);
    assert_eq!(index.stored_layout().unwrap(), None);
}

#[test]
fn stage17_incomplete_rebuild_never_swaps_even_with_last_document_failure() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/a.md", &doc("Original", &["old"]));
    let path = index_path(dir.path());
    let mut initial = FakeEmbedder::new("old");
    rebuild_index(dir.path(), &path, &settings(), &mut initial).unwrap();
    let original = fs::read(&path).unwrap();
    write(dir.path(), "docs/zbad.md", &doc("Bad", &["new"]));
    let mut model = Failing::new();
    model.bad_paths = true;
    let error = rebuild_index_observed(
        dir.path(),
        &path,
        &settings(),
        &mut model,
        &FastObserver::default(),
        || panic!("swap must not run"),
    )
    .unwrap_err();
    let IndexingError::Incomplete(report) = error else {
        panic!()
    };
    assert!(report.rebuild_not_applied);
    assert_eq!(report.added, 1);
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(!with_suffix(&path, ".new").exists());
}

#[test]
fn stage17_success_resets_series_but_unchanged_files_do_not() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a_bad", "b_good", "c_bad", "d_bad", "z"] {
        write(
            dir.path(),
            &format!("docs/{name}.md"),
            &doc(name, &["text"]),
        );
    }
    let mut model = Failing::new();
    model.bad_paths = true;
    let mut index = open(dir.path());
    let UpdateOutcome::Updated(report) = update_index_observed(
        dir.path(),
        &mut index,
        &settings(),
        &mut model,
        &FastObserver::default(),
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(report.problems.len(), 3);
    assert_eq!(report.added, 2);
    // b_good is now unchanged; it no longer demonstrates provider recovery.
    let error = update_index_observed(
        dir.path(),
        &mut index,
        &settings(),
        &mut model,
        &FastObserver::default(),
    )
    .unwrap_err();
    assert!(matches!(cause(&error), IndexingError::ConsecutiveFailures));
}

#[test]
fn stage17_partial_layout_change_does_not_mark_failed_file_as_current() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/bad.md", &doc("Old", &["one", "two"]));
    let mut index = open(dir.path());
    let mut model = Failing::new();
    update_index(dir.path(), &mut index, &settings(), &mut model).unwrap();
    let before = index.stored_layout().unwrap();
    let mut changed = settings();
    changed.chunking.max_chars = 300;
    model.bad_paths = true;
    let UpdateOutcome::Updated(report) = update_index_observed(
        dir.path(),
        &mut index,
        &changed,
        &mut model,
        &FastObserver::default(),
    )
    .unwrap() else {
        panic!()
    };
    assert!(!report.problems.is_empty());
    assert_eq!(index.stored_layout().unwrap(), before);
}

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
fn changed_registry_algorithm_rechunks_unchanged_files_and_reuses_vectors() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/a.md", &doc("A", &["альфа", "бета"]));
    let mut index = open(dir.path());
    let mut model = FakeEmbedder::new("m");
    updated(dir.path(), &mut index, &mut model);
    let calls = model.calls;
    index
        .conn()
        .execute("DELETE FROM meta WHERE key='chunking_version'", [])
        .unwrap();

    let snapshot = ScanSnapshot::read(Some(&index)).unwrap();
    let scan = scan_index(dir.path(), &snapshot, &settings(), &NoObserver).unwrap();
    assert!(scan.full_check);
    let report = updated(dir.path(), &mut index, &mut model);
    assert!(report.full_check);
    assert_eq!((report.changed, report.unchanged), (1, 0));
    assert_eq!(report.chunks_embedded, 0);
    assert_eq!(report.chunks_reused, index.chunk_count().unwrap());
    assert_eq!(model.calls, calls);
    assert_eq!(index.stored_layout().unwrap(), Some(settings().layout()));
    assert!(!updated(dir.path(), &mut index, &mut model).full_check);
}

#[test]
fn registry_migration_replaces_legacy_table_during_scoped_update() {
    use crate::indexing::chunking::Chunk;
    let dir = tempfile::tempdir().unwrap();
    let mut body = String::from("| ID | Описание |\n|---|---|\n");
    for i in 0..25 {
        body.push_str(&format!("| ISS-{i} | {} |\n", "строка ".repeat(23)));
    }
    body.pop();
    let source = format!("# Реестр\n{body}");
    write(dir.path(), "docs/registry.md", &source);
    write(dir.path(), "docs/a.md", &doc("A", &["альфа", "бета"]));
    let mut index = open(dir.path());
    let mut model = FakeEmbedder::new("m");
    updated(dir.path(), &mut index, &mut model);
    let stored = index
        .documents()
        .unwrap()
        .into_iter()
        .find(|d| d.path == "docs/registry.md")
        .unwrap();
    let embed_text = format!("path: docs/registry.md\nРеестр\n{body}");
    let hash = blake3::hash(embed_text.as_bytes()).to_hex().to_string();
    let vector = model.embed(&[&embed_text]).unwrap().remove(0);
    let chunks = [Chunk {
        ordinal: 0,
        heading_chain: "Реестр".into(),
        first_line: 1,
        last_line: source.lines().count(),
        text: body,
        embed_text,
        embed_hash: hash.clone(),
    }];
    index
        .replace_document(
            &DocumentRecord {
                path: &stored.path,
                module: ".",
                title: "registry",
                size: stored.size,
                modified_ns: stored.modified_ns,
                hash: &stored.hash,
                chunks: &chunks,
                text: None,
            },
            &[(&hash, &vector)],
        )
        .unwrap();
    index.remove_orphan_vectors().unwrap();
    index.set_meta("chunking_version", "older").unwrap();
    let expected = chunk_document("docs/registry.md", &source, &settings().chunking);
    assert!(expected.chunks.len() > 1);
    // Список путей не должен скрыть старый реестр, когда меняется алгоритм всего индекса.
    let selected = HashSet::from(["docs/a.md".to_string()]);
    let UpdateOutcome::Updated(report) = update_index_paths_observed(
        dir.path(),
        &mut index,
        &settings(),
        &mut model,
        &NoObserver,
        Some(&selected),
    )
    .unwrap() else {
        panic!()
    };
    assert!(report.full_check);
    assert_eq!(report.changed, 2);
    assert_eq!(report.chunks_embedded, expected.chunks.len());
    assert!(report.chunks_reused > 0);
    let mut stmt = index.conn().prepare("SELECT c.embed_hash FROM chunks c JOIN documents d ON d.id=c.document_id WHERE d.path='docs/registry.md' ORDER BY c.ordinal").unwrap();
    let actual: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        actual,
        expected
            .chunks
            .into_iter()
            .map(|c| c.embed_hash)
            .collect::<Vec<_>>()
    );
}

#[cfg(windows)]
#[test]
fn registry_migration_waits_for_old_unreadable_document() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/a.md", &doc("A", &["альфа", "бета"]));
    let mut index = open(dir.path());
    let mut model = FakeEmbedder::new("m");
    updated(dir.path(), &mut index, &mut model);
    index.set_meta("chunking_version", "older").unwrap();
    let locked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(dir.path().join("docs/a.md"))
        .unwrap();
    let report = updated(dir.path(), &mut index, &mut model);
    assert!(report.full_check);
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.path == "docs/a.md" && p.previous_version)
    );
    assert_eq!(index.stored_layout().unwrap(), None);
    drop(locked);
    assert!(updated(dir.path(), &mut index, &mut model).full_check);
    assert_eq!(index.stored_layout().unwrap(), Some(settings().layout()));
    assert!(!updated(dir.path(), &mut index, &mut model).full_check);
}

#[test]
fn registry_migration_waits_for_old_unvisited_subdirectory() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "docs/hidden/a.md",
        &doc("A", &["альфа", "бета"]),
    );
    let mut index = open(dir.path());
    let mut model = FakeEmbedder::new("m");
    updated(dir.path(), &mut index, &mut model);
    index.set_meta("chunking_version", "older").unwrap();
    let stored = index
        .documents()
        .unwrap()
        .into_iter()
        .map(|d| (d.path.clone(), d))
        .collect();
    let walk = Walk {
        problems: vec![WalkProblem {
            path: "docs/hidden".into(),
            reason: "нет доступа".into(),
        }],
        ..Default::default()
    };
    let mut report = UpdateReport {
        full_check: true,
        ..Default::default()
    };
    finish_update(
        &mut index,
        &walk,
        &stored,
        &settings().layout(),
        &mut report,
        &NoObserver,
    )
    .unwrap();
    assert_eq!(index.stored_layout().unwrap(), None);
    assert_eq!(index.documents().unwrap().len(), 1);
    assert!(updated(dir.path(), &mut index, &mut model).full_check);
    assert_eq!(index.stored_layout().unwrap(), Some(settings().layout()));
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

    let report = update_index(dir.path(), &mut index, &settings(), &mut model).unwrap();
    assert!(matches!(report, UpdateOutcome::Updated(_)));
    assert_eq!(paths(&index), ["docs/a.md", "docs/b.md"]);
    assert_eq!(
        model.calls, 3,
        "transient failure recovered on the next attempt"
    );

    model.fail_on_call = None;
    let report = updated(dir.path(), &mut index, &mut model);

    assert!(!report.full_check);
    assert_eq!(report.unchanged, 2);
    assert_eq!(report.chunks_embedded, 0);
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
        cause(&error),
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
    model.returned_dim = DIM + 1;

    let error = rebuild_index(dir.path(), &path, &settings(), &mut model).unwrap_err();

    assert!(matches!(cause(&error), IndexingError::Embed(_)));
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

    fn cancel_requested(&self) -> bool {
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

    assert!(matches!(cause(&error), IndexingError::Cancelled));
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

    assert!(matches!(cause(&error), IndexingError::Cancelled));
    assert!(!with_suffix(&path, ".new").exists());
    let index = Index::open(&path, Journal::Wal).unwrap();
    assert_eq!(index.document_count().unwrap(), 2);
    assert_eq!(index.stored_fingerprint().unwrap().unwrap().model, "old");
}

#[test]
fn cancellation_after_the_last_document_keeps_the_old_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let path = built_project(dir.path());
    let error = rebuild_index_observed(
        dir.path(),
        &path,
        &settings(),
        &mut FakeEmbedder::new("new"),
        &StopAfter::new(2),
        || panic!("остановленный проход не должен ждать подмену"),
    )
    .unwrap_err();
    assert!(matches!(cause(&error), IndexingError::Cancelled));
    assert!(!with_suffix(&path, ".new").exists());
    let index = Index::open_read_only(&path).unwrap();
    assert_eq!(index.stored_fingerprint().unwrap().unwrap().model, "old");
    assert_eq!(index.document_count().unwrap(), 2);
}

#[test]
fn cancelled_fresh_update_and_rebuild_do_not_prepare_the_model() {
    struct NoPrepare(FakeEmbedder);
    impl Embedder for NoPrepare {
        fn fingerprint(&self) -> &ModelFingerprint {
            self.0.fingerprint()
        }
        fn prepare(&mut self) -> Result<(), EmbedError> {
            panic!("отменённый проход не должен загружать модель");
        }
        fn embed(&mut self, _: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
            panic!("отменённый проход не должен считать embed");
        }
    }
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/a.md", "# Документ\n");
    let mut index = open(dir.path());
    let mut embedder = NoPrepare(FakeEmbedder::new("m"));
    let error = update_index_observed(
        dir.path(),
        &mut index,
        &settings(),
        &mut embedder,
        &StopAfter::new(0),
    )
    .unwrap_err();
    assert!(matches!(cause(&error), IndexingError::Cancelled));
    assert!(index.stored_fingerprint().unwrap().is_none());
    index.close().unwrap();
    let path = index_path(dir.path());
    let before = fs::read(&path).unwrap();
    let error = rebuild_index_observed(
        dir.path(),
        &path,
        &settings(),
        &mut embedder,
        &StopAfter::new(0),
        || panic!("отменённый проход не подменяет индекс"),
    )
    .unwrap_err();
    assert!(matches!(cause(&error), IndexingError::Cancelled));
    assert!(!with_suffix(&path, ".new").exists());
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn cancellation_while_waiting_for_swap_keeps_the_old_index() {
    let dir = tempfile::tempdir().unwrap();
    let path = built_project(dir.path());
    let observer = StopAfter::new(usize::MAX);
    let error = rebuild_index_observed(
        dir.path(),
        &path,
        &settings(),
        &mut FakeEmbedder::new("new"),
        &observer,
        || observer.done.set(usize::MAX),
    )
    .unwrap_err();
    assert!(matches!(cause(&error), IndexingError::Cancelled));
    assert!(!with_suffix(&path, ".new").exists());
    let index = Index::open_read_only(&path).unwrap();
    assert_eq!(index.stored_fingerprint().unwrap().unwrap().model, "old");
}

#[test]
fn rebuild_never_waits_for_pause_under_the_swap_guard() {
    struct Observer(std::cell::Cell<bool>);
    impl IndexObserver for Observer {
        fn should_stop(&self) -> bool {
            assert!(
                !self.0.get(),
                "ожидание паузы не допускается под write gate"
            );
            false
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = built_project(dir.path());
    let observer = Observer(std::cell::Cell::new(false));
    rebuild_index_observed(
        dir.path(),
        &path,
        &settings(),
        &mut FakeEmbedder::new("new"),
        &observer,
        || observer.0.set(true),
    )
    .unwrap();
    let index = Index::open_read_only(&path).unwrap();
    assert_eq!(index.stored_fingerprint().unwrap().unwrap().model, "new");
}

struct Deferring(&'static str);
impl IndexObserver for Deferring {
    fn defer(&self, path: &str, _full_check: bool) -> bool {
        path.ends_with(self.0)
    }
}

#[test]
fn a_deferred_document_keeps_its_old_version_until_the_next_check() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/a.md", "# Старый\n\nТекст\n");
    write(dir.path(), "docs/b.md", "# Второй\n");
    let mut index = open(dir.path());
    updated(dir.path(), &mut index, &mut FakeEmbedder::new("m"));
    let before: Vec<_> = index.documents().unwrap();
    write(
        dir.path(),
        "docs/a.md",
        "# Новый\n\nСодержимое изменено и стало длиннее\n",
    );
    write(dir.path(), "docs/b.md", "# Второй, правка\n");
    write(dir.path(), "docs/c.md", "# Третий\n");

    let outcome = update_index_observed(
        dir.path(),
        &mut index,
        &settings(),
        &mut FakeEmbedder::new("m"),
        &Deferring("a.md"),
    )
    .unwrap();
    let UpdateOutcome::Updated(report) = outcome else {
        panic!("ждали обновление");
    };
    assert_eq!(report.deferred, 1);
    assert_eq!((report.added, report.changed), (1, 1));
    let a_before = before.iter().find(|d| d.path.ends_with("a.md")).unwrap();
    let a_now = index
        .documents()
        .unwrap()
        .into_iter()
        .find(|d| d.path.ends_with("a.md"))
        .unwrap();
    assert_eq!(
        a_now.hash, a_before.hash,
        "отложенный документ не пересчитан"
    );

    let report = updated(dir.path(), &mut index, &mut FakeEmbedder::new("m"));
    assert_eq!((report.deferred, report.changed), (0, 1));
}

fn touch_index_files(base: &Path, suffixes: &[&str]) {
    for suffix in suffixes {
        std::fs::write(with_suffix(base, suffix), suffix).unwrap();
    }
}

#[test]
fn recovery_drops_old_when_the_swap_finished_and_always_drops_new() {
    let dir = tempfile::tempdir().unwrap();
    let index = dir.path().join("index.sqlite");
    touch_index_files(&index, &["", ".old", ".old-wal", ".new", ".new-shm"]);

    recover_index_files(&index).unwrap();

    assert_eq!(std::fs::read_to_string(&index).unwrap(), "");
    for suffix in [".old", ".old-wal", ".new", ".new-shm"] {
        assert!(!with_suffix(&index, suffix).exists(), "{suffix}");
    }
}

#[test]
fn recovery_returns_old_when_the_index_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let index = dir.path().join("index.sqlite");
    touch_index_files(&index, &["-wal", ".old", ".new"]);
    std::fs::write(with_suffix(&index, ".old"), "прежний").unwrap();

    recover_index_files(&index).unwrap();

    assert_eq!(std::fs::read_to_string(&index).unwrap(), "прежний");
    assert!(!with_suffix(&index, ".old").exists());
    assert!(!with_suffix(&index, ".new").exists());
    assert!(!with_suffix(&index, "-wal").exists());
}

#[test]
fn recovery_without_old_keeps_index_and_ignores_missing_files() {
    let dir = tempfile::tempdir().unwrap();
    let index = dir.path().join("index.sqlite");
    recover_index_files(&index).unwrap();
    assert!(!index.exists());
    touch_index_files(&index, &[""]);
    recover_index_files(&index).unwrap();
    assert!(index.exists());
}

#[test]
fn failed_rollback_reports_both_causes() {
    let dir = tempfile::tempdir().unwrap();
    let index = dir.path().join("index.sqlite");
    let new = with_suffix(&index, ".new");
    let old = with_suffix(&index, ".old");
    touch_index_files(&index, &["", ".new"]);
    let error = swap_with(&index, &new, &old, |from, to| {
        // Прежний индекс уходит в `.old`, новый не встаёт, и возврат тоже не удаётся.
        if from == index && to == old {
            std::fs::rename(from, to)
        } else if from == new {
            Err(io::Error::other("не встал"))
        } else {
            Err(io::Error::other("не вернулся"))
        }
    })
    .unwrap_err();

    let IndexingError::Swap { cause, rollback } = &error else {
        panic!("ожидалась ошибка подмены")
    };
    assert!(cause.to_string().contains("не встал"));
    assert!(
        rollback
            .as_ref()
            .unwrap()
            .to_string()
            .contains("не вернулся")
    );
    assert!(error.to_string().contains("не возвращён"));
    assert!(old.exists() && !index.exists());
}

#[test]
fn rollback_success_keeps_the_old_index_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let index = dir.path().join("index.sqlite");
    let new = with_suffix(&index, ".new");
    let old = with_suffix(&index, ".old");
    touch_index_files(&index, &[""]);
    // Нового файла нет: установка не удалась, откат проходит.
    let error = swap(&index, &new, &old).unwrap_err();
    let IndexingError::Swap { rollback, .. } = &error else {
        panic!("ожидалась ошибка подмены")
    };
    assert!(rollback.is_none());
    assert!(index.exists() && !old.exists());
}
