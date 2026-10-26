use super::*;
use crate::graph::similarity::VectorNode;
use crate::graph::similarity_job::{Calculation, METHOD};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct GraphBuildState {
    active: Option<Active>,
}

pub(super) struct Active {
    prepared: Prepared,
    calculation: Calculation,
    revision: String,
    bytes: usize,
}

pub(super) fn ensure_revision(index: &Index) -> Result<(), GraphError> {
    index.conn().execute_batch(
        "INSERT OR IGNORE INTO graph_meta VALUES('knn_input_epoch',hex(randomblob(16)));
        INSERT OR IGNORE INTO graph_meta VALUES('knn_input_revision','0');",
    )?;
    for table in ["documents", "chunks", "vectors", "meta"] {
        for event in ["INSERT", "UPDATE", "DELETE"] {
            let row = if event == "DELETE" { "OLD" } else { "NEW" };
            let filter = if table == "meta" {
                format!(
                    "WHEN {row}.key LIKE 'model_%' OR {row}.key IN ('docs_root','chunk_min_chars','chunk_max_chars')"
                )
            } else {
                String::new()
            };
            index.conn().execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS knn_inputs_{table}_{event} AFTER {event} ON {table} {filter} BEGIN
                UPDATE graph_meta SET value=CAST(value AS INTEGER)+1 WHERE key='knn_input_revision'; END;"))?;
        }
    }
    Ok(())
}

fn revision(index: &Index) -> Result<String, GraphError> {
    Ok(index.conn().query_row("SELECT (SELECT value FROM graph_meta WHERE key='knn_input_epoch') || ':' || (SELECT value FROM graph_meta WHERE key='knn_input_revision')", [], |r| r.get(0))?)
}

impl GraphBuildState {
    pub(super) fn take_current(
        &mut self,
        project: &Path,
        index: &Index,
        layer: bars_graph::BuildLayer,
    ) -> Result<Option<Active>, GraphError> {
        let Some(active) = self.active.take() else {
            return Ok(None);
        };
        let namespace = project
            .canonicalize()
            .map_err(|e| GraphError::InvalidData(e.to_string()))?
            .to_string_lossy()
            .replace('\\', "/");
        Ok((active.prepared.layer == layer
            && active.prepared.data.namespace == namespace
            && active.revision == revision(index)?)
        .then_some(active))
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct Prepared {
    pub key: String,
    pub layer: bars_graph::BuildLayer,
    pub data: bars_graph::Dataset,
    pub vectors: Vec<(String, String)>,
    pub expected: BTreeMap<String, String>,
    pub errors: Vec<bars_graph::GraphFileError>,
}

fn read<T: DeserializeOwned>(index: &Index, key: &str) -> Result<Option<T>, GraphError> {
    let value: Option<String> = index
        .conn()
        .query_row("SELECT value FROM graph_meta WHERE key=?1", [key], |r| {
            r.get(0)
        })
        .optional()?;
    // Недописанный/несовместимый checkpoint никогда не становится основанием результата.
    Ok(value.and_then(|s| serde_json::from_str(&s).ok()))
}

pub(super) fn save(index: &Index, key: &str, value: &impl Serialize) -> Result<(), GraphError> {
    index.conn().execute("INSERT INTO graph_meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [key, &serde_json::to_string(value)?])?;
    Ok(())
}

fn config(index: &Index) -> Result<String, GraphError> {
    let mut stmt = index.conn().prepare("SELECT key,value FROM meta WHERE key LIKE 'model_%' OR key IN ('docs_root','chunk_min_chars','chunk_max_chars') ORDER BY key")?;
    let meta = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!(
        "{}:{METHOD}:{}",
        bars_graph::METHOD_VERSION,
        serde_json::to_string(&meta)?
    ))
}

pub(super) fn input_key(index: &Index) -> Result<String, GraphError> {
    let mut hash = blake3::Hasher::new();
    hash.update(config(index)?.as_bytes());
    hash.update(&serde_json::to_vec(&bars_graph::store::document_manifest(
        index.conn(),
    )?)?);
    // BLOB читается последовательным проходом; случайные lookup перечитывают overflow-страницы.
    let mut stmt = index.conn().prepare("SELECT v.embed_hash,v.vector FROM vectors v WHERE EXISTS(SELECT 1 FROM chunks c WHERE c.embed_hash=v.embed_hash) ORDER BY v.embed_hash")?;
    for row in stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<Vec<u8>>>(1)?))
    })? {
        let (id, bytes) = row?;
        hash.update(id.as_bytes());
        if let Some(bytes) = bytes {
            hash.update(&bytes);
        }
    }
    let mut missing = index.conn().prepare("SELECT DISTINCT c.embed_hash FROM chunks c LEFT JOIN vectors v ON v.embed_hash=c.embed_hash WHERE v.embed_hash IS NULL ORDER BY c.embed_hash")?;
    for id in missing.query_map([], |r| r.get::<_, String>(0))? {
        hash.update(id?.as_bytes());
    }
    Ok(hash.finalize().to_hex().to_string())
}

