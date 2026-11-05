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

// Формат v1 зафиксирован до исправления: большие значения и метки в WITHOUT ROWID.
fn legacy(db: &Connection) {
    let names: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type='trigger'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for name in names {
        db.execute_batch(&format!("DROP TRIGGER {name}")).unwrap();
    }
    db.execute_batch("CREATE TABLE old_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL) WITHOUT ROWID;
        INSERT INTO old_meta SELECT * FROM graph_meta; INSERT INTO old_meta SELECT * FROM graph_payload;
        INSERT INTO old_meta VALUES('read_epoch','legacy-read'),('read_revision','123'),('knn_input_epoch','legacy-input'),('knn_input_revision','456');
        DROP TABLE graph_meta; ALTER TABLE old_meta RENAME TO graph_meta;
        DROP TABLE graph_payload; DROP TABLE graph_revisions;
        UPDATE graph_meta SET value='1' WHERE key='schema_version';
        CREATE TRIGGER graph_read_graph_nodes_DELETE AFTER DELETE ON graph_nodes BEGIN UPDATE graph_meta SET value=CAST(value AS INTEGER)+1 WHERE key='read_revision'; END;
        CREATE TRIGGER graph_documents_insert AFTER INSERT ON documents BEGIN INSERT INTO graph_meta VALUES('state','stale') ON CONFLICT(key) DO UPDATE SET value='stale'; END;").unwrap();
}

#[test]
fn v1_migration_preserves_bytes_unknown_metadata_pause_and_graph_and_is_idempotent() {
    let (mut db, data, manifest) = setup();
    store::publish(&mut db, &data, &manifest).unwrap();
    for (key, value) in [
        (store::PayloadKey::Prepared, "{invalid legacy json"),
        (store::PayloadKey::Checkpoint, ""),
        (store::PayloadKey::Complete, "{\"cursor\":123}"),
    ] {
        store::write_payload(&db, key, value).unwrap();
    }
    db.execute_batch("INSERT INTO graph_meta VALUES('unknown','opaque'),('paused','true'),('auto_stopped','target');").unwrap();
    legacy(&db);
    let before = store::load_previous(&db).unwrap();
    assert!(store::revision_stamp(&db).unwrap().is_none());
    let bytes:Vec<(String,String)>=db.prepare("SELECT key,value FROM graph_meta WHERE key NOT IN ('schema_version','read_epoch','read_revision','knn_input_epoch','knn_input_revision') ORDER BY key").unwrap().query_map([],|r|Ok((r.get(0)?,r.get(1)?))).unwrap().collect::<Result<_,_>>().unwrap();
    store::ensure_schema(&mut db).unwrap();
    for (key, value) in bytes {
        let table = if key.starts_with("knn_") {
            "graph_payload"
        } else {
            "graph_meta"
        };
        assert_eq!(
            db.query_row(
                &format!("SELECT value FROM {table} WHERE key=?1"),
                [key],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            value
        );
    }
    assert_eq!(store::load_previous(&db).unwrap().snapshot, before.snapshot);
    assert_eq!(store::load_previous(&db).unwrap().evidence, before.evidence);
    let stamp = store::revision_stamp(&db).unwrap();
    store::ensure_schema(&mut db).unwrap();
    assert_eq!(stamp, store::revision_stamp(&db).unwrap());
    assert!(store::status(&db).unwrap().paused);
    assert!(
        db.execute("INSERT INTO graph_meta VALUES('knn_prepared','wrong')", [])
            .is_err()
    );
    assert!(
        db.execute("INSERT INTO graph_payload VALUES('unknown','wrong')", [])
            .is_err()
    );
}

#[test]
fn migration_rejects_unknown_dependencies_future_and_incomplete_versions_without_loss() {
    let (mut db, _, _) = setup();
    legacy(&db);
    db.execute_batch("CREATE VIEW external_meta AS SELECT * FROM graph_meta")
        .unwrap();
    assert!(store::ensure_schema(&mut db).is_err());
    assert_eq!(
        db.query_row(
            "SELECT value FROM graph_meta WHERE key='schema_version'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "1"
    );
    db.execute_batch(
        "DROP VIEW external_meta; UPDATE graph_meta SET value='99' WHERE key='schema_version'",
    )
    .unwrap();
    assert!(store::ensure_schema(&mut db).is_err());
    assert!(store::load_previous(&db).is_err());
    db.execute_batch("UPDATE graph_meta SET value='2' WHERE key='schema_version'")
        .unwrap();
    assert!(store::ensure_schema(&mut db).is_err());
    assert!(store::revision_stamp(&db).is_err());
}

#[test]
fn revisions_cover_inputs_links_cascades_old_new_keys_rollback_and_overflow() {
    let (mut db, data, manifest) = setup();
    store::publish(&mut db, &data, &manifest).unwrap();
    let stamp = store::revision_stamp(&db).unwrap().unwrap();
    store::write_payload(&db, store::PayloadKey::Checkpoint, "large data").unwrap();
    assert_eq!(stamp, store::revision_stamp(&db).unwrap().unwrap());
    db.execute("DELETE FROM graph_edge_evidence", []).unwrap();
    let links = store::revision_stamp(&db).unwrap().unwrap();
    assert_ne!(links.read, stamp.read);
    assert_eq!(links.input, stamp.input);
    let tx = db.transaction().unwrap();
    tx.execute("DELETE FROM graph_node_sources", []).unwrap();
    assert_ne!(
        store::revision_stamp(&tx).unwrap().unwrap().read,
        links.read
    );
    tx.rollback().unwrap();
    assert_eq!(links, store::revision_stamp(&db).unwrap().unwrap());
    db.execute("INSERT INTO meta VALUES('chunking_version','v1')", [])
        .unwrap();
    let input = store::revision_stamp(&db).unwrap().unwrap();
    assert_ne!(links.input, input.input);
    db.execute(
        "UPDATE meta SET key='irrelevant' WHERE key='chunking_version'",
        [],
    )
    .unwrap();
    let renamed = store::revision_stamp(&db).unwrap().unwrap();
    assert_ne!(renamed.input, input.input);
    db.execute("UPDATE meta SET value='v2' WHERE key='irrelevant'", [])
        .unwrap();
    assert_eq!(
        renamed.input,
        store::revision_stamp(&db).unwrap().unwrap().input
    );
    db.execute("DELETE FROM graph_sources", []).unwrap();
    assert_ne!(
        renamed.read,
        store::revision_stamp(&db).unwrap().unwrap().read
    );
    db.execute(
        "UPDATE graph_revisions SET read_revision=9223372036854775807",
        [],
    )
    .unwrap();
    assert!(
        db.execute("INSERT INTO graph_aux VALUES('overflow','x')", [])
            .is_err()
    );
    db.execute("DELETE FROM graph_revisions", []).unwrap();
    assert!(
        db.execute("INSERT INTO graph_aux VALUES('missing','x')", [])
            .is_err()
    );
    assert!(store::ensure_schema(&mut db).is_err());
}

#[test]
fn migration_busy_and_sqlite_interrupt_leave_v1_whole() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite");
    let (db, _, _) = setup();
    legacy(&db);
    db.execute("VACUUM INTO ?1", [path.to_str().unwrap()])
        .unwrap();
    let mut writer = Connection::open(&path).unwrap();
    let blocker = Connection::open(&path).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    writer.busy_timeout(std::time::Duration::ZERO).unwrap();
    assert!(store::ensure_schema(&mut writer).is_err());
    blocker.execute_batch("ROLLBACK").unwrap();
    writer.progress_handler(1, Some(|| true)).unwrap();
    assert!(store::ensure_schema(&mut writer).is_err());
    writer.progress_handler(0, None::<fn() -> bool>).unwrap();
    assert_eq!(
        writer
            .query_row(
                "SELECT value FROM graph_meta WHERE key='schema_version'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "1"
    );
    store::ensure_schema(&mut writer).unwrap();
}

#[test]
fn migration_failure_at_each_ddl_copy_and_commit_phase_rolls_back_everything() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization, TransactionOperation};
    for phase in 0..8 {
        let (mut db, _, _) = setup();
        store::write_payload(&db, store::PayloadKey::Prepared, "{preserve me}").unwrap();
        legacy(&db);
        db.authorizer(Some(move |ctx: AuthContext<'_>| {
            let deny = match ctx.action {
                AuthAction::DropTrigger { .. } => phase == 0,
                AuthAction::CreateTable {
                    table_name: "graph_meta_v2",
                } => phase == 1,
                AuthAction::CreateTable {
                    table_name: "graph_payload",
                } => phase == 2,
                AuthAction::Insert {
                    table_name: "graph_payload",
                } => phase == 3,
                AuthAction::DropTable {
                    table_name: "graph_meta",
                } => phase == 4,
                AuthAction::AlterTable {
                    table_name: "graph_meta_v2",
                    ..
                } => phase == 5,
                AuthAction::CreateTrigger { .. } => phase == 6,
                AuthAction::Transaction {
                    // rusqlite 0.40.2 отображает COMMIT в Unknown.
                    operation: TransactionOperation::Unknown,
                } => phase == 7,
                _ => false,
            };
            if deny {
                Authorization::Deny
            } else {
                Authorization::Allow
            }
        }))
        .unwrap();
        assert!(store::ensure_schema(&mut db).is_err(), "phase {phase}");
        db.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
            .unwrap();
        assert!(db.is_autocommit());
        assert_eq!(
            db.query_row(
                "SELECT value FROM graph_meta WHERE key='schema_version'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "1"
        );
        assert_eq!(
            store::read_payload(&db, store::PayloadKey::Prepared)
                .unwrap()
                .as_deref(),
            Some("{preserve me}")
        );
        assert!(!db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name IN ('graph_payload','graph_meta_v2','graph_revisions'))",[],|r|r.get::<_,bool>(0)).unwrap());
        store::ensure_schema(&mut db).unwrap();
    }
}

#[test]
fn migration_sqlite_full_rolls_back_payload_and_remains_retryable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("full.sqlite");
    let (db, _, _) = setup();
    store::write_payload(&db, store::PayloadKey::Prepared, &"x".repeat(1024 * 1024)).unwrap();
    legacy(&db);
    db.execute("VACUUM INTO ?1", [path.to_str().unwrap()])
        .unwrap();
    let mut db = Connection::open(path).unwrap();
    let pages: i64 = db.query_row("PRAGMA page_count", [], |r| r.get(0)).unwrap();
    db.pragma_update(None, "max_page_count", pages).unwrap();
    assert!(store::ensure_schema(&mut db).is_err());
    assert_eq!(
        store::read_payload(&db, store::PayloadKey::Prepared)
            .unwrap()
            .unwrap()
            .len(),
        1024 * 1024
    );
    assert!(store::revision_stamp(&db).unwrap().is_none());
    db.pragma_update(None, "max_page_count", pages * 4).unwrap();
    store::ensure_schema(&mut db).unwrap();
}

