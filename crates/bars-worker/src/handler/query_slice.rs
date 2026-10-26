use super::errors::worker_error;
use super::graph::graph_error;
use super::{WorkerHandler, index_path, internal};
use bars_core::graph::{
    Dataset, Freshness, GraphError, LayerState, centroid, physical, query, query_slice,
};
use bars_core::storage::Index;
use bars_protocol::{GraphQuerySliceParams, QueryGraphSlice, RpcError};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub(super) type RepresentationCache =
    Arc<Mutex<Option<(PathBuf, String, Arc<centroid::DocumentRepresentations>)>>>;

pub(super) fn representations(
    cache: &RepresentationCache,
    project: &Path,
    index: &Index,
    data: &Dataset,
    deadline: Instant,
) -> Result<Arc<centroid::DocumentRepresentations>, GraphError> {
    if let Some(value) = cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .filter(|(path, snapshot, _)| path == project && snapshot == &data.snapshot)
        .map(|(_, _, v)| v.clone())
    {
        return Ok(value);
    }
    let value = Arc::new(centroid::document_representations(index, data, || {
        Instant::now() >= deadline
    })?);
    *cache.lock().unwrap_or_else(|e| e.into_inner()) =
        Some((project.into(), data.snapshot.clone(), value.clone()));
    Ok(value)
}

impl WorkerHandler {
    pub(super) async fn graph_query_slice(
        &self,
        params: GraphQuerySliceParams,
    ) -> Result<QueryGraphSlice, RpcError> {
        params.request.validate().map_err(graph_error)?;
        let project = self.resolve(&params.target)?;
        if !index_path(&project).exists() {
            return Err(graph_error(GraphError::NotReady(
                "индекс ещё не построен".into(),
            )));
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        let project_gate = self.worker.gate(&project).map_err(worker_error)?;
        let gate = tokio::time::timeout_at(deadline.into(), project_gate.clone().read_owned())
            .await
            .map_err(|_| graph_error(GraphError::TimedOut))?;
        let datasets = self.graph_cache.clone();
        let cache = self.document_representations.clone();
        tokio::task::spawn_blocking(move || {
            let _gate = gate;
            let data = datasets.load(&project,&project_gate,deadline).map_err(graph_error)?;
            let request = params.request;
            query::validate_scope(&data,&request.scope).map_err(graph_error)?;
            if request.snapshot.as_ref().is_some_and(|s| s != &data.snapshot) {return Err(graph_error(GraphError::SnapshotExpired));}
            let stale = data.layers.values().any(|l| l.freshness == Freshness::Stale);
            let index = Index::open_read_only(&index_path(&project)).map_err(internal)?;
            let vectors = if stale {None} else {Some(representations(&cache,&project,&index,&data,deadline).map_err(graph_error)?)};
            let allowed: std::collections::HashSet<_> = data.sources.iter().filter(|s| request.scope.allows(&s.meta)).map(|s| s.meta.reference.id.as_str()).collect();
            let covered = vectors.as_ref().map_or(0,|v| v.vectors.iter().filter(|v| allowed.contains(v.id.as_str())).count());
            let state = if stale {LayerState::Stale} else if covered == 0 {LayerState::NotBuilt} else if covered < allowed.len() {LayerState::Partial} else {LayerState::Ready};
            let layer = |state,note: &str| physical::Layer {state,note:note.into()};
            let inputs = physical::LayerInputs {
                markdown_link:layer(if stale {LayerState::Stale} else if data.failed_sources.is_empty() {LayerState::Ready} else {LayerState::Partial},"Ссылки текущего снимка графа"),
                document_similarity:layer(state, &format!("Точный cosine в области: {covered} из {} документов с готовыми представлениями{}",allowed.len(),if stale {"; граф устарел, близость ждёт обновления"} else {""})),
                chunk_similarity:layer(LayerState::NotBuilt,"Отдельная проекция чанков"),
            };
            query_slice::project(&data,&request,inputs,|id,targets| {
                if Instant::now() >= deadline {return Err(GraphError::TimedOut);}
                let Some(vectors) = &vectors else {return Ok(vec![])};
                centroid::exact_document_pairs(vectors,id,targets,request.min_strength,|| Instant::now() >= deadline)
                    .map_err(|e| if matches!(e,GraphError::Cancelled) {GraphError::TimedOut} else {e})
            }).map_err(graph_error)
        }).await.map_err(internal)?
    }
}