pub(super) fn load(
    project: &Path,
    index: &Index,
    layer: bars_graph::BuildLayer,
) -> Result<Option<Prepared>, GraphError> {
    let prepared: Option<Prepared> = read(index, "knn_prepared")?;
    let Some(prepared) = prepared else {
        return Ok(None);
    };
    let namespace = project
        .canonicalize()
        .map_err(|e| GraphError::InvalidData(e.to_string()))?
        .to_string_lossy()
        .replace('\\', "/");
    if prepared.layer == layer
        && prepared.data.namespace == namespace
        && prepared.key == input_key(index)?
    {
        Ok(Some(prepared))
    } else {
        index.conn().execute(
            "DELETE FROM graph_meta WHERE key IN ('knn_prepared','knn_checkpoint')",
            [],
        )?;
        Ok(None)
    }
}

pub(super) fn run(
    index: &mut Index,
    prepared: Prepared,
    state: &mut GraphBuildState,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Option<bars_graph::store::GraphStatus>, GraphError> {
    let active = prepare_calculation(index, prepared)?;
    run_active(index, active, state, &mut cancelled, Duration::from_secs(2))
}

#[cfg(test)]
fn run_quantum(
    index: &mut Index,
    prepared: Prepared,
    mut cancelled: impl FnMut() -> bool,
    quantum: Duration,
) -> Result<Option<bars_graph::store::GraphStatus>, GraphError> {
    ensure_revision(index)?;
    let active = prepare_calculation(index, prepared)?;
    run_active(
        index,
        active,
        &mut GraphBuildState::default(),
        &mut cancelled,
        quantum,
    )
}

fn prepare_calculation(index: &Index, prepared: Prepared) -> Result<Active, GraphError> {
    let started = Instant::now();
    let vectors = prepared
        .vectors
        .iter()
        .map(|(id, hash)| {
            let vector = index
                .vector(hash)
                .map_err(|e| GraphError::InvalidData(e.to_string()))?
                .ok_or_else(|| GraphError::StaleRef(id.clone()))?;
            Ok(VectorNode {
                id: id.clone(),
                vector,
            })
        })
        .collect::<Result<Vec<_>, GraphError>>()?;
    let mut calculation = Calculation::from_vectors(
        vectors,
        &config(index)?,
        read(index, "knn_checkpoint")?,
        read(index, "knn_complete")?,
    )
    .map_err(|e| GraphError::InvalidData(e.to_string()))?;
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len());
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = Count(0);
    serde_json::to_writer(&mut count, &prepared)?;
    let bytes = calculation
        .estimated_bytes()
        .saturating_add(count.0.saturating_mul(2));
    calculation.checkpoint.elapsed_ms += started.elapsed().as_millis() as u64;
    Ok(Active {
        prepared,
        calculation,
        revision: revision(index)?,
        bytes,
    })
}

