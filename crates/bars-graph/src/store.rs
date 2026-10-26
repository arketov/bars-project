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
        "INSERT INTO graph_meta VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value WHERE value<>excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Добавочная схема графа; meta/schema_version embeddings не изменяется.
pub fn ensure_schema(db: &mut Connection) -> Result<(), GraphError> {
    db.pragma_update(None, "foreign_keys", "ON")?;
    let tx = db.transaction()?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS graph_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL) WITHOUT ROWID;")?;
    if let Some(version) = meta(&tx, "schema_version")?
        && version.parse::<u32>().ok() != Some(GRAPH_SCHEMA_VERSION)
    {
        return Err(GraphError::InvalidData(format!(
            "схема графа {version} не поддерживается"
        )));
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
    // Счётчик живёт в файле: кэш не удерживает SQLite-handle, мешающий подмене на Windows.
    tx.execute_batch(
        "INSERT OR IGNORE INTO graph_meta VALUES('read_epoch',hex(randomblob(16)));
        INSERT OR IGNORE INTO graph_meta VALUES('read_revision','0');",
    )?;
    for table in [
        "graph_meta",
        "graph_sources",
        "graph_nodes",
        "graph_edges",
        "graph_evidence",
        "graph_aux",
        "documents",
        "chunks",
        "vectors",
        "meta",
    ] {
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [table],
            |r| r.get(0),
        )?;
        if !exists {
            continue;
        }
        for event in ["INSERT", "UPDATE", "DELETE"] {
            let row = if event == "DELETE" { "OLD" } else { "NEW" };
            let filter = if table == "graph_meta" {
                format!(
                    "WHEN {row}.key NOT IN ('read_revision','read_epoch','knn_prepared','knn_checkpoint','knn_complete','knn_input_revision','knn_input_epoch')"
                )
            } else {
                String::new()
            };
            tx.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS graph_read_{table}_{event} AFTER {event} ON {table} {filter} BEGIN
                UPDATE graph_meta SET value=CAST(value AS INTEGER)+1 WHERE key='read_revision'; END;"))?;
        }
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

// Модель и доступность векторов меняют близость даже без правки Markdown.
fn similarity_inputs(db: &Connection) -> Result<String, GraphError> {
    let mut stmt =
        db.prepare("SELECT key,value FROM meta WHERE key LIKE 'model_%' ORDER BY key")?;
    let model = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let vectors_exist: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='vectors')",
        [],
        |r| r.get(0),
    )?;
    let vectors: Vec<String> = if vectors_exist {
        let mut stmt = db.prepare("SELECT DISTINCT v.embed_hash FROM vectors v CROSS JOIN chunks c ON c.embed_hash=v.embed_hash ORDER BY v.embed_hash")?;
        stmt.query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?
    } else {
        vec![]
    };
    Ok(blake3::hash(&serde_json::to_vec(&(model, vectors))?)
        .to_hex()
        .to_string())
}

pub fn validate(data: &Dataset) -> Result<(), GraphError> {
    validate_until(data, None)
}

fn check_deadline(deadline: Option<std::time::Instant>) -> Result<(), GraphError> {
    if deadline.is_some_and(|end| std::time::Instant::now() >= end) {
        return Err(GraphError::TimedOut);
    }
    Ok(())
}

