use super::*;
use crate::graph::similarity::VectorNode;
use crate::graph::similarity_job::{Calculation, METHOD};
use bars_graph::store::PayloadKey;
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
    bars_graph::store::revision_stamp(index.conn())?
        .ok_or_else(|| GraphError::InvalidData("требуется схема графа v2".into()))?;
    Ok(())
}
fn revision(index: &Index) -> Result<String, GraphError> {
    Ok(bars_graph::store::revision_stamp(index.conn())?
        .ok_or_else(|| GraphError::InvalidData("отсутствуют метки входов".into()))?
        .input)
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

fn read<T: DeserializeOwned>(index: &Index, key: PayloadKey) -> Result<Option<T>, GraphError> {
    Ok(bars_graph::store::read_payload(index.conn(), key)?
        .and_then(|s| serde_json::from_str(&s).ok()))
}
pub(super) fn save(
    index: &Index,
    key: PayloadKey,
    value: &impl Serialize,
) -> Result<(), GraphError> {
    bars_graph::store::write_payload(index.conn(), key, &serde_json::to_string(value)?)
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

fn legacy_input_key(index: &Index) -> Result<String, GraphError> {
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

pub(super) fn input_key(index: &Index) -> Result<String, GraphError> {
    input_key_db(index.conn())
}

pub(super) fn input_key_db(db: &rusqlite::Connection) -> Result<String, GraphError> {
    if db.is_autocommit() {
        let tx = db.unchecked_transaction()?;
        let key = input_key_db(&tx)?;
        tx.commit()?;
        return Ok(key);
    }
    let mut hash = blake3::Hasher::new();
    hash.update(b"knn-input-v2");
    // Детерминированные строки JSON задают границы значений; BLOB идёт последовательным проходом.
    for sql in [
        "SELECT json_array(path,hash,module,title) FROM documents ORDER BY path",
        "SELECT json_array(d.path,c.ordinal,c.embed_hash,c.first_line,c.last_line,c.heading_chain,c.text) FROM chunks c JOIN documents d ON d.id=c.document_id ORDER BY d.path,c.ordinal",
        "SELECT json_array(key,value) FROM meta WHERE key LIKE 'model_%' OR key IN ('docs_root','chunk_min_chars','chunk_max_chars','chunking_version') ORDER BY key",
    ] {
        let mut stmt = db.prepare(sql)?;
        for row in stmt.query_map([], |r| r.get::<_, String>(0))? {
            hash.update(row?.as_bytes());
        }
    }
    hash.update(bars_graph::METHOD_VERSION.as_bytes());
    hash.update(METHOD.as_bytes());
    let mut stmt = db.prepare("SELECT v.embed_hash,v.vector FROM vectors v WHERE EXISTS(SELECT 1 FROM chunks c WHERE c.embed_hash=v.embed_hash) ORDER BY v.embed_hash")?;
    for row in stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
    })? {
        let (id, bytes) = row?;
        hash.update(&(id.len() as u64).to_le_bytes());
        hash.update(id.as_bytes());
        hash.update(&(bytes.len() as u64).to_le_bytes());
        hash.update(&bytes);
    }
    Ok(format!("knn-input-v2:{}", hash.finalize().to_hex()))
}

// V1 не хешировала соответствие чанков документам. Доказываем его по сохранённому тексту.
fn legacy_matches(index: &Index, prepared: &Prepared) -> Result<bool, GraphError> {
    let Some(layout) = index
        .stored_layout()
        .map_err(|e| GraphError::InvalidData(e.to_string()))?
    else {
        return Ok(false);
    };
    if prepared.expected != bars_graph::store::document_manifest(index.conn())?
        || !prepared.errors.is_empty()
    {
        return Ok(false);
    }
    let mut expected_vectors = Vec::new();
    let mut expected_refs = std::collections::BTreeSet::new();
    if bars_graph::store::validate(&prepared.data).is_err() {
        return Ok(false);
    }
    let nodes: BTreeMap<_, _> = prepared
        .data
        .nodes
        .iter()
        .filter(|n| n.reference.kind == Kind::Chunk)
        .map(|n| (n.reference.id.as_str(), n))
        .collect();
    let evidence: BTreeMap<_, _> = prepared
        .data
        .evidence
        .iter()
        .map(|e| (e.id.as_str(), e))
        .collect();
    for source in &prepared.data.sources {
        let (hash, module, title): (String, String, String) = index.conn().query_row(
            "SELECT hash,module,title FROM documents WHERE path=?1",
            [&source.meta.path],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if hash != source.meta.hash || module != source.meta.module || title != source.meta.title {
            return Ok(false);
        }
        let mapped = chunk_document_with_spans(&source.meta.path, &source.text, &layout.chunking);
        let mut stmt=index.conn().prepare("SELECT ordinal,embed_hash,first_line,last_line,heading_chain,text FROM chunks c JOIN documents d ON d.id=c.document_id WHERE d.path=?1 ORDER BY ordinal")?;
        let stored = stmt
            .query_map([&source.meta.path], |r| {
                Ok((
                    r.get::<_, u32>(0)? as usize,
                    r.get::<_, String>(1)?,
                    r.get::<_, u32>(2)? as usize,
                    r.get::<_, u32>(3)? as usize,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if stored.len() != mapped.document.chunks.len() {
            return Ok(false);
        }
        for (row, chunk) in stored.iter().zip(&mapped.document.chunks) {
            if row
                != &(
                    chunk.ordinal,
                    chunk.embed_hash.clone(),
                    chunk.first_line,
                    chunk.last_line,
                    chunk.heading_chain.clone(),
                    chunk.text.clone(),
                )
            {
                return Ok(false);
            }
            // Ревизия узла — хеш исходника; embed_hash входит в его ID и пару vectors.
            let reference = bars_graph::object_ref(
                &prepared.data.namespace,
                Kind::Chunk,
                &format!(
                    "{}:{}:{}",
                    source.meta.docs_path, chunk.ordinal, chunk.embed_hash
                ),
                &source.meta.hash,
            );
            let Some(node) = nodes.get(reference.id.as_str()) else {
                return Ok(false);
            };
            let spans = &mapped.source_spans[chunk.ordinal];
            if node.label != chunk.heading_chain
                || node.source_ids != [source.meta.reference.id.clone()]
                || node.evidence.len() != spans.len()
            {
                return Ok(false);
            }
            for (id, range) in node.evidence.iter().zip(spans) {
                let Some(proof) = evidence.get(id.as_str()) else {
                    return Ok(false);
                };
                if proof.document != source.meta.reference
                    || proof.span.start != range.start
                    || proof.span.end != range.end
                {
                    return Ok(false);
                }
            }
            expected_refs.insert((reference.id.clone(), reference.revision));
            if index
                .has_vector(&chunk.embed_hash)
                .map_err(|e| GraphError::InvalidData(e.to_string()))?
            {
                expected_vectors.push((reference.id, chunk.embed_hash.clone()));
            }
        }
    }
    let refs = prepared
        .data
        .nodes
        .iter()
        .filter(|n| n.reference.kind == Kind::Chunk)
        .map(|n| (n.reference.id.clone(), n.reference.revision.clone()))
        .collect::<std::collections::BTreeSet<_>>();
    expected_vectors.sort();
    let mut vectors = prepared.vectors.clone();
    vectors.sort();
    Ok(refs == expected_refs
        && vectors == expected_vectors
        && prepared.data.sources.len() == prepared.expected.len())
}

pub(super) fn load(
    project: &Path,
    index: &Index,
    layer: bars_graph::BuildLayer,
) -> Result<Option<Prepared>, GraphError> {
    let prepared: Option<Prepared> = read(index, PayloadKey::Prepared)?;
    let Some(mut prepared) = prepared else {
        return Ok(None);
    };
    let namespace = project
        .canonicalize()
        .map_err(|e| GraphError::InvalidData(e.to_string()))?
        .to_string_lossy()
        .replace('\\', "/");
    let current_key = input_key(index)?;
    let compatible = if prepared.key.starts_with("knn-input-v2:") {
        prepared.key == current_key
    } else {
        prepared.key == legacy_input_key(index)? && legacy_matches(index, &prepared)?
    };
    if prepared.layer == layer && prepared.data.namespace == namespace && compatible {
        if prepared.key != current_key {
            prepared.key = current_key;
            save(index, PayloadKey::Prepared, &prepared)?;
        }
        Ok(Some(prepared))
    } else {
        let tx = index.conn().unchecked_transaction()?;
        tx.execute(
            "DELETE FROM graph_payload WHERE key IN ('knn_prepared','knn_checkpoint')",
            [],
        )?;
        tx.commit()?;
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
        read(index, PayloadKey::Checkpoint)?,
        read(index, PayloadKey::Complete)?,
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
    let checkpoint = serde_json::to_string(&calculation.checkpoint)?;
    let tx = index
        .conn_mut()
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    bars_graph::store::write_payload(&tx, PayloadKey::Checkpoint, &checkpoint)?;
    let c = &calculation.checkpoint;
    bars_graph::store::set_progress(
        &tx,
        &bars_graph::GraphProgress {
            phase: "similarity_pairs".into(),
            processed: c.compared as usize,
            total: Some(c.total as usize),
            elapsed_ms: c.elapsed_ms,
        },
    )?;
    tx.commit()?;
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
        Some((&prepared.key, &checkpoint)),
        cancelled,
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

    fn paused_fixture() -> (tempfile::TempDir, Index) {
        let (dir, mut index) = fixture();
        let db = rusqlite::Connection::open(dir.path().join("index.sqlite")).unwrap();
        let result = crate::graph::build_index_graph_step(
            dir.path(),
            &mut index,
            bars_graph::BuildLayer::All,
            || {
                db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM graph_payload WHERE key='knn_prepared')",
                    [],
                    |r| r.get::<_, bool>(0),
                )
                .unwrap_or(false)
            },
        );
        assert!(matches!(result, Err(GraphError::Cancelled)));
        (dir, index)
    }

    #[test]
    fn resume_rejects_changed_chunk_mapping_and_document_labels_even_with_legacy_key() {
        for change in [
            "UPDATE documents SET module='other',title='renamed'",
            "DELETE FROM chunks WHERE id=(SELECT min(id) FROM chunks)",
            "UPDATE chunks SET first_line=first_line+1 WHERE id=(SELECT min(id) FROM chunks)",
        ] {
            for legacy in [false, true] {
                let (dir, index) = paused_fixture();
                let mut prepared: Prepared = read(&index, PayloadKey::Prepared).unwrap().unwrap();
                if legacy {
                    prepared.key = legacy_input_key(&index).unwrap();
                    save(&index, PayloadKey::Prepared, &prepared).unwrap();
                }
                index.conn().execute(change, []).unwrap();
                assert!(
                    load(dir.path(), &index, bars_graph::BuildLayer::All)
                        .unwrap()
                        .is_none(),
                    "{change}, legacy={legacy}"
                );
            }
        }
    }

    #[test]
    fn compatible_legacy_prepared_rekeys_and_resumes_without_source_files() {
        let (dir, mut index) = paused_fixture();
        let mut prepared: Prepared = read(&index, PayloadKey::Prepared).unwrap().unwrap();
        prepared.key = legacy_input_key(&index).unwrap();
        save(&index, PayloadKey::Prepared, &prepared).unwrap();
        // Настоящее размещение v1 перед миграцией, JSON и курсор не пересериализуются.
        let names:Vec<String>=index.conn().prepare("SELECT name FROM sqlite_master WHERE type='trigger' AND (name GLOB 'graph_*' OR name GLOB 'knn_inputs_*')").unwrap().query_map([],|r|r.get(0)).unwrap().collect::<Result<_,_>>().unwrap();
        for name in names {
            index
                .conn()
                .execute_batch(&format!("DROP TRIGGER {name}"))
                .unwrap();
        }
        index.conn().execute_batch("CREATE TABLE old_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL) WITHOUT ROWID; INSERT INTO old_meta SELECT * FROM graph_meta; INSERT INTO old_meta SELECT * FROM graph_payload; DROP TABLE graph_meta; ALTER TABLE old_meta RENAME TO graph_meta; DROP TABLE graph_payload; DROP TABLE graph_revisions; UPDATE graph_meta SET value='1' WHERE key='schema_version';").unwrap();
        let previous = bars_graph::store::read_payload(index.conn(), PayloadKey::Prepared).unwrap();
        bars_graph::store::ensure_schema(index.conn_mut()).unwrap();
        assert_eq!(
            previous,
            bars_graph::store::read_payload(index.conn(), PayloadKey::Prepared).unwrap()
        );
        let prepared = load(dir.path(), &index, bars_graph::BuildLayer::All)
            .unwrap()
            .unwrap();
        assert!(prepared.key.starts_with("knn-input-v2:"));
        run_quantum(&mut index, prepared, || false, Duration::ZERO).unwrap();
        let checkpoint =
            read::<crate::graph::similarity_job::Checkpoint>(&index, PayloadKey::Checkpoint)
                .unwrap()
                .unwrap();
        std::fs::rename(dir.path().join("docs"), dir.path().join("hidden-docs")).unwrap();
        let prepared = load(dir.path(), &index, bars_graph::BuildLayer::All)
            .unwrap()
            .unwrap();
        let status = run_quantum(&mut index, prepared, || false, Duration::from_secs(30))
            .unwrap()
            .unwrap();
        let complete =
            read::<crate::graph::similarity_job::Checkpoint>(&index, PayloadKey::Complete)
                .unwrap()
                .unwrap();
        assert_eq!(checkpoint.ids, complete.ids);
        assert!(status.snapshot.is_some());
        assert!(
            read::<Prepared>(&index, PayloadKey::Prepared)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn completion_payload_failure_rolls_back_published_generation_and_can_retry() {
        let (dir, mut index) = paused_fixture();
        let prepared = load(dir.path(), &index, bars_graph::BuildLayer::All)
            .unwrap()
            .unwrap();
        let old = bars_graph::store::status(index.conn()).unwrap().snapshot;
        index.conn().execute_batch("CREATE TRIGGER fail_complete BEFORE INSERT ON graph_payload WHEN NEW.key='knn_complete' BEGIN SELECT RAISE(ABORT,'injected completion failure'); END").unwrap();
        assert!(run_quantum(&mut index, prepared, || false, Duration::from_secs(30)).is_err());
        assert_eq!(
            bars_graph::store::status(index.conn()).unwrap().snapshot,
            old
        );
        assert!(
            read::<Prepared>(&index, PayloadKey::Prepared)
                .unwrap()
                .is_some()
        );
        assert!(
            read::<crate::graph::similarity_job::Checkpoint>(&index, PayloadKey::Complete)
                .unwrap()
                .is_none()
        );
        index
            .conn()
            .execute_batch("DROP TRIGGER fail_complete")
            .unwrap();
        let prepared = load(dir.path(), &index, bars_graph::BuildLayer::All)
            .unwrap()
            .unwrap();
        assert!(
            run_quantum(&mut index, prepared, || false, Duration::from_secs(30))
                .unwrap()
                .is_some()
        );
        assert!(
            read::<Prepared>(&index, PayloadKey::Prepared)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn ready_generation_does_not_skip_rebuild_after_same_key_input_changes() {
        for change in [
            "UPDATE documents SET module='other',title='renamed'",
            "UPDATE vectors SET vector=X'0000803f0000803f'",
        ] {
            let (dir, mut index) = fixture();
            crate::graph::build_index_graph(dir.path(), &mut index, || false).unwrap();
            index.conn().execute(change, []).unwrap();
            assert_eq!(
                bars_graph::store::status(index.conn()).unwrap().freshness,
                bars_graph::Freshness::Stale
            );
            let rebuilt =
                crate::graph::build_index_graph(dir.path(), &mut index, || false).unwrap();
            assert_ne!(rebuilt.freshness, bars_graph::Freshness::Stale);
            let data = bars_graph::store::load(index.conn()).unwrap();
            if change.contains("module") {
                assert!(
                    data.sources
                        .iter()
                        .all(|s| s.meta.module == "other" && s.meta.title == "renamed")
                );
            }
        }
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
                    "SELECT EXISTS(SELECT 1 FROM graph_payload WHERE key='knn_prepared')",
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
        assert!(
            read::<Prepared>(&index, PayloadKey::Prepared)
                .unwrap()
                .is_none()
        );
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
                    "SELECT EXISTS(SELECT 1 FROM graph_payload WHERE key='knn_prepared')",
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
            read::<crate::graph::similarity_job::Checkpoint>(&index, PayloadKey::Checkpoint)
                .unwrap()
                .is_none()
        );
        save(&index, PayloadKey::Checkpoint, &"truncated checkpoint").unwrap();
        assert!(
            read::<crate::graph::similarity_job::Checkpoint>(&index, PayloadKey::Checkpoint)
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
                    "SELECT EXISTS(SELECT 1 FROM graph_payload WHERE key='knn_prepared')",
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
                    "SELECT EXISTS(SELECT 1 FROM graph_payload WHERE key='knn_prepared')",
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
