mod job;
use crate::indexing::chunking::chunk_document_with_spans;
use crate::storage::Index;
use bars_graph::source::{InputChunk, InputDocument};
use bars_graph::{GraphError, Kind, LayerState, Origin, Polarity, Predicate};
pub use job::GraphBuildState;
use std::io::Read;
use std::path::Path;

pub fn build_index_graph(
    project: &Path,
    index: &mut Index,
    cancelled: impl FnMut() -> bool,
) -> Result<bars_graph::store::GraphStatus, GraphError> {
    build_index_graph_layer(project, index, bars_graph::BuildLayer::All, cancelled)
}

pub fn build_index_graph_layer(
    project: &Path,
    index: &mut Index,
    layer: bars_graph::BuildLayer,
    mut cancelled: impl FnMut() -> bool,
) -> Result<bars_graph::store::GraphStatus, GraphError> {
    let mut state = GraphBuildState::default();
    loop {
        if let Some(status) =
            build_index_graph_step_with_state(project, index, layer, &mut state, &mut cancelled)?
        {
            return Ok(status);
        }
    }
}

/// Один рабочий квант; None требует продолжения после освобождения gate проекта.
pub fn build_index_graph_step(
    project: &Path,
    index: &mut Index,
    layer: bars_graph::BuildLayer,
    cancelled: impl FnMut() -> bool,
) -> Result<Option<bars_graph::store::GraphStatus>, GraphError> {
    build_index_graph_step_with_state(
        project,
        index,
        layer,
        &mut GraphBuildState::default(),
        cancelled,
    )
}