pub(super) fn run_active(
    index: &mut Index,
    active: Active,
    state: &mut GraphBuildState,
    mut cancelled: impl FnMut() -> bool,
    quantum: Duration,
) -> Result<Option<bars_graph::store::GraphStatus>, GraphError> {
    let started = Instant::now();
    let Active {
        mut prepared,
        mut calculation,
        revision,
        bytes,
    } = active;
    let vector_count = calculation.checkpoint.ids.len();
    let complete = calculation.advance(quantum, &mut cancelled);
    calculation.checkpoint.elapsed_ms += started.elapsed().as_millis() as u64;
    save(index, "knn_checkpoint", &calculation.checkpoint)?;
    let c = &calculation.checkpoint;
    bars_graph::store::set_progress(
        index.conn(),
        &bars_graph::GraphProgress {
            phase: "similarity_pairs".into(),
            processed: c.compared as usize,
            total: Some(c.total as usize),
            elapsed_ms: c.elapsed_ms,
        },
    )?;
    if cancelled() {
        return Err(GraphError::Cancelled);
    }
    if !complete {
        // Большие задания восстанавливаются с диска; удержание одного задания ограничено.
        if bytes <= 256 * 1024 * 1024 {
            state.active = Some(Active {
                prepared,
                calculation,
                revision,
                bytes,
            });
        }
        return Ok(None);
    }
    if input_key(index)? != prepared.key {
        return Err(GraphError::StaleRef("входы kNN изменились".into()));
    }
    let total = index
        .conn()
        .query_row("SELECT count(*) FROM chunks", [], |r| r.get::<_, u32>(0))?
        as usize;
    let missing = total.saturating_sub(vector_count);
    let data = &mut prepared.data;
    data.similarity_state = if missing == 0 && prepared.errors.is_empty() {
        LayerState::Ready
    } else {
        LayerState::Partial
    };
    data.layers.insert(
        "similarity".into(),
        bars_graph::GraphLayerStatus {
            freshness: bars_graph::Freshness::Current,
            completeness: if data.similarity_state == LayerState::Ready {
                bars_graph::Completeness::Complete
            } else {
                bars_graph::Completeness::Partial
            },
            availability: bars_graph::Availability::Enabled,
            stale_reason: None,
            coverage: bars_graph::LayerCoverage {
                unit: "chunks".into(),
                processed: vector_count,
                total,
                missing,
                comparisons: c.compared,
                reason: (missing > 0)
                    .then(|| "У части чанков нет пригодного вектора или файл исключён".into()),
            },
            source_revisions: prepared.expected.clone(),
            state: data.similarity_state,
            generation: None,
            fingerprint: METHOD.into(),
            processed: 0,
            total: 0,
            failed: 0,
        },
    );
    let nodes: BTreeMap<_, _> = data
        .nodes
        .iter()
        .map(|n| (n.reference.id.as_str(), n))
        .collect();
    for (from, neighbors) in calculation.neighbors() {
        for neighbor in neighbors {
            let a = nodes
                .get(from.as_str())
                .ok_or_else(|| GraphError::InvalidData("Нет исходного узла kNN".into()))?;
            let b = nodes
                .get(neighbor.id.as_str())
                .ok_or_else(|| GraphError::InvalidData("Нет целевого узла kNN".into()))?;
            let mut evidence = a.evidence.clone();
            evidence.extend(b.evidence.iter().cloned());
            evidence.sort();
            evidence.dedup();
            if evidence.is_empty() {
                continue;
            }
            data.edges.push(bars_graph::Edge {
                id: blake3::hash(format!("similar:{from}:{}", neighbor.id).as_bytes())
                    .to_hex()
                    .to_string(),
                from: from.clone(),
                to: neighbor.id,
                predicate: Predicate::SimilarTo,
                polarity: Polarity::Positive,
                qualifiers: Default::default(),
                origin: Origin::Similarity,
                evidence,
                score: Some(neighbor.cosine),
            });
        }
    }
    let status = super::finish_graph(
        index,
        prepared.data,
        prepared.expected,
        prepared.errors,
        cancelled,
    )?;
    save(index, "knn_complete", &calculation.checkpoint)?;
    index.conn().execute(
        "DELETE FROM graph_meta WHERE key IN ('knn_prepared','knn_checkpoint')",
        [],
    )?;
    Ok(Some(status))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexing::chunking::{ChunkingParams, chunk_document};
    use crate::storage::{DocumentRecord, Journal, Layout};

    fn fixture() -> (tempfile::TempDir, Index) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("docs")).unwrap();
        let mut index = Index::open(&dir.path().join("index.sqlite"), Journal::Wal).unwrap();
        let chunking = ChunkingParams::default();
        index
            .write_layout(&Layout {
                chunking,
                docs_root: "docs".into(),
            })
            .unwrap();
        for i in 0..20 {
            let path = format!("docs/{i}.md");
            let text = format!("# Документ {i}\n\nТ-1 зависит от Т-2. Текст документа {i}.");
            std::fs::write(dir.path().join(&path), &text).unwrap();
            let mapped = chunk_document(&path, &text, &chunking);
            let vector = vec![1., (i % 3) as f32];
            let vectors: Vec<_> = mapped
                .chunks
                .iter()
                .map(|c| (c.embed_hash.as_str(), vector.as_slice()))
                .collect();
            index
                .replace_document(
                    &DocumentRecord {
                        path: &path,
                        module: ".",
                        title: &mapped.title,
                        size: text.len() as u64,
                        modified_ns: None,
                        hash: &blake3::hash(text.as_bytes()).to_hex(),
                        chunks: &mapped.chunks,
                    },
                    &vectors,
                )
                .unwrap();
        }
        (dir, index)
    }

    #[test]
    fn similarity_job_sqlite_checkpoint_preserves_snapshot_and_resumes_without_markdown() {
        let (dir, mut index) = fixture();
        let original = crate::graph::build_index_graph(dir.path(), &mut index, || false)
            .unwrap()
            .snapshot;
        let database = rusqlite::Connection::open(dir.path().join("index.sqlite")).unwrap();
        let cancelled = || {
            database
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM graph_meta WHERE key='knn_prepared')",
                    [],
                    |r| r.get::<_, bool>(0),
                )
                .unwrap()
        };
        assert!(matches!(
            crate::graph::build_index_graph_step(
                dir.path(),
                &mut index,
                bars_graph::BuildLayer::Similarity,
                cancelled
            ),
            Err(GraphError::Cancelled)
        ));
        assert_eq!(
            bars_graph::store::load_previous(index.conn())
                .unwrap()
                .snapshot,
            original.as_ref().unwrap().as_str()
        );
        let prepared = load(dir.path(), &index, bars_graph::BuildLayer::Similarity)
            .unwrap()
            .unwrap();
        assert!(
            run_quantum(&mut index, prepared, || false, Duration::ZERO)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            bars_graph::store::status(index.conn())
                .unwrap()
                .progress
                .unwrap()
                .phase,
            "similarity_pairs"
        );
        // Продолжение читает подготовленный набор: повторное открытие исходника не требуется.
        std::fs::rename(dir.path().join("docs"), dir.path().join("hidden-docs")).unwrap();
        drop(index);
        let mut index = Index::open(&dir.path().join("index.sqlite"), Journal::Wal).unwrap();
        let prepared = load(dir.path(), &index, bars_graph::BuildLayer::Similarity)
            .unwrap()
            .unwrap();
        let status = run_quantum(&mut index, prepared, || false, Duration::from_secs(30))
            .unwrap()
            .unwrap();
        assert_eq!(status.snapshot, original);
        assert_eq!(
            status.layers["similarity"].completeness,
            bars_graph::Completeness::Complete
        );
        assert!(read::<Prepared>(&index, "knn_prepared").unwrap().is_none());
    }

    #[test]
    fn similarity_job_changed_manifest_or_vector_invalidates_preparation() {
        let (dir, mut index) = fixture();
        let db = rusqlite::Connection::open(dir.path().join("index.sqlite")).unwrap();
        let _ = crate::graph::build_index_graph_step(
            dir.path(),
            &mut index,
            bars_graph::BuildLayer::All,
            || {
                db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM graph_meta WHERE key='knn_prepared')",
                    [],
                    |r| r.get::<_, bool>(0),
                )
                .unwrap()
            },
        );
        assert!(
            load(dir.path(), &index, bars_graph::BuildLayer::All)
                .unwrap()
                .is_some()
        );
        index
            .conn()
            .execute("UPDATE vectors SET vector=zeroblob(length(vector))", [])
            .unwrap();
        assert!(
            load(dir.path(), &index, bars_graph::BuildLayer::All)
                .unwrap()
                .is_none()
        );
        assert!(
            read::<crate::graph::similarity_job::Checkpoint>(&index, "knn_checkpoint")
                .unwrap()
                .is_none()
        );
        save(&index, "knn_checkpoint", &"truncated checkpoint").unwrap();
        assert!(
            read::<crate::graph::similarity_job::Checkpoint>(&index, "knn_checkpoint")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn active_quantum_reuses_vectors_but_invalidates_same_key_vector_write() {
        let (dir, mut index) = fixture();
        let db = rusqlite::Connection::open(dir.path().join("index.sqlite")).unwrap();
        let _ = crate::graph::build_index_graph_step(
            dir.path(),
            &mut index,
            bars_graph::BuildLayer::All,
            || {
                db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM graph_meta WHERE key='knn_prepared')",
                    [],
                    |r| r.get::<_, bool>(0),
                )
                .unwrap()
            },
        );
        let prepared = load(dir.path(), &index, bars_graph::BuildLayer::All)
            .unwrap()
            .unwrap();
        let active = prepare_calculation(&index, prepared).unwrap();
        let vectors = active.calculation.vectors_address();
        let mut state = GraphBuildState::default();
        assert!(
            run_active(&mut index, active, &mut state, || false, Duration::ZERO)
                .unwrap()
                .is_none()
        );
        let active = state
            .take_current(dir.path(), &index, bars_graph::BuildLayer::All)
            .unwrap()
            .unwrap();
        assert_eq!(vectors, active.calculation.vectors_address());
        let expected = active.prepared.key.clone();
        assert!(
            run_active(&mut index, active, &mut state, || false, Duration::ZERO)
                .unwrap()
                .is_none()
        );
        index
            .conn()
            .execute("UPDATE vectors SET vector=zeroblob(length(vector))", [])
            .unwrap();
        assert!(
            state
                .take_current(dir.path(), &index, bars_graph::BuildLayer::All)
                .unwrap()
                .is_none()
        );
        assert_ne!(expected, input_key(&index).unwrap());
    }

    #[test]
    fn active_quantum_and_disk_resume_produce_identical_neighbors() {
        let (dir, mut index) = fixture();
        let db = rusqlite::Connection::open(dir.path().join("index.sqlite")).unwrap();
        let _ = crate::graph::build_index_graph_step(
            dir.path(),
            &mut index,
            bars_graph::BuildLayer::All,
            || {
                db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM graph_meta WHERE key='knn_prepared')",
                    [],
                    |r| r.get::<_, bool>(0),
                )
                .unwrap()
            },
        );
        let prepared = load(dir.path(), &index, bars_graph::BuildLayer::All)
            .unwrap()
            .unwrap();
        let disk = prepare_calculation(
            &index,
            load(dir.path(), &index, bars_graph::BuildLayer::All)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        let mut state = GraphBuildState::default();
        let active = prepare_calculation(&index, prepared).unwrap();
        run_active(&mut index, active, &mut state, || false, Duration::ZERO).unwrap();
        let mut active = state
            .take_current(dir.path(), &index, bars_graph::BuildLayer::All)
            .unwrap()
            .unwrap();
        assert!(active.calculation.advance(Duration::from_secs(5), || false));
        let mut restored = disk.calculation;
        assert!(restored.advance(Duration::from_secs(5), || false));
        assert_eq!(
            serde_json::to_value(active.calculation.neighbors()).unwrap(),
            serde_json::to_value(restored.neighbors()).unwrap()
        );
        assert_eq!(
            active.calculation.checkpoint.compared,
            restored.checkpoint.compared
        );
    }
}
