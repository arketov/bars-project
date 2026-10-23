use crate::*;
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{BTreeMap, BTreeSet};

pub const GRAPH_SCHEMA_VERSION: u32 = 1;

pub use crate::GraphStatus;

fn meta(db: &Connection, key: &str) -> Result<Option<String>, GraphError> {
    Ok(db
        .query_row("SELECT value FROM graph_meta WHERE key=?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}
fn set_meta(db: &Connection, key: &str, value: &str) -> Result<(), GraphError> {
    db.execute(
        "INSERT INTO graph_meta VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Добавочная схема графа; meta/schema_version embeddings не изменяется.
pub fn ensure_schema(db: &mut Connection) -> Result<(), GraphError> {
    db.pragma_update(None, "foreign_keys", "ON")?;
    let tx = db.transaction()?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS graph_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL) WITHOUT ROWID;")?;
    if let Some(version) = meta(&tx, "schema_version")? {
        if version.parse::<u32>().ok() != Some(GRAPH_SCHEMA_VERSION) {
            return Err(GraphError::InvalidData(format!(
                "схема графа {version} не поддерживается"
            )));
        }
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
    let documents: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='documents')",
        [],
        |r| r.get(0),
    )?;
    if documents {
        tx.execute_batch("CREATE TRIGGER IF NOT EXISTS graph_documents_insert AFTER INSERT ON documents BEGIN
                INSERT INTO graph_meta VALUES('state','stale') ON CONFLICT(key) DO UPDATE SET value='stale'; END;
            CREATE TRIGGER IF NOT EXISTS graph_documents_delete AFTER DELETE ON documents BEGIN
                INSERT INTO graph_meta VALUES('state','stale') ON CONFLICT(key) DO UPDATE SET value='stale'; END;
            CREATE TRIGGER IF NOT EXISTS graph_documents_change AFTER UPDATE OF hash,path ON documents WHEN OLD.hash<>NEW.hash OR OLD.path<>NEW.path BEGIN
                INSERT INTO graph_meta VALUES('state','stale') ON CONFLICT(key) DO UPDATE SET value='stale'; END;")?;
    }
    set_meta(&tx, "schema_version", &GRAPH_SCHEMA_VERSION.to_string())?;
    if meta(&tx, "state")?.is_none() {
        set_meta(&tx, "state", "not_built")?;
    }
    tx.commit()?;
    Ok(())
}

pub fn document_manifest(db: &Connection) -> Result<BTreeMap<String, String>, GraphError> {
    let mut stmt = db.prepare("SELECT path,hash FROM documents ORDER BY path")?;
    Ok(stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?)
}

pub fn validate(data: &Dataset) -> Result<(), GraphError> {
    let mut sources = BTreeMap::new();
    let mut nodes = BTreeSet::new();
    let mut proof = BTreeSet::new();
    for source in &data.sources {
        if sources
            .insert(source.meta.reference.id.clone(), source)
            .is_some()
            || source.meta.hash != blake3::hash(source.text.as_bytes()).to_hex().as_str()
            || source.meta.reference.kind != Kind::Document
            || source.meta.reference.revision != source.meta.hash
        {
            return Err(GraphError::InvalidData(
                "источник не соответствует ревизии".into(),
            ));
        }
    }
    for ev in &data.evidence {
        let source = sources
            .get(&ev.document.id)
            .ok_or_else(|| GraphError::InvalidData("неизвестный источник evidence".into()))?;
        if !proof.insert(ev.id.clone())
            || source.meta.reference != ev.document
            || source.meta.hash != ev.source_hash
            || source.text.get(ev.span.start..ev.span.end) != Some(ev.quote.as_str())
            || crate::source::span(&source.text, ev.span.start..ev.span.end)? != ev.span
        {
            return Err(GraphError::InvalidData(
                "evidence не соответствует исходному диапазону".into(),
            ));
        }
    }
    for node in &data.nodes {
        if !nodes.insert(node.reference.id.clone())
            || node.source_ids.is_empty()
            || node.source_ids.iter().any(|id| !sources.contains_key(id))
            || node.evidence.iter().any(|id| !proof.contains(id))
        {
            return Err(GraphError::InvalidData(
                "неверный узел/источник графа".into(),
            ));
        }
    }
    let mut edges = BTreeSet::new();
    for edge in &data.edges {
        if !edges.insert(edge.id.clone())
            || !nodes.contains(&edge.from)
            || !nodes.contains(&edge.to)
            || edge.evidence.is_empty()
            || edge.evidence.iter().any(|id| !proof.contains(id))
        {
            return Err(GraphError::InvalidData(
                "ребро без разрешимых endpoints/evidence".into(),
            ));
        }
    }
    Ok(())
}

/// Публикует всё поколение атомарно; запоздалый extraction не заменяет текущий граф.
pub fn publish(
    db: &mut Connection,
    data: &Dataset,
    expected: &BTreeMap<String, String>,
) -> Result<(), GraphError> {
    publish_with_errors(db, data, expected, &[])
}

pub fn publish_with_errors(
    db: &mut Connection,
    data: &Dataset,
    expected: &BTreeMap<String, String>,
    errors: &[GraphFileError],
) -> Result<(), GraphError> {
    validate(data)?;
    let tx = db.transaction()?;
    if &document_manifest(&tx)? != expected {
        return Err(GraphError::StaleRef(
            "исходники изменились во время построения".into(),
        ));
    }
    let actual: BTreeMap<_, _> = data
        .sources
        .iter()
        .map(|s| (s.meta.path.clone(), s.meta.hash.clone()))
        .collect();
    let failed: BTreeSet<_> = errors.iter().map(|e| e.path.as_str()).collect();
    let failed_metadata: BTreeSet<_> = data
        .failed_sources
        .iter()
        .map(|s| s.path.as_str())
        .collect();
    if actual.iter().any(|(p, h)| expected.get(p) != Some(h))
        || failed_metadata != failed
        || data
            .failed_sources
            .iter()
            .any(|s| expected.get(&s.path) != Some(&s.hash))
        || expected
            .keys()
            .any(|p| !actual.contains_key(p) && !failed.contains(p.as_str()))
        || failed
            .iter()
            .any(|p| actual.contains_key(*p) || !expected.contains_key(*p))
    {
        return Err(GraphError::InvalidData(
            "граф не покрывает снимок индексированных документов".into(),
        ));
    }
    tx.execute_batch("DELETE FROM graph_edge_evidence; DELETE FROM graph_node_sources; DELETE FROM graph_edges; DELETE FROM graph_evidence; DELETE FROM graph_nodes; DELETE FROM graph_sources; DELETE FROM graph_aux;")?;
    for source in &data.sources {
        tx.execute(
            "INSERT INTO graph_sources VALUES(?1,?2,?3)",
            params![
                source.meta.path,
                source.meta.hash,
                serde_json::to_string(source)?
            ],
        )?;
    }
    for node in &data.nodes {
        tx.execute(
            "INSERT INTO graph_nodes VALUES(?1,?2,?3,?4)",
            params![
                node.reference.id,
                serde_json::to_string(&node.reference.kind)?,
                node.reference.revision,
                serde_json::to_string(node)?
            ],
        )?;
        for id in &node.source_ids {
            let source = data
                .sources
                .iter()
                .find(|s| s.meta.reference.id == *id)
                .unwrap();
            tx.execute(
                "INSERT INTO graph_node_sources VALUES(?1,?2)",
                params![node.reference.id, source.meta.path],
            )?;
        }
    }
    for ev in &data.evidence {
        let source = data
            .sources
            .iter()
            .find(|s| s.meta.reference.id == ev.document.id)
            .unwrap();
        tx.execute(
            "INSERT INTO graph_evidence VALUES(?1,?2,?3)",
            params![ev.id, source.meta.path, serde_json::to_string(ev)?],
        )?;
    }
    for edge in &data.edges {
        tx.execute(
            "INSERT INTO graph_edges VALUES(?1,?2,?3,?4,?5)",
            params![
                edge.id,
                edge.from,
                edge.to,
                serde_json::to_string(&edge.predicate)?,
                serde_json::to_string(edge)?
            ],
        )?;
        for ev in &edge.evidence {
            tx.execute(
                "INSERT INTO graph_edge_evidence VALUES(?1,?2)",
                params![edge.id, ev],
            )?;
        }
    }
    for (key, value) in [
        ("namespace", serde_json::to_string(&data.namespace)?),
        ("docs_root", serde_json::to_string(&data.docs_root)?),
        ("broken_links", serde_json::to_string(&data.broken_links)?),
        ("warnings", serde_json::to_string(&data.warnings)?),
        (
            "similarity_state",
            serde_json::to_string(&data.similarity_state)?,
        ),
        (
            "failed_sources",
            serde_json::to_string(&data.failed_sources)?,
        ),
        (
            "semantic_state",
            serde_json::to_string(&data.semantic_state)?,
        ),
    ] {
        tx.execute("INSERT INTO graph_aux VALUES(?1,?2)", params![key, value])?;
    }
    set_meta(&tx, "snapshot", &data.snapshot)?;
    set_meta(&tx, "manifest", &serde_json::to_string(expected)?)?;
    set_meta(
        &tx,
        "file_errors",
        &serde_json::to_string(&errors.iter().take(100).collect::<Vec<_>>())?,
    )?;
    set_meta(&tx, "file_errors_total", &errors.len().to_string())?;
    set_meta(&tx, "method", METHOD_VERSION)?;
    let mut layers = BTreeMap::new();
    for (name, origin, fingerprint, state) in [
        (
            "structure",
            Origin::Structural,
            "markdown-structure-v1",
            if errors.is_empty() {
                LayerState::Ready
            } else {
                LayerState::Partial
            },
        ),
        (
            "semantic",
            Origin::ExplicitRule,
            METHOD_VERSION,
            data.semantic_state,
        ),
        (
            "similarity",
            Origin::Similarity,
            "cosine-knn-v1:k=5:tau=0.5:limit=1000000",
            data.similarity_state,
        ),
    ] {
        let edges: Vec<_> = data.edges.iter().filter(|e| e.origin == origin).collect();
        let proof: BTreeSet<_> = edges.iter().flat_map(|e| e.evidence.iter()).collect();
        let evidence: Vec<_> = data
            .evidence
            .iter()
            .filter(|e| proof.contains(&e.id))
            .collect();
        let source_revisions = (origin == Origin::Structural).then_some(expected);
        let generation = blake3::hash(&serde_json::to_vec(&(
            fingerprint,
            source_revisions,
            &edges,
            evidence,
            &data.failed_sources,
        ))?)
        .to_hex()
        .to_string();
        layers.insert(
            name,
            GraphLayerStatus {
                state,
                generation: if state == LayerState::NotBuilt {
                    None
                } else {
                    Some(generation)
                },
                fingerprint: fingerprint.into(),
                processed: data.sources.len(),
                total: expected.len(),
                failed: errors.len(),
            },
        );
    }
    set_meta(&tx, "layers", &serde_json::to_string(&layers)?)?;
    set_meta(
        &tx,
        "state",
        if data.semantic_state == LayerState::Ready {
            "ready"
        } else {
            "partial"
        },
    )?;
    set_meta(&tx, "active_job", "")?;
    set_meta(&tx, "error", "")?;
    tx.commit()?;
    Ok(())
}

pub fn status(db: &Connection) -> Result<GraphStatus, GraphError> {
    if db.is_autocommit() {
        let tx = db.unchecked_transaction()?;
        let status = status_snapshot(&tx)?;
        tx.commit()?;
        Ok(status)
    } else {
        status_snapshot(db)
    }
}

fn status_snapshot(db: &Connection) -> Result<GraphStatus, GraphError> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='graph_meta')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(GraphStatus {
            state: LayerState::NotBuilt,
            snapshot: None,
            active_job: None,
            source_count: 0,
            error: None,
            method: METHOD_VERSION.into(),
            progress: None,
            file_errors: vec![],
            file_errors_total: 0,
            layers: BTreeMap::new(),
            paused: false,
        });
    }
    let mut state: LayerState = serde_json::from_str(&format!(
        "\"{}\"",
        meta(db, "state")?.unwrap_or_else(|| "not_built".into())
    ))?;
    let method = meta(db, "method")?.unwrap_or_default();
    if method != METHOD_VERSION && !matches!(state, LayerState::NotBuilt | LayerState::Building) {
        state = LayerState::Stale;
    }
    let nonempty = |s: Option<String>| s.filter(|s| !s.is_empty());
    let snapshot = nonempty(meta(db, "snapshot")?);
    let source_count = db.query_row("SELECT count(*) FROM graph_sources", [], |r| {
        r.get::<_, u32>(0)
    })? as usize;
    let file_errors_total = meta(db, "file_errors_total")?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut layers: BTreeMap<String, GraphLayerStatus> = meta(db, "layers")?
        .map(|v| serde_json::from_str(&v))
        .transpose()?
        .unwrap_or_default();
    // Состояние задания не подменяет состояние сохранённого слоя тех же исходников.
    let saved_manifest: Option<BTreeMap<String, String>> = meta(db, "manifest")?
        .map(|v| serde_json::from_str(&v))
        .transpose()?;
    if method != METHOD_VERSION
        || saved_manifest
            .map(|m| document_manifest(db).map(|current| current != m))
            .transpose()?
            .unwrap_or(false)
    {
        for layer in layers.values_mut() {
            layer.state = LayerState::Stale;
        }
    }
    let mut file_errors: Vec<GraphFileError> = meta(db, "file_errors")?
        .map(|v| serde_json::from_str(&v))
        .transpose()?
        .unwrap_or_default();
    let docs_root: Option<String> = db
        .query_row(
            "SELECT data FROM graph_aux WHERE key='docs_root'",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .map(|v| serde_json::from_str(&v))
        .transpose()?;
    if let Some(root) = docs_root {
        for error in &mut file_errors {
            if let Ok(path) = std::path::Path::new(&error.path).strip_prefix(&root) {
                error.path = path.to_string_lossy().replace('\\', "/");
            }
        }
    }
    Ok(GraphStatus {
        state,
        snapshot,
        active_job: nonempty(meta(db, "active_job")?),
        source_count,
        error: nonempty(meta(db, "error")?),
        method,
        progress: meta(db, "progress")?
            .map(|v| serde_json::from_str(&v))
            .transpose()?,
        file_errors,
        file_errors_total,
        layers,
        paused: meta(db, "paused")?.as_deref() == Some("true"),
    })
}

pub fn set_paused(db: &Connection, paused: bool) -> Result<(), GraphError> {
    set_meta(db, "paused", if paused { "true" } else { "false" })
}

pub fn set_progress(db: &Connection, progress: &GraphProgress) -> Result<(), GraphError> {
    set_meta(db, "progress", &serde_json::to_string(progress)?)
}

pub fn set_state(
    db: &Connection,
    state: LayerState,
    job: Option<&str>,
    error: Option<&str>,
) -> Result<(), GraphError> {
    let state = serde_json::to_string(&state)?;
    let tx = db.unchecked_transaction()?;
    set_meta(&tx, "state", state.trim_matches('"'))?;
    set_meta(&tx, "active_job", job.unwrap_or(""))?;
    set_meta(&tx, "error", error.unwrap_or(""))?;
    tx.commit()?;
    Ok(())
}

pub fn load(db: &Connection) -> Result<Dataset, GraphError> {
    let tx = db.unchecked_transaction()?;
    let state = status(&tx)?;
    if state.method != METHOD_VERSION {
        return Err(GraphError::NotReady("method_changed".into()));
    }
    if !matches!(
        state.state,
        LayerState::Ready
            | LayerState::Partial
            | LayerState::Building
            | LayerState::Cancelled
            | LayerState::Failed
    ) || state.snapshot.is_none()
    {
        return Err(GraphError::NotReady(format!("{:?}", state.state)));
    }
    let rows = |table: &str| -> Result<Vec<String>, GraphError> {
        let sql = match table {
            "sources" => "SELECT data FROM graph_sources ORDER BY path",
            "nodes" => "SELECT data FROM graph_nodes ORDER BY id",
            "edges" => "SELECT data FROM graph_edges ORDER BY id",
            "evidence" => "SELECT data FROM graph_evidence ORDER BY id",
            _ => unreachable!(),
        };
        let mut stmt = tx.prepare(sql)?;
        Ok(stmt
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?)
    };
    let parse = |key: &str| -> Result<String, GraphError> {
        Ok(
            tx.query_row("SELECT data FROM graph_aux WHERE key=?1", [key], |r| {
                r.get(0)
            })?,
        )
    };
    let mut data = Dataset {
        namespace: serde_json::from_str(&parse("namespace")?)?,
        docs_root: serde_json::from_str(&parse("docs_root")?)?,
        snapshot: state.snapshot.unwrap(),
        layers: state.layers,
        sources: rows("sources")?
            .iter()
            .map(|s| serde_json::from_str(s))
            .collect::<Result<_, _>>()?,
        failed_sources: tx
            .query_row(
                "SELECT data FROM graph_aux WHERE key='failed_sources'",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map(|s| serde_json::from_str(&s))
            .transpose()?
            .unwrap_or_default(),
        nodes: rows("nodes")?
            .iter()
            .map(|s| serde_json::from_str(s))
            .collect::<Result<_, _>>()?,
        edges: rows("edges")?
            .iter()
            .map(|s| serde_json::from_str(s))
            .collect::<Result<_, _>>()?,
        evidence: rows("evidence")?
            .iter()
            .map(|s| serde_json::from_str(s))
            .collect::<Result<_, _>>()?,
        broken_links: serde_json::from_str(&parse("broken_links")?)?,
        warnings: serde_json::from_str(&parse("warnings")?)?,
        semantic_state: serde_json::from_str(&parse("semantic_state")?)?,
        similarity_state: tx
            .query_row(
                "SELECT data FROM graph_aux WHERE key='similarity_state'",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map(|s| serde_json::from_str(&s))
            .transpose()?
            .unwrap_or(LayerState::Partial),
    };
    let expected: BTreeMap<String, String> = if let Some(manifest) = meta(&tx, "manifest")? {
        serde_json::from_str(&manifest)?
    } else {
        data.sources
            .iter()
            .map(|s| (s.meta.path.clone(), s.meta.hash.clone()))
            .collect()
    };
    if document_manifest(&tx)? != expected {
        return Err(GraphError::NotReady("stale".into()));
    }
    if matches!(
        state.state,
        LayerState::Building | LayerState::Cancelled | LayerState::Failed
    ) {
        data.warnings.push(format!("Состояние фонового задания {:?}; возвращён предыдущий согласованный снимок тех же исходников",state.state));
    }
    tx.commit()?;
    validate(&data)?;
    Ok(data)
}
