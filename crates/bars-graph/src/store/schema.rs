use super::*;

const PAYLOAD: &str = "'knn_prepared','knn_checkpoint','knn_complete'";
const REVISIONS: &str = "'read_epoch','read_revision','knn_input_epoch','knn_input_revision'";
const GRAPH_TABLES: &[&str] = &[
    "graph_meta",
    "graph_sources",
    "graph_nodes",
    "graph_edges",
    "graph_evidence",
    "graph_aux",
    "graph_edge_evidence",
    "graph_node_sources",
];
const INPUT_TABLES: &[&str] = &["documents", "chunks", "vectors", "meta"];

fn exists(db: &Connection, name: &str) -> Result<bool, GraphError> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [name],
        |r| r.get(0),
    )?)
}

fn invalid(message: &str) -> GraphError {
    GraphError::InvalidData(message.into())
}

pub(super) fn version(db: &Connection) -> Result<Option<u32>, GraphError> {
    if !exists(db, "graph_meta")? {
        return Ok(None);
    }
    match meta(db, "schema_version")?.as_deref() {
        Some("1") => Ok(Some(1)),
        Some("2") => Ok(Some(2)),
        _ => Err(invalid("неподдерживаемая или неполная схема графа")),
    }
}

fn trigger_names() -> Vec<String> {
    let mut names = vec![
        "graph_documents_insert".into(),
        "graph_documents_delete".into(),
        "graph_documents_change".into(),
    ];
    for table in GRAPH_TABLES.iter().chain(INPUT_TABLES) {
        for event in ["INSERT", "UPDATE", "DELETE"] {
            names.push(format!("graph_read_{table}_{event}"));
        }
    }
    for table in INPUT_TABLES {
        for event in ["INSERT", "UPDATE", "DELETE"] {
            names.push(format!("knn_inputs_{table}_{event}"));
        }
    }
    names
}