pub fn build_index_graph_step_with_state(
    project: &Path,
    index: &mut Index,
    layer: bars_graph::BuildLayer,
    state: &mut GraphBuildState,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Option<bars_graph::store::GraphStatus>, GraphError> {
    bars_graph::store::ensure_schema(index.conn_mut())?;
    job::ensure_revision(index)?;
    let status = bars_graph::store::status(index.conn())?;
    if status.job_state == bars_graph::JobState::Idle
        && !index.conn().query_row("SELECT EXISTS(SELECT 1 FROM graph_payload WHERE key IN ('knn_prepared','knn_checkpoint'))",[],|r|r.get::<_,bool>(0))?
        && layer == bars_graph::BuildLayer::All
        && matches!(status.state, LayerState::Ready | LayerState::Partial)
        && status.file_errors_total == 0
        && status
            .layers
            .get("similarity")
            .is_some_and(|l| l.fingerprint == crate::graph::similarity_job::METHOD)
        && status.layers.len() == 3
        && status
            .layers
            .values()
            .all(|l| matches!(l.state, LayerState::Ready | LayerState::Partial))
        && bars_graph::store::load(index.conn()).is_ok()
    {
        return Ok(Some(status));
    }
    let job = format!(
        "auto:{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    bars_graph::store::set_state(
        index.conn(),
        LayerState::Building,
        status.active_job.as_deref().or(Some(&job)),
        None,
    )?;
    let result = build_graph(project, index, layer, state, &mut cancelled);
    if let Err(error) = &result {
        let state = if matches!(error, GraphError::Cancelled) {
            LayerState::Cancelled
        } else {
            LayerState::Failed
        };
        let _ = bars_graph::store::set_state(index.conn(), state, None, Some(&error.to_string()));
    }
    result
}

fn build_graph(
    project: &Path,
    index: &mut Index,
    layer: bars_graph::BuildLayer,
    state: &mut GraphBuildState,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Option<bars_graph::store::GraphStatus>, GraphError> {
    if let Some(active) = state.take_current(project, index, layer)? {
        return job::run_active(
            index,
            active,
            state,
            &mut cancelled,
            std::time::Duration::from_secs(2),
        );
    }
    if let Some(prepared) = job::load(project, index, layer)? {
        return job::run(index, prepared, state, &mut cancelled);
    }
    let layout = index
        .stored_layout()
        .map_err(|e| GraphError::InvalidData(e.to_string()))?
        .ok_or_else(|| GraphError::NotReady("индекс ещё не имеет параметров нарезки".into()))?;
    let preparation_key = job::input_key(index)?;
    let expected = bars_graph::store::document_manifest(index.conn())?;
    let previous = match bars_graph::store::load_previous(index.conn()) {
        Ok(data) => Some(data),
        Err(GraphError::NotReady(_)) => None,
        Err(error) => return Err(error),
    };
    let namespace = project
        .canonicalize()
        .map_err(|e| GraphError::InvalidData(e.to_string()))?
        .to_string_lossy()
        .replace('\\', "/");
    let docs = project
        .join(&layout.docs_root)
        .canonicalize()
        .map_err(|e| GraphError::InvalidData(e.to_string()))?;
    let mut inputs = Vec::new();
    let mut vectors = Vec::new();
    let mut errors = Vec::new();
    let started = std::time::Instant::now();
    let progress = |index: &Index, phase: &str, processed: usize, total: Option<usize>| {
        bars_graph::store::set_progress(
            index.conn(),
            &bars_graph::GraphProgress {
                phase: phase.into(),
                processed,
                total,
                elapsed_ms: started.elapsed().as_millis() as u64,
            },
        )
    };
    for (processed, (path, hash)) in expected.iter().enumerate() {
        progress(index, "structure", processed, Some(expected.len()))?;
        if cancelled() {
            return Err(GraphError::Cancelled);
        }
        let physical = match project.join(path).canonicalize() {
            Ok(path) => path,
            Err(_) => {
                errors.push(bars_graph::GraphFileError {
                    path: path.clone(),
                    code: "unreadable".into(),
                    message: "Файл недоступен".into(),
                });
                continue;
            }
        };
        let relative = physical
            .strip_prefix(&docs)
            .map_err(|_| {
                GraphError::InvalidData(
                    "индексированный файл за пределами корня документации".into(),
                )
            })?
            .to_string_lossy()
            .replace('\\', "/");
        if std::fs::metadata(&physical).is_ok_and(|m| m.len() > 16 * 1024 * 1024) {
            errors.push(bars_graph::GraphFileError {
                path: path.clone(),
                code: "too_large".into(),
                message: "Файл превышает предел графа 16 МиБ".into(),
            });
            continue;
        }
        let read_text = || -> Result<String, std::io::Error> {
            let mut bytes = Vec::new();
            std::fs::File::open(&physical)?
                .take(16 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 16 * 1024 * 1024 {
                return Err(std::io::Error::other("too_large"));
            }
            String::from_utf8(bytes)
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))
        };
        let text = match read_text() {
            Ok(text) => text,
            Err(error) => {
                let (code, message) = if error.kind() == std::io::ErrorKind::InvalidData {
                    ("invalid_utf8", "Файл не является UTF-8")
                } else if error.to_string() == "too_large" {
                    ("too_large", "Файл превышает предел графа 16 МиБ")
                } else {
                    ("unreadable", "Файл недоступен")
                };
                errors.push(bars_graph::GraphFileError {
                    path: path.clone(),
                    code: code.into(),
                    message: message.into(),
                });
                continue;
            }
        };
        if blake3::hash(text.as_bytes()).to_hex().as_str() != hash {
            return Err(GraphError::StaleRef(path.clone()));
        }
        let (module, title): (String, String) = index.conn().query_row(
            "SELECT module,title FROM documents WHERE path=?1",
            [path],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let mapped = chunk_document_with_spans(path, &text, &layout.chunking);
        let mut statement=index.conn().prepare("SELECT c.ordinal,c.embed_hash FROM chunks c JOIN documents d ON d.id=c.document_id WHERE d.path=?1 ORDER BY c.ordinal")?;
        let stored: Vec<(usize, String)> = statement
            .query_map([path], |r| Ok((r.get::<_, u32>(0)? as usize, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        if stored.len() != mapped.document.chunks.len() {
            return Err(GraphError::StaleRef(format!(
                "{path}: нарезка не совпадает с индексом"
            )));
        }
        let mut chunks = Vec::new();
        for (ordinal, hash) in stored {
            let chunk = mapped
                .document
                .chunks
                .get(ordinal)
                .ok_or_else(|| GraphError::InvalidData("неверный ordinal чанка".into()))?;
            if chunk.embed_hash != hash {
                return Err(GraphError::StaleRef(format!(
                    "{path}: отпечаток чанка изменился"
                )));
            }
            let key = format!("{ordinal}:{hash}");
            let reference = bars_graph::object_ref(
                &namespace,
                Kind::Chunk,
                &format!("{relative}:{key}"),
                &hash,
            );
            if matches!(
                layer,
                bars_graph::BuildLayer::All | bars_graph::BuildLayer::Similarity
            ) && index
                .has_vector(&hash)
                .map_err(|e| GraphError::InvalidData(e.to_string()))?
            {
                vectors.push((reference.id, hash.clone()));
            }
            chunks.push(InputChunk {
                key,
                label: chunk.heading_chain.clone(),
                spans: mapped.source_spans[ordinal].clone(),
            });
        }
        inputs.push(InputDocument {
            path: path.clone(),
            docs_path: relative,
            module,
            title,
            text,
            chunks,
        });
    }
    let mut data = bars_graph::source::build_cancellable(
        &namespace,
        &layout.docs_root,
        inputs,
        &mut cancelled,
    )?;
    let reuse = previous.filter(|old| {
        let sources = |dataset: &bars_graph::Dataset| {
            dataset
                .sources
                .iter()
                .map(|s| (s.meta.path.clone(), s.meta.hash.clone()))
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        sources(old) == sources(&data)
    });
    if matches!(
        layer,
        bars_graph::BuildLayer::Structure | bars_graph::BuildLayer::Similarity
    ) {
        data.edges.retain(|e| e.origin != Origin::ExplicitRule);
        data.semantic_state = LayerState::NotBuilt;
        if let Some(old) = &reuse
            && old
                .layers
                .get("semantic")
                .is_some_and(|l| l.freshness == bars_graph::Freshness::Current)
        {
            data.edges.extend(
                old.edges
                    .iter()
                    .filter(|e| e.origin == Origin::ExplicitRule)
                    .cloned(),
            );
            data.semantic_state = old.semantic_state;
        }
    }
    if matches!(
        layer,
        bars_graph::BuildLayer::Structure | bars_graph::BuildLayer::Semantic
    ) {
        if let Some(old) = &reuse
            && old
                .layers
                .get("similarity")
                .is_some_and(|l| l.freshness == bars_graph::Freshness::Current)
        {
            data.edges.extend(
                old.edges
                    .iter()
                    .filter(|e| e.origin == Origin::Similarity)
                    .cloned(),
            );
            data.similarity_state = old.similarity_state;
            if let Some(layer) = old.layers.get("similarity") {
                data.layers.insert("similarity".into(), layer.clone());
            }
            data.warnings.extend(
                old.warnings
                    .iter()
                    .filter(|w| bars_graph::is_similarity_warning(w))
                    .cloned(),
            );
        }
    } else {
        let prepared = job::Prepared {
            key: preparation_key,
            layer,
            data,
            vectors,
            expected,
            errors,
        };
        let serialized = serde_json::to_string(&prepared)?;
        let tx = index
            .conn_mut()
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if job::input_key_db(&tx)? != prepared.key {
            return Err(GraphError::StaleRef(
                "входы изменились при подготовке".into(),
            ));
        }
        bars_graph::store::write_payload(
            &tx,
            bars_graph::store::PayloadKey::Prepared,
            &serialized,
        )?;
        bars_graph::store::delete_payload(&tx, bars_graph::store::PayloadKey::Checkpoint)?;
        tx.commit()?;
        return job::run(index, prepared, state, &mut cancelled);
    }
    finish_graph(index, data, expected, errors, None, &mut cancelled).map(Some)
}

fn finish_graph(
    index: &mut Index,
    mut data: bars_graph::Dataset,
    expected: std::collections::BTreeMap<String, String>,
    errors: Vec<bars_graph::GraphFileError>,
    completion: Option<(&str, &str)>,
    mut cancelled: impl FnMut() -> bool,
) -> Result<bars_graph::store::GraphStatus, GraphError> {
    for error in &errors {
        let (module, title): (String, String) = index.conn().query_row(
            "SELECT module,title FROM documents WHERE path=?1",
            [&error.path],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let relative = Path::new(&error.path)
            .strip_prefix(&data.docs_root)
            .map_err(|_| GraphError::InvalidData("путь ошибки вне корня документации".into()))?
            .to_string_lossy()
            .replace('\\', "/");
        data.failed_sources.push(bars_graph::SourceMeta {
            reference: bars_graph::object_ref(
                &data.namespace,
                Kind::Document,
                &relative,
                &expected[&error.path],
            ),
            path: error.path.clone(),
            docs_path: relative,
            module,
            title,
            hash: expected[&error.path].clone(),
        });
    }
    if !errors.is_empty() {
        data.warnings.push(
            "Граф частичный: файлы с ошибками исключены; покрытие учитывает область запроса".into(),
        );
    }
    data.fingerprint();
    data.snapshot = blake3::hash(&serde_json::to_vec(&(&data.snapshot, &expected, &errors))?)
        .to_hex()
        .to_string();
    bars_graph::store::set_progress(
        index.conn(),
        &bars_graph::GraphProgress {
            phase: "publish".into(),
            processed: expected.len(),
            total: Some(expected.len()),
            elapsed_ms: bars_graph::store::status(index.conn())?
                .progress
                .map_or(0, |p| p.elapsed_ms),
        },
    )?;
    if cancelled() {
        return Err(GraphError::Cancelled);
    }
    bars_graph::store::validate(&data)?;
    let tx = index
        .conn_mut()
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if let Some((key, _)) = completion
        && job::input_key_db(&tx)? != key
    {
        return Err(GraphError::StaleRef("входы kNN изменились".into()));
    }
    bars_graph::store::publish_in_transaction(&tx, &data, &expected, &errors)?;
    if let Some((_, checkpoint)) = completion {
        use bars_graph::store::{PayloadKey, delete_payload, write_payload};
        write_payload(&tx, PayloadKey::Complete, checkpoint)?;
        delete_payload(&tx, PayloadKey::Prepared)?;
        delete_payload(&tx, PayloadKey::Checkpoint)?;
    }
    if cancelled() {
        return Err(GraphError::Cancelled);
    }
    tx.commit()?;
    bars_graph::store::status(index.conn())
}
