use bars_graph::*;
use bars_graph::{
    source::{InputDocument, build},
    store,
};
use rusqlite::{Connection, params};
use std::collections::BTreeMap;
fn input(path: &str, text: &str) -> InputDocument {
    InputDocument {
        path: format!("docs/{path}"),
        docs_path: path.into(),
        module: ".".into(),
        title: path.into(),
        text: text.into(),
        chunks: vec![],
    }
}
fn setup() -> (Connection, Dataset, BTreeMap<String, String>) {
    let mut db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT);INSERT INTO meta VALUES('schema_version','1');CREATE TABLE documents(path TEXT PRIMARY KEY,hash TEXT NOT NULL);").unwrap();
    let data = build(
        "fixture",
        "docs",
        vec![
            input("a.md", "# A\n\nТ-1 зависит от Т-2."),
            input("b.md", "# B\n\nТ-1 зависит от Т-2."),
        ],
    )
    .unwrap();
    for s in &data.sources {
        db.execute(
            "INSERT INTO documents VALUES(?1,?2)",
            params![s.meta.path, s.meta.hash],
        )
        .unwrap();
    }
    store::ensure_schema(&mut db).unwrap();
    let manifest = store::document_manifest(&db).unwrap();
    (db, data, manifest)
}
#[test]
fn migration_roundtrip_preserves_embedding_schema_and_data() {
    let (mut db, data, manifest) = setup();
    store::publish(&mut db, &data, &manifest).unwrap();
    assert_eq!(store::load(&db).unwrap().snapshot, data.snapshot);
    assert_eq!(
        db.query_row(
            "SELECT value FROM meta WHERE key='schema_version'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "1"
    );
    store::ensure_schema(&mut db).unwrap();
    assert_eq!(store::load(&db).unwrap().sources.len(), 2);
}
#[test]
fn method_change_invalidates_cached_snapshot_even_with_unchanged_sources() {
    let (mut db, data, manifest) = setup();
    store::publish(&mut db, &data, &manifest).unwrap();
    db.execute(
        "UPDATE graph_meta SET value='old-method' WHERE key='method'",
        [],
    )
    .unwrap();
    assert_eq!(store::status(&db).unwrap().state, LayerState::Stale);
    assert!(matches!(store::load(&db), Err(GraphError::NotReady(_))));
    store::set_state(&db, LayerState::Building, Some("new-job"), None).unwrap();
    assert!(matches!(store::load(&db), Err(GraphError::NotReady(_))));
}
#[test]
fn not_ready_messages_are_plain_russian_without_debug_output() {
    let (mut db, data, manifest) = setup();
    store::publish(&mut db, &data, &manifest).unwrap();
    db.execute(
        "UPDATE documents SET hash='changed' WHERE path='docs/a.md'",
        [],
    )
    .unwrap();
    let message = store::load(&db).unwrap_err().to_string();
    assert_eq!(
        message,
        "граф ещё не готов: документы изменились после построения, граф ждёт обновления"
    );
    for state in [
        LayerState::Disabled,
        LayerState::NotBuilt,
        LayerState::Building,
        LayerState::Stale,
        LayerState::Failed,
        LayerState::Cancelled,
        LayerState::Ready,
        LayerState::Partial,
    ] {
        let text = format!(
            "{}{}",
            state.not_ready_reason(),
            state.background_job_warning()
        );
        assert!(!text.is_ascii() && !text.contains("Some("), "{state:?}");
    }
}
#[test]
fn legacy_similarity_warning_is_read_as_plain_text() {
    let old =
        "Граф близости частичный: Some(ComparisonLimit); завершено 539/1856 строк".to_string();
    assert!(is_similarity_warning(&old));
    assert_eq!(
        humanize_warning(old),
        "Близость посчитана для 539 из 1856 чанков: достигнут предел сравнений"
    );
    let new = similarity_partial_warning(10, 20, "расчёт остановлен");
    assert!(is_similarity_warning(&new));
    assert_eq!(humanize_warning(new.clone()), new);
    let other = "Граф частичный: файлы с ошибками исключены".to_string();
    assert!(!is_similarity_warning(&other));
    assert_eq!(humanize_warning(other.clone()), other);
}
#[test]
fn text_mutation_invalidates_evidence_and_late_publish_rolls_back() {
    let (mut db, data, manifest) = setup();
    store::publish(&mut db, &data, &manifest).unwrap();
    db.execute(
        "UPDATE documents SET hash='changed' WHERE path='docs/a.md'",
        [],
    )
    .unwrap();
    assert_eq!(store::status(&db).unwrap().state, LayerState::Stale);
    assert!(matches!(store::load(&db), Err(GraphError::NotReady(_))));
    assert!(matches!(
        store::publish(&mut db, &data, &manifest),
        Err(GraphError::StaleRef(_))
    ));
    assert_eq!(
        store::status(&db).unwrap().snapshot.as_deref(),
        Some(data.snapshot.as_str())
    );
}
#[test]
fn independent_evidence_survives_source_deletion() {
    let (mut db, data, manifest) = setup();
    store::publish(&mut db, &data, &manifest).unwrap();
    let old = data
        .edges
        .iter()
        .find(|e| e.predicate == Predicate::DependsOn)
        .unwrap();
    assert_eq!(old.evidence.len(), 2);
    db.execute("DELETE FROM documents WHERE path='docs/a.md'", [])
        .unwrap();
    let next = build(
        "fixture",
        "docs",
        vec![input("b.md", "# B\n\nТ-1 зависит от Т-2.")],
    )
    .unwrap();
    let manifest = store::document_manifest(&db).unwrap();
    store::publish(&mut db, &next, &manifest).unwrap();
    let loaded = store::load(&db).unwrap();
    let current = loaded
        .edges
        .iter()
        .find(|e| e.predicate == Predicate::DependsOn)
        .unwrap();
    assert_eq!(old.id, current.id);
    assert_eq!(current.evidence.len(), 1);
    assert_eq!(loaded.sources.len(), 1);
}
#[test]
fn interrupted_job_keeps_previous_consistent_snapshot_and_bad_quote_never_publishes() {
    let (mut db, mut data, manifest) = setup();
    store::publish(&mut db, &data, &manifest).unwrap();
    for state in [
        LayerState::Building,
        LayerState::Cancelled,
        LayerState::Failed,
    ] {
        store::set_state(&db, state, Some("job"), None).unwrap();
        assert_eq!(store::load(&db).unwrap().snapshot, data.snapshot);
    }
    data.evidence[0].quote.push_str("wrong");
    assert!(store::publish(&mut db, &data, &manifest).is_err());
    assert_eq!(store::load(&db).unwrap().snapshot, data.snapshot);
}

#[test]
fn failed_file_is_excluded_but_manifest_still_guards_late_publication() {
    let (mut db, data, manifest) = setup();
    store::publish(&mut db, &data, &manifest).unwrap();
    let mut next = build(
        "fixture",
        "docs",
        vec![input("b.md", "# B\n\nТ-1 зависит от Т-2.")],
    )
    .unwrap();
    next.failed_sources.push(data.sources[0].meta.clone());
    next.fingerprint();
    let errors = vec![GraphFileError {
        path: "docs/a.md".into(),
        code: "unreadable".into(),
        message: "Файл недоступен".into(),
    }];
    store::publish_with_errors(&mut db, &next, &manifest, &errors).unwrap();
    let status = store::status(&db).unwrap();
    assert_eq!(status.file_errors_total, 1);
    assert_eq!(status.layers["structure"].state, LayerState::Partial);
    assert_eq!(status.layers["structure"].total, 2);
    let loaded = store::load(&db).unwrap();
    assert_eq!(loaded.sources.len(), 1);
    let reply = query::execute(
        &loaded,
        &GraphRequest {
            scope: Scope {
                files: vec!["a.md".into()],
                modules: vec![],
            },
            ..Default::default()
        },
        || false,
    )
    .unwrap();
    assert_eq!(reply.coverage.failed, 1);
    assert_eq!(reply.coverage.processed, 0);
    assert_eq!(reply.coverage.denominator, Some(1));
    assert!(
        loaded
            .evidence
            .iter()
            .all(|e| e.document != data.sources[0].meta.reference)
    );
    db.execute("UPDATE documents SET hash='new' WHERE path='docs/a.md'", [])
        .unwrap();
    assert!(store::load(&db).is_err());
    assert!(matches!(
        store::publish_with_errors(&mut db, &next, &manifest, &errors),
        Err(GraphError::StaleRef(_))
    ));
}

#[test]
fn diagnostics_and_pause_survive_schema_reopen_without_changing_snapshot() {
    let (mut db, data, manifest) = setup();
    store::publish(&mut db, &data, &manifest).unwrap();
    store::set_paused(&db, true).unwrap();
    let progress = GraphProgress {
        phase: "structure".into(),
        processed: 1,
        total: Some(2),
        elapsed_ms: 20,
    };
    store::set_progress(&db, &progress).unwrap();
    store::ensure_schema(&mut db).unwrap();
    let status = store::status(&db).unwrap();
    assert!(status.paused);
    assert_eq!(status.progress, Some(progress));
    assert_eq!(store::load(&db).unwrap().snapshot, data.snapshot);
    assert_eq!(status.layers["semantic"].state, LayerState::Partial);
    assert_eq!(status.layers["structure"].state, LayerState::Ready);
}
