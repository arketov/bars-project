use super::similarity::{KnnOptions, VectorNode, build_knn};
use crate::indexing::chunking::chunk_document_with_spans;
use crate::storage::Index;
use bars_graph::source::{InputChunk, InputDocument};
use bars_graph::{GraphError, Kind, LayerState, Origin, Polarity, Predicate};
use std::path::Path;

pub fn build_index_graph(
    project: &Path,
    index: &mut Index,
    mut cancelled: impl FnMut() -> bool,
) -> Result<bars_graph::store::GraphStatus, GraphError> {
    bars_graph::store::ensure_schema(index.conn_mut())?;
    let status = bars_graph::store::status(index.conn())?;
    if matches!(status.state, LayerState::Ready | LayerState::Partial)
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
    bars_graph::store::set_state(index.conn(), LayerState::Building, Some(&job), None)?;
    let result = build_graph(project, index, &mut cancelled);
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
    mut cancelled: impl FnMut() -> bool,
) -> Result<bars_graph::store::GraphStatus, GraphError> {
    let layout = index
        .stored_layout()
        .map_err(|e| GraphError::InvalidData(e.to_string()))?
        .ok_or_else(|| GraphError::NotReady("индекс ещё не имеет параметров нарезки".into()))?;
    let expected = bars_graph::store::document_manifest(index.conn())?;
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
    for (path, hash) in &expected {
        if cancelled() {
            return Err(GraphError::Cancelled);
        }
        let physical = project
            .join(path)
            .canonicalize()
            .map_err(|e| GraphError::StaleRef(e.to_string()))?;
        let relative = physical
            .strip_prefix(&docs)
            .map_err(|_| {
                GraphError::InvalidData(
                    "индексированный файл за пределами корня документации".into(),
                )
            })?
            .to_string_lossy()
            .replace('\\', "/");
        let text =
            std::fs::read_to_string(&physical).map_err(|e| GraphError::StaleRef(e.to_string()))?;
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
            if let Some(vector) = index
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
    let knn = build_knn(
        &vectors,
        KnnOptions {
            k: 5,
            min_cosine: 0.5,
            max_comparisons: 1_000_000,
        },
        &mut cancelled,
    )
    .map_err(|e| GraphError::InvalidData(e.to_string()))?;
    if cancelled() {
        return Err(GraphError::Cancelled);
    }
    if knn.stop_reason.is_some() {
        data.warnings.push(format!(
            "Граф близости частичный: {:?}; завершено {}/{} строк",
            knn.stop_reason,
            knn.neighbors.len(),
            knn.total_nodes
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
    data.semantic_state = LayerState::Partial;
    data.fingerprint();
    bars_graph::store::publish(index.conn_mut(), &data, &expected)?;
    bars_graph::store::status(index.conn())
}