fn migrate(db: &Connection) -> Result<(), GraphError> {
    let names = trigger_names();
    // Не удаляем чужие зависимости: ALTER/DROP иначе могут тихо изменить их смысл.
    let mut objects = db.prepare(
        "SELECT type,name,tbl_name,coalesce(sql,'') FROM sqlite_master WHERE sql IS NOT NULL",
    )?;
    for row in objects.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })? {
        let (kind, name, table, sql) = row?;
        if name == "graph_meta" || names.contains(&name) {
            continue;
        }
        if table == "graph_meta"
            || (kind != "index" && sql.to_ascii_lowercase().contains("graph_meta"))
        {
            return Err(invalid(
                "неизвестная зависимость от graph_meta; миграция отменена",
            ));
        }
    }
    drop(objects);
    for name in names {
        db.execute_batch(&format!("DROP TRIGGER IF EXISTS {name}"))?;
    }
    create_meta(db, "graph_meta_v2")?;
    create_special(db)?;
    db.execute_batch(&format!("INSERT INTO graph_payload SELECT key,value FROM graph_meta WHERE key IN ({PAYLOAD});
        INSERT INTO graph_meta_v2 SELECT key,value FROM graph_meta WHERE key NOT IN ({PAYLOAD},{REVISIONS});"))?;
    let same: bool = db.query_row(&format!("SELECT NOT EXISTS(SELECT key,value FROM graph_meta WHERE key NOT IN ({REVISIONS}) EXCEPT SELECT key,value FROM (SELECT key,value FROM graph_meta_v2 UNION ALL SELECT key,value FROM graph_payload))
        AND NOT EXISTS(SELECT key,value FROM (SELECT key,value FROM graph_meta_v2 UNION ALL SELECT key,value FROM graph_payload) EXCEPT SELECT key,value FROM graph_meta WHERE key NOT IN ({REVISIONS}))"), [], |r|r.get(0))?;
    if !same {
        return Err(invalid("неполный перенос метаданных графа"));
    }
    db.execute_batch("DROP TABLE graph_meta; ALTER TABLE graph_meta_v2 RENAME TO graph_meta;")?;
    Ok(())
}

fn create_meta(db: &Connection, name: &str) -> Result<(), GraphError> {
    db.execute_batch(&format!("CREATE TABLE {name}(key TEXT NOT NULL PRIMARY KEY CHECK(key NOT IN ({PAYLOAD},{REVISIONS})),value TEXT NOT NULL);"))?;
    Ok(())
}

fn create_special(db: &Connection) -> Result<(), GraphError> {
    db.execute_batch(&format!("CREATE TABLE graph_payload(key TEXT NOT NULL PRIMARY KEY CHECK(key IN ({PAYLOAD})),value TEXT NOT NULL);
        CREATE TABLE graph_revisions(id INTEGER PRIMARY KEY CHECK(id=1),read_epoch TEXT NOT NULL CHECK(length(read_epoch)=32 AND read_epoch NOT GLOB '*[^0-9A-Fa-f]*'),read_revision INTEGER NOT NULL CHECK(typeof(read_revision)='integer' AND read_revision>=0),knn_input_epoch TEXT NOT NULL CHECK(length(knn_input_epoch)=32 AND knn_input_epoch NOT GLOB '*[^0-9A-Fa-f]*'),knn_input_revision INTEGER NOT NULL CHECK(typeof(knn_input_revision)='integer' AND knn_input_revision>=0));
        INSERT INTO graph_revisions VALUES(1,hex(randomblob(16)),0,hex(randomblob(16)),0);"))?;
    Ok(())
}

fn check_trigger(db: &Connection, sql: &str, install: bool) -> Result<(), GraphError> {
    if install {
        db.execute_batch(sql)?;
    } else {
        let name = sql.split_whitespace().nth(2).expect("static trigger SQL");
        let actual: Option<String> = db
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='trigger' AND name=?1",
                [name],
                |r| r.get(0),
            )
            .optional()?;
        if actual.as_deref().map(|s| s.trim().trim_end_matches(';'))
            != Some(sql.trim().trim_end_matches(';'))
        {
            return Err(invalid("отсутствует или изменён триггер графа v2"));
        }
    }
    Ok(())
}

fn triggers(db: &Connection, install: bool) -> Result<(), GraphError> {
    if exists(db, "documents")? {
        for sql in [
            "CREATE TRIGGER graph_documents_insert AFTER INSERT ON documents BEGIN INSERT INTO graph_meta VALUES('state','stale') ON CONFLICT(key) DO UPDATE SET value='stale'; END;",
            "CREATE TRIGGER graph_documents_delete AFTER DELETE ON documents BEGIN INSERT INTO graph_meta VALUES('state','stale') ON CONFLICT(key) DO UPDATE SET value='stale'; END;",
            "CREATE TRIGGER graph_documents_change AFTER UPDATE OF hash,path ON documents WHEN OLD.hash<>NEW.hash OR OLD.path<>NEW.path BEGIN INSERT INTO graph_meta VALUES('state','stale') ON CONFLICT(key) DO UPDATE SET value='stale'; END;",
        ] {
            check_trigger(db, sql, install)?;
        }
    }
    for table in GRAPH_TABLES.iter().chain(INPUT_TABLES) {
        if !exists(db, table)? {
            continue;
        }
        for event in ["INSERT", "UPDATE", "DELETE"] {
            check_trigger(db,&format!("CREATE TRIGGER graph_read_{table}_{event} AFTER {event} ON {table} BEGIN
                SELECT CASE WHEN NOT EXISTS(SELECT 1 FROM graph_revisions WHERE id=1) THEN RAISE(ABORT,'missing graph revisions') END;
                UPDATE graph_revisions SET read_revision=read_revision+1 WHERE id=1; END;"),install)?;
        }
    }
    for table in INPUT_TABLES {
        if !exists(db, table)? {
            continue;
        }
        for event in ["INSERT", "UPDATE", "DELETE"] {
            let significant = |row: &str| {
                format!(
                    "{row}.key LIKE 'model_%' OR {row}.key IN ('docs_root','chunk_min_chars','chunk_max_chars','chunking_version')"
                )
            };
            let filter = if *table == "meta" {
                match event {
                    "UPDATE" => {
                        format!("WHEN ({}) OR ({})", significant("OLD"), significant("NEW"))
                    }
                    "DELETE" => format!("WHEN {}", significant("OLD")),
                    _ => format!("WHEN {}", significant("NEW")),
                }
            } else {
                String::new()
            };
            check_trigger(db,&format!("CREATE TRIGGER knn_inputs_{table}_{event} AFTER {event} ON {table} {filter} BEGIN
                SELECT CASE WHEN NOT EXISTS(SELECT 1 FROM graph_revisions WHERE id=1) THEN RAISE(ABORT,'missing graph revisions') END;
                UPDATE graph_revisions SET knn_input_revision=knn_input_revision+1 WHERE id=1; END;"),install)?;
        }
    }
    Ok(())
}

pub(super) fn validate(db: &Connection) -> Result<(), GraphError> {
    for name in GRAPH_TABLES
        .iter()
        .chain(["graph_payload", "graph_revisions"].iter())
    {
        if !exists(db, name)? {
            return Err(invalid("неполная схема графа v2"));
        }
    }
    revision_stamp(db)?;
    let misplaced: bool = db.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM graph_meta WHERE key IN ({PAYLOAD},{REVISIONS}))"),
        [],
        |r| r.get(0),
    )?;
    if misplaced {
        return Err(invalid("старые ключи в graph_meta v2"));
    }
    triggers(db, false)?;
    Ok(())
}

pub(super) fn ensure(db: &mut Connection) -> Result<(), GraphError> {
    db.pragma_update(None, "foreign_keys", "ON")?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let version = version(&tx)?;
    if version == Some(2) {
        validate(&tx)?;
        tx.commit()?;
        return Ok(());
    }
    if version == Some(1) {
        migrate(&tx)?;
    } else {
        create_meta(&tx, "graph_meta")?;
        create_special(&tx)?;
    }
    tx.execute_batch("CREATE TABLE IF NOT EXISTS graph_sources(path TEXT PRIMARY KEY,revision TEXT NOT NULL,data TEXT NOT NULL) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS graph_nodes(id TEXT PRIMARY KEY,kind TEXT NOT NULL,revision TEXT NOT NULL,data TEXT NOT NULL) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS graph_evidence(id TEXT PRIMARY KEY,source_path TEXT NOT NULL REFERENCES graph_sources(path) ON DELETE CASCADE,data TEXT NOT NULL) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS graph_edges(id TEXT PRIMARY KEY,from_id TEXT NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,to_id TEXT NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,predicate TEXT NOT NULL,data TEXT NOT NULL) WITHOUT ROWID;
        CREATE INDEX IF NOT EXISTS graph_edges_from ON graph_edges(from_id,predicate);
        CREATE INDEX IF NOT EXISTS graph_edges_to ON graph_edges(to_id,predicate);
        CREATE TABLE IF NOT EXISTS graph_edge_evidence(edge_id TEXT NOT NULL REFERENCES graph_edges(id) ON DELETE CASCADE,evidence_id TEXT NOT NULL REFERENCES graph_evidence(id) ON DELETE CASCADE,PRIMARY KEY(edge_id,evidence_id)) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS graph_node_sources(node_id TEXT NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,source_path TEXT NOT NULL REFERENCES graph_sources(path) ON DELETE CASCADE,PRIMARY KEY(node_id,source_path)) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS graph_aux(key TEXT PRIMARY KEY,data TEXT NOT NULL) WITHOUT ROWID;")?;
    if meta(&tx, "state")?.is_none() {
        set_meta(&tx, "state", "not_built")?;
    }
    triggers(&tx, true)?;
    set_meta(&tx, "schema_version", "2")?;
    if version == Some(1)
        && meta(&tx, "snapshot")?.is_some()
        && meta(&tx, "published_input_stamp")?.is_none()
    {
        set_meta(
            &tx,
            "published_input_stamp",
            &revision_stamp(&tx)?.expect("v2 initialized").input,
        )?;
    }
    validate(&tx)?;
    let violation = tx.prepare("PRAGMA foreign_key_check")?.exists([])?;
    if violation {
        return Err(invalid("нарушены внешние ключи графа"));
    }
    tx.commit()?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionStamp {
    pub read: String,
    pub input: String,
}

/// Один SELECT, без payload. V1 читается, но не удерживается в кэше.
pub fn revision_stamp(db: &Connection) -> Result<Option<RevisionStamp>, GraphError> {
    struct Raw {
        version: String,
        read: String,
        input: String,
        rt: String,
        it: String,
        rr: i64,
        ir: i64,
        schema: i64,
        count: i64,
    }
    let result = db.query_row("SELECT m.value,r.read_epoch,r.knn_input_epoch,typeof(r.read_revision),typeof(r.knn_input_revision),r.read_revision,r.knn_input_revision,(SELECT schema_version FROM pragma_schema_version),(SELECT count(*) FROM graph_revisions) FROM graph_meta m LEFT JOIN graph_revisions r ON r.id=1 WHERE m.key='schema_version'", [], |r|Ok(Raw{version:r.get(0)?,read:r.get(1)?,input:r.get(2)?,rt:r.get(3)?,it:r.get(4)?,rr:r.get(5)?,ir:r.get(6)?,schema:r.get(7)?,count:r.get(8)?})).optional();
    let stamp = match result {
        Ok(value) => value,
        Err(rusqlite::Error::SqliteFailure(_, Some(ref message)))
            if message == "no such table: graph_revisions" && version(db)? == Some(1) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    let Some(Raw {
        version,
        read,
        input,
        rt,
        it,
        rr,
        ir,
        schema,
        count,
    }) = stamp
    else {
        return Err(invalid("отсутствуют метки графа"));
    };
    let epoch = |s: &str| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit());
    if version != "2"
        || !epoch(&read)
        || !epoch(&input)
        || rt != "integer"
        || it != "integer"
        || rr < 0
        || ir < 0
        || count != 1
    {
        return Err(invalid("повреждены метки графа v2"));
    }
    Ok(Some(RevisionStamp {
        read: format!("2:{read}:{rr}:{schema}"),
        input: format!("2:{input}:{ir}"),
    }))
}
