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
