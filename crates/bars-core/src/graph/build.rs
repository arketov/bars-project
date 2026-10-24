use super::similarity::{KnnOptions, VectorNode, build_knn_observed};
use crate::indexing::chunking::chunk_document_with_spans;
use crate::storage::Index;
use bars_graph::source::{InputChunk, InputDocument};
use bars_graph::{GraphError, Kind, LayerState, Origin, Polarity, Predicate};
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
    bars_graph::store::ensure_schema(index.conn_mut())?;
    let status = bars_graph::store::status(index.conn())?;
    if layer == bars_graph::BuildLayer::All
        && matches!(status.state, LayerState::Ready | LayerState::Partial)
        && status.file_errors_total == 0
        && status.layers.len() == 3
        && status
            .layers
            .values()
            .all(|l| matches!(l.state, LayerState::Ready | LayerState::Partial))
        && bars_graph::store::load(index.conn()).is_ok()
    {
        return Ok(status);
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
    let result = build_graph(project, index, layer, &mut cancelled);
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
    mut cancelled: impl FnMut() -> bool,
) -> Result<bars_graph::store::GraphStatus, GraphError> {
    let layout = index
        .stored_layout()
        .map_err(|e| GraphError::InvalidData(e.to_string()))?
        .ok_or_else(|| GraphError::NotReady("индекс ещё не имеет параметров нарезки".into()))?;
    let expected = bars_graph::store::document_manifest(index.conn())?;
    let previous = match bars_graph::store::load(index.conn()) {
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
            ) && let Some(vector) = index
                .vector(&hash)
                .map_err(|e| GraphError::InvalidData(e.to_string()))?
            {
                vectors.push(VectorNode {
                    id: reference.id,
                    vector,
                });
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
        if let Some(old) = &reuse {
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
        if let Some(old) = &reuse {
            data.edges.extend(
                old.edges
                    .iter()
                    .filter(|e| e.origin == Origin::Similarity)
                    .cloned(),
            );
            data.similarity_state = old.similarity_state;
            data.warnings.extend(
                old.warnings
                    .iter()
                    .filter(|w| bars_graph::is_similarity_warning(w))
                    .cloned(),
            );
        }
    } else {
        progress(index, "similarity", 0, Some(vectors.len()))?;
        let mut last_progress = std::time::Instant::now();
        let mut progress_error = None;
        let knn = build_knn_observed(
            &vectors,
            KnnOptions {
                k: 5,
                min_cosine: 0.5,
                max_comparisons: 1_000_000,
            },
            &mut cancelled,
            |processed, total| {
                if processed == total
                    || last_progress.elapsed() >= std::time::Duration::from_millis(100)
                {
                    if let Err(error) = progress(index, "similarity", processed, Some(total)) {
                        progress_error = Some(error);
                    }
                    last_progress = std::time::Instant::now();
                }
            },
        )
        .map_err(|e| GraphError::InvalidData(e.to_string()))?;
        if let Some(error) = progress_error {
            return Err(error);
        }
        if cancelled() {
            return Err(GraphError::Cancelled);
        }
        if let Some(reason) = knn.stop_reason {
            data.warnings.push(bars_graph::similarity_partial_warning(
                knn.neighbors.len(),
                knn.total_nodes,
                reason.label(),
            ));
        }
        for (from, neighbors) in knn.neighbors {
            for neighbor in neighbors {
                let a = data.nodes.iter().find(|n| n.reference.id == from).unwrap();
                let b = data
                    .nodes
                    .iter()
                    .find(|n| n.reference.id == neighbor.id)
                    .unwrap();
                let mut evidence = a.evidence.clone();
                evidence.extend(b.evidence.iter().cloned());
                evidence.sort();
                evidence.dedup();
                if evidence.is_empty() {
                    continue;
                }
                let id = blake3::hash(format!("similar:{from}:{}", neighbor.id).as_bytes())
                    .to_hex()
                    .to_string();
                data.edges.push(bars_graph::Edge {
                    id,
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
        data.similarity_state = LayerState::Partial;
    }
    for error in &errors {
        let (module, title): (String, String) = index.conn().query_row(
            "SELECT module,title FROM documents WHERE path=?1",
            [&error.path],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let relative = Path::new(&error.path)
            .strip_prefix(&layout.docs_root)
            .map_err(|_| GraphError::InvalidData("путь ошибки вне корня документации".into()))?
            .to_string_lossy()
            .replace('\\', "/");
        data.failed_sources.push(bars_graph::SourceMeta {
            reference: bars_graph::object_ref(
                &namespace,
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
    progress(index, "publish", expected.len(), Some(expected.len()))?;
    if cancelled() {
        return Err(GraphError::Cancelled);
    }
    bars_graph::store::publish_with_errors(index.conn_mut(), &data, &expected, &errors)?;
    bars_graph::store::status(index.conn())
}