fn validate_until(data: &Dataset, deadline: Option<std::time::Instant>) -> Result<(), GraphError> {
    let mut sources = BTreeMap::new();
    let mut nodes = BTreeSet::new();
    let mut proof = BTreeSet::new();
    for source in &data.sources {
        check_deadline(deadline)?;
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
        check_deadline(deadline)?;
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
        check_deadline(deadline)?;
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
        check_deadline(deadline)?;
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
    set_meta(&tx, "similarity_inputs", &similarity_inputs(&tx)?)?;
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
            data.layers
                .get("similarity")
                .map(|l| l.fingerprint.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("cosine-knn-v1:k=5:tau=0.5:limit=1000000"),
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
                stale_reason: None,
                freshness: if state == LayerState::NotBuilt {
                    Freshness::NotBuilt
                } else {
                    Freshness::Current
                },
                completeness: if state == LayerState::NotBuilt {
                    Completeness::Unknown
                } else if !errors.is_empty() {
                    Completeness::Partial
                } else if name == "semantic" || state == LayerState::NotBuilt {
                    Completeness::Unknown
                } else if state == LayerState::Ready {
                    Completeness::Complete
                } else {
                    Completeness::Partial
                },
                availability: Availability::Enabled,
                coverage: data
                    .layers
                    .get(name)
                    .map(|l| l.coverage.clone())
                    .unwrap_or_else(|| LayerCoverage {
                        unit: if name == "similarity" {
                            "chunks"
                        } else {
                            "documents"
                        }
                        .into(),
                        processed: if name == "similarity" || state == LayerState::NotBuilt {
                            0
                        } else {
                            data.sources.len()
                        },
                        total: if name == "similarity" {
                            data.nodes
                                .iter()
                                .filter(|n| n.reference.kind == Kind::Chunk)
                                .count()
                        } else {
                            expected.len()
                        },
                        missing: if name == "similarity" {
                            0
                        } else {
                            errors.len()
                        },
                        comparisons: 0,
                        reason: (name == "similarity")
                            .then(|| "Покрытие расчёта не сохранено".into()),
                    }),
                source_revisions: expected.clone(),
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
    set_meta(&tx, "job_state", "\"idle\"")?;
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
            freshness: Freshness::NotBuilt,
            completeness: Completeness::Unknown,
            availability: Availability::Enabled,
            job_state: JobState::Idle,
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
    let stale = state == LayerState::Stale
        || method != METHOD_VERSION
        || saved_manifest
            .as_ref()
            .map(|m| document_manifest(db).map(|current| &current != m))
            .transpose()?
            .unwrap_or(false);
    let similarity_changed = match meta(db, "similarity_inputs")? {
        Some(saved) => similarity_inputs(db)? != saved,
        None => true,
    };
    for (name, layer) in &mut layers {
        // Старые сохранённые поколения: полнота смысла неизвестна даже после прохода.
        if layer.coverage.unit.is_empty() {
            layer.availability = if layer.state == LayerState::Disabled {
                Availability::Disabled
            } else {
                Availability::Enabled
            };
            layer.freshness = if layer.generation.is_some() {
                Freshness::Current
            } else {
                Freshness::NotBuilt
            };
            layer.completeness = if layer.failed > 0 {
                Completeness::Partial
            } else if name == "semantic" || layer.generation.is_none() {
                Completeness::Unknown
            } else if layer.state == LayerState::Ready {
                Completeness::Complete
            } else {
                Completeness::Partial
            };
            layer.source_revisions = saved_manifest.clone().unwrap_or_default();
            layer.coverage = LayerCoverage {
                unit: if name == "similarity" {
                    "chunks"
                } else {
                    "documents"
                }
                .into(),
                processed: if name == "similarity" {
                    0
                } else {
                    layer.processed
                },
                total: if name == "similarity" { 0 } else { layer.total },
                missing: layer.failed,
                comparisons: 0,
                reason: (name == "similarity")
                    .then(|| "Покрытие прежнего расчёта неизвестно".into()),
            };
        }
        if stale && layer.availability == Availability::Enabled && layer.generation.is_some() {
            layer.stale_reason = Some(
                if method != METHOD_VERSION {
                    "Изменился метод построения"
                } else {
                    "Изменились исходные ревизии или граф ожидает явного обновления"
                }
                .into(),
            );
            layer.state = LayerState::Stale;
            layer.freshness = Freshness::Stale;
        }
        if name == "similarity" && layer.generation.is_some() && similarity_changed {
            layer.freshness = Freshness::Stale;
            layer.state = LayerState::Stale;
            layer.stale_reason = Some("Изменилась модель или доступность векторов; у прежнего графа fingerprint векторов может отсутствовать".into());
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
    let mut result = GraphStatus {
        freshness: Freshness::NotBuilt,
        completeness: Completeness::Unknown,
        availability: Availability::Enabled,
        job_state: meta(db, "job_state")?
            .map(|v| serde_json::from_str(&v))
            .transpose()?
            .unwrap_or(match state {
                LayerState::Building => JobState::Running,
                LayerState::Cancelled => JobState::Stopped,
                LayerState::Failed => JobState::Failed,
                _ => JobState::Idle,
            }),
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
    };
    result.derive_data_state();
    Ok(result)
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
    let tx = db.unchecked_transaction()?;
    let job_state = match state {
        LayerState::Building => Some(JobState::Running),
        LayerState::Cancelled => Some(JobState::Stopped),
        LayerState::Failed => Some(JobState::Failed),
        _ => None,
    };
    if let Some(job_state) = job_state {
        set_meta(&tx, "job_state", &serde_json::to_string(&job_state)?)?;
    } else {
        set_meta(
            &tx,
            "state",
            serde_json::to_string(&state)?.trim_matches('"'),
        )?;
    }
    set_meta(&tx, "active_job", job.unwrap_or(""))?;
    set_meta(&tx, "error", error.unwrap_or(""))?;
    tx.commit()?;
    Ok(())
}

pub fn load(db: &Connection) -> Result<Dataset, GraphError> {
    load_snapshot(db, false, None, &mut 0)
}

/// Последнее целое поколение; устаревшие данные явно помечены и не годятся для mix.
pub fn load_previous(db: &Connection) -> Result<Dataset, GraphError> {
    load_snapshot(db, true, None, &mut 0)
}

/// Целый снимок или ошибка дедлайна; частично загруженный Dataset не публикуется.
pub fn load_previous_until(
    db: &Connection,
    deadline: std::time::Instant,
) -> Result<Dataset, GraphError> {
    load_previous_sized_until(db, deadline).map(|(data, _)| data)
}

/// Размер JSON учитывается при чтении строк, без повторной сериализации Dataset.
pub fn load_previous_sized_until(
    db: &Connection,
    deadline: std::time::Instant,
) -> Result<(Dataset, usize), GraphError> {
    check_deadline(Some(deadline))?;
    db.progress_handler(1000, Some(move || std::time::Instant::now() >= deadline))?;
    let mut bytes = 0usize;
    let result = load_snapshot(db, true, Some(deadline), &mut bytes);
    db.progress_handler(0, None::<fn() -> bool>)?;
    check_deadline(Some(deadline))?;
    result.map(|data| (data, bytes.saturating_mul(2)))
}

fn load_rows<T: serde::de::DeserializeOwned>(
    db: &Connection,
    sql: &str,
    deadline: Option<std::time::Instant>,
    bytes: &mut usize,
) -> Result<Vec<T>, GraphError> {
    let mut statement = db.prepare(sql)?;
    let mut rows = statement.query([])?;
    let mut values = Vec::new();
    while let Some(row) = rows.next()? {
        check_deadline(deadline)?;
        let text = row
            .get_ref(0)?
            .as_str()
            .map_err(|e| GraphError::InvalidData(e.to_string()))?;
        *bytes = bytes.saturating_add(text.len());
        values.push(serde_json::from_str(text)?);
    }
    Ok(values)
}

fn load_snapshot(
    db: &Connection,
    allow_stale: bool,
    deadline: Option<std::time::Instant>,
    bytes: &mut usize,
) -> Result<Dataset, GraphError> {
    check_deadline(deadline)?;
    let tx = db.unchecked_transaction()?;
    let state = status(&tx)?;
    if meta(&tx, "schema_version")?.and_then(|v| v.parse::<u32>().ok())
        != Some(GRAPH_SCHEMA_VERSION)
    {
        return Err(GraphError::NotReady(
            "Несовместимая схема графа; обновите граф".into(),
        ));
    }
    if state.method != METHOD_VERSION {
        return Err(GraphError::NotReady(
            "изменился способ построения, граф нужно перестроить".into(),
        ));
    }
    if state.snapshot.is_none()
        || state
            .layers
            .get("structure")
            .is_none_or(|l| l.generation.is_none() || l.availability == Availability::Disabled)
        || (!allow_stale && state.freshness == Freshness::Stale)
    {
        return Err(GraphError::NotReady(state.state.not_ready_reason().into()));
    }
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
        sources: load_rows(
            &tx,
            "SELECT data FROM graph_sources ORDER BY path",
            deadline,
            bytes,
        )?,
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
        nodes: load_rows(
            &tx,
            "SELECT data FROM graph_nodes ORDER BY id",
            deadline,
            bytes,
        )?,
        edges: load_rows(
            &tx,
            "SELECT data FROM graph_edges ORDER BY id",
            deadline,
            bytes,
        )?,
        evidence: load_rows(
            &tx,
            "SELECT data FROM graph_evidence ORDER BY id",
            deadline,
            bytes,
        )?,
        broken_links: serde_json::from_str(&parse("broken_links")?)?,
        warnings: serde_json::from_str::<Vec<String>>(&parse("warnings")?)?
            .into_iter()
            .map(humanize_warning)
            .collect(),
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
    if document_manifest(&tx)? != expected && !allow_stale {
        return Err(GraphError::NotReady(
            LayerState::Stale.not_ready_reason().into(),
        ));
    }
    if state.freshness == Freshness::Stale {
        data.warnings.push(
                "Граф устарел · Показан предыдущий снимок; изменились входы графа или требуется его обновление"
                .into(),
        );
    }
    let job_layer = match state.job_state {
        JobState::Running => Some(LayerState::Building),
        JobState::Stopped => Some(LayerState::Cancelled),
        JobState::Failed => Some(LayerState::Failed),
        _ => None,
    };
    if let Some(job_layer) = job_layer {
        data.warnings.push(if state.freshness == Freshness::Stale {
            "Показан предыдущий снимок; состояние задания не меняет свежесть данных".into()
        } else {
            job_layer.background_job_warning().into()
        });
    }
    tx.commit()?;
    validate_until(&data, deadline)?;
    Ok(data)
}