#[test]
fn readonly_v1_is_not_migrated_and_wal_reader_sees_only_committed_generation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite");
    let (mut db, data, manifest) = setup();
    store::publish(&mut db, &data, &manifest).unwrap();
    legacy(&db);
    db.execute("VACUUM INTO ?1", [path.to_str().unwrap()])
        .unwrap();
    let reader =
        Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(
        store::load_previous(&reader).unwrap().snapshot,
        data.snapshot
    );
    assert!(store::revision_stamp(&reader).unwrap().is_none());
    drop(reader);
    let mut writer = Connection::open(&path).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    store::ensure_schema(&mut writer).unwrap();
    let reader =
        Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let before = store::revision_stamp(&reader).unwrap();
    let tx = writer.transaction().unwrap();
    tx.execute("UPDATE graph_nodes SET data=data", []).unwrap();
    assert_eq!(before, store::revision_stamp(&reader).unwrap());
    tx.commit().unwrap();
    assert_ne!(before, store::revision_stamp(&reader).unwrap());
    assert_eq!(
        store::load_previous(&reader).unwrap().snapshot,
        data.snapshot
    );
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
fn load_deadline_does_not_return_partial_data_or_leave_sqlite_interrupted() {
    let (mut db, data, manifest) = setup();
    store::publish(&mut db, &data, &manifest).unwrap();
    assert!(matches!(
        store::load_previous_until(&db, std::time::Instant::now()),
        Err(GraphError::TimedOut)
    ));
    let loaded = store::load_previous_until(
        &db,
        std::time::Instant::now() + std::time::Duration::from_secs(5),
    )
    .unwrap();
    assert_eq!(loaded.snapshot, data.snapshot);
    assert_eq!(loaded.evidence, data.evidence);
    assert_eq!(
        db.query_row("SELECT 1", [], |r| r.get::<_, i32>(0))
            .unwrap(),
        1
    );
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

#[test]
fn freshness_completeness_and_job_are_independent_and_old_snapshot_is_explicit() {
    let (mut db, mut data, manifest) = setup();
    data.similarity_state = LayerState::Ready;
    store::publish(&mut db, &data, &manifest).unwrap();
    let status = store::status(&db).unwrap();
    assert_eq!(status.freshness, Freshness::Current);
    assert_eq!(status.completeness, Completeness::Unknown);
    assert_eq!(status.state, LayerState::Ready);
    assert_eq!(
        status.layers["semantic"].completeness,
        Completeness::Unknown
    );
    assert_eq!(
        status.layers["similarity"].completeness,
        Completeness::Complete
    );
    for (state, job) in [
        (LayerState::Building, JobState::Running),
        (LayerState::Cancelled, JobState::Stopped),
        (LayerState::Failed, JobState::Failed),
    ] {
        store::set_state(&db, state, Some("job"), None).unwrap();
        let status = store::status(&db).unwrap();
        assert_eq!(status.freshness, Freshness::Current);
        assert_eq!(status.job_state, job);
        assert_eq!(status.state, LayerState::Ready);
    }
    db.execute("DELETE FROM documents WHERE path='docs/a.md'", [])
        .unwrap();
    store::set_state(&db, LayerState::Building, Some("job"), None).unwrap();
    let status = store::status(&db).unwrap();
    assert_eq!(status.state, LayerState::Stale);
    assert_eq!(status.freshness, Freshness::Stale);
    assert_eq!(status.job_state, JobState::Running);
    assert_eq!(status.completeness, Completeness::Unknown);
    assert!(store::load(&db).is_err());
    let old = store::load_previous(&db).unwrap();
    assert_eq!(old.snapshot, data.snapshot);
    assert_eq!(old.sources.len(), 2);
    assert!(old.warnings.iter().any(|w| w.contains("Граф устарел")));
    assert_eq!(old.layers["structure"].source_revisions, manifest);
    let reply = query::execute(&old, &GraphRequest::default(), || false).unwrap();
    assert_eq!(reply.layers["structure"].freshness, Freshness::Stale);
}

#[test]
fn disabled_layers_do_not_lower_aggregate_and_partial_is_not_stale() {
    let (mut db, data, manifest) = setup();
    store::publish(&mut db, &data, &manifest).unwrap();
    let mut status = store::status(&db).unwrap();
    status.layers.get_mut("semantic").unwrap().availability = Availability::Disabled;
    status.layers.get_mut("similarity").unwrap().availability = Availability::Disabled;
    status.derive_data_state();
    assert_eq!(status.completeness, Completeness::Complete);
    assert_eq!(status.freshness, Freshness::Current);
    let structure = status.layers.get_mut("structure").unwrap();
    structure.completeness = Completeness::Partial;
    status.derive_data_state();
    assert_eq!(status.freshness, Freshness::Current);
    assert_eq!(status.completeness, Completeness::Partial);
    status.layers.get_mut("structure").unwrap().availability = Availability::Disabled;
    status.derive_data_state();
    assert_eq!(status.availability, Availability::Disabled);
    assert_eq!(status.state, LayerState::Disabled);
}

#[test]
fn legacy_layers_load_without_writes_and_missing_vector_fingerprint_is_stale() {
    let (mut db, data, manifest) = setup();
    store::publish(&mut db, &data, &manifest).unwrap();
    let mut layers = serde_json::to_value(store::status(&db).unwrap().layers).unwrap();
    for layer in layers.as_object_mut().unwrap().values_mut() {
        for key in [
            "freshness",
            "completeness",
            "availability",
            "coverage",
            "source_revisions",
        ] {
            layer.as_object_mut().unwrap().remove(key);
        }
    }
    db.execute(
        "UPDATE graph_meta SET value=?1 WHERE key='layers'",
        [layers.to_string()],
    )
    .unwrap();
    db.execute("DELETE FROM graph_meta WHERE key='similarity_inputs'", [])
        .unwrap();
    let changes = db.total_changes();
    let status = store::status(&db).unwrap();
    assert_eq!(
        status.layers["semantic"].completeness,
        Completeness::Unknown
    );
    // Эта фикстура ещё не строила близость: отсутствующее поколение остаётся not_built.
    assert_eq!(status.layers["similarity"].freshness, Freshness::NotBuilt);
    assert_eq!(status.layers["structure"].source_revisions, manifest);
    store::load_previous(&db).unwrap();
    assert_eq!(db.total_changes(), changes);
}

#[test]
fn vector_model_change_marks_similarity_and_aggregate_stale() {
    let (mut db, mut data, manifest) = setup();
    data.similarity_state = LayerState::Ready;
    store::publish(&mut db, &data, &manifest).unwrap();
    db.execute(
        "INSERT INTO meta VALUES('model_fingerprint','new-model')",
        [],
    )
    .unwrap();
    let status = store::status(&db).unwrap();
    assert_eq!(status.layers["structure"].freshness, Freshness::Current);
    assert_eq!(status.layers["similarity"].freshness, Freshness::Stale);
    assert_eq!(status.freshness, Freshness::Stale);
    assert!(store::load(&db).is_err());
    assert!(
        store::load_previous(&db)
            .unwrap()
            .warnings
            .iter()
            .any(|w| w.contains("Граф устарел"))
    );
}

fn read_revision(db: &Connection) -> i64 {
    db.query_row(
        "SELECT read_revision FROM graph_revisions WHERE id=1",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

fn cache_size(db: &Connection) -> i64 {
    db.query_row("PRAGMA cache_size", [], |r| r.get(0)).unwrap()
}

fn has_object(db: &Connection, name: &str) -> bool {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name=?1)",
        [name],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn publication_raises_read_revision_exactly_once_and_cache_sees_it() {
    let (mut db, data, manifest) = setup();
    let before = read_revision(&db);
    let stamp = store::revision_stamp(&db).unwrap().unwrap();
    store::publish(&mut db, &data, &manifest).unwrap();
    assert_eq!(read_revision(&db), before + 1);
    assert_ne!(
        store::revision_stamp(&db).unwrap().unwrap().read,
        stamp.read
    );
    // Повторная публикация того же набора тоже даёт одно приращение.
    let again = read_revision(&db);
    store::publish(&mut db, &data, &manifest).unwrap();
    assert_eq!(read_revision(&db), again + 1);
    assert!(!has_publishing_key(&db));
}

fn has_publishing_key(db: &Connection) -> bool {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM graph_meta WHERE key='publishing')",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn publication_keeps_data_and_graph_triggers_work_outside_it() {
    let (mut db, data, manifest) = setup();
    store::publish(&mut db, &data, &manifest).unwrap();
    let loaded = store::load(&db).unwrap();
    assert_eq!(loaded.snapshot, data.snapshot);
    assert_eq!(loaded.nodes.len(), data.nodes.len());
    assert_eq!(loaded.edges.len(), data.edges.len());
    assert_eq!(loaded.evidence.len(), data.evidence.len());
    assert_eq!(loaded.sources.len(), data.sources.len());
    // Индексы рёбер пересозданы после массовой вставки.
    assert!(has_object(&db, "graph_edges_from"));
    assert!(has_object(&db, "graph_edges_to"));
    // Вне публикации триггеры по-прежнему считают каждую запись.
    let before = read_revision(&db);
    db.execute("INSERT INTO graph_aux VALUES('probe','{}')", [])
        .unwrap();
    assert_eq!(read_revision(&db), before + 1);
}

#[test]
fn publication_restores_cache_size_and_failed_one_leaves_no_trace() {
    let (mut db, data, manifest) = setup();
    store::publish(&mut db, &data, &manifest).unwrap();
    let cache = cache_size(&db);
    let revision = read_revision(&db);
    let mut broken = data.clone();
    broken.evidence[0].quote.push_str("wrong");
    assert!(store::publish(&mut db, &broken, &manifest).is_err());
    assert_eq!(cache_size(&db), cache);
    assert_eq!(read_revision(&db), revision);
    assert!(!has_publishing_key(&db));
    // Откат транзакции возвращает прежнее поколение вместе с индексами.
    let tx = db.transaction().unwrap();
    store::publish_in_transaction(&tx, &data, &manifest, &[]).unwrap();
    drop(tx);
    assert_eq!(read_revision(&db), revision);
    assert!(!has_publishing_key(&db));
    assert!(has_object(&db, "graph_edges_from"));
    assert_eq!(store::load(&db).unwrap().snapshot, data.snapshot);
    // Явный подъём кэша возвращает прежнее значение.
    let boost = store::CacheBoost::raise(&db).unwrap();
    assert_ne!(cache_size(&db), cache);
    boost.restore(&db).unwrap();
    assert_eq!(cache_size(&db), cache);
}

fn parts_count(db: &Connection) -> i64 {
    db.query_row("SELECT count(*) FROM graph_parts", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn publication_writes_parts_and_the_delta_base_marker() {
    let (mut db, data, manifest) = setup();
    assert!(!data.parts.is_empty());
    store::publish(&mut db, &data, &manifest).unwrap();
    assert_eq!(parts_count(&db), data.parts.len() as i64);
    assert_eq!(
        db.query_row(
            "SELECT value FROM graph_meta WHERE key='delta_base'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "1"
    );
    // Части удаляются вместе с источником (каскад), а не копятся.
    db.execute("DELETE FROM graph_sources WHERE path='docs/a.md'", [])
        .unwrap();
    assert!(parts_count(&db) < data.parts.len() as i64);
}

#[test]
fn schema_v2_moves_to_v3_without_touching_the_graph_and_without_delta_base() {
    let (mut db, data, manifest) = setup();
    store::publish(&mut db, &data, &manifest).unwrap();
    let nodes: i64 = db
        .query_row("SELECT count(*) FROM graph_nodes", [], |r| r.get(0))
        .unwrap();
    // Состояние схемы v2: нет таблицы частей, её триггеров и признака.
    db.execute_batch(
        "DROP TRIGGER graph_read_graph_parts_INSERT; DROP TRIGGER graph_read_graph_parts_UPDATE; DROP TRIGGER graph_read_graph_parts_DELETE;
         DROP TABLE graph_parts; DROP INDEX graph_edge_evidence_evidence;
         DELETE FROM graph_meta WHERE key='delta_base';
         UPDATE graph_meta SET value='2' WHERE key='schema_version';",
    )
    .unwrap();
    assert!(store::status(&db).is_ok());
    store::ensure_schema(&mut db).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT value FROM graph_meta WHERE key='schema_version'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "3"
    );
    assert_eq!(parts_count(&db), 0);
    assert_eq!(
        db.query_row("SELECT count(*) FROM graph_nodes", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        nodes
    );
    assert!(store::delta_base(&db).unwrap().is_none());
    // Повторное открытие ничего не меняет, а триггер чтения на новой таблице работает.
    let stamp = store::revision_stamp(&db).unwrap();
    store::ensure_schema(&mut db).unwrap();
    assert_eq!(stamp, store::revision_stamp(&db).unwrap());
    db.execute(
        "INSERT INTO graph_parts VALUES('docs/a.md','anchor','x','y')",
        [],
    )
    .unwrap();
    assert_ne!(stamp, store::revision_stamp(&db).unwrap());
}
