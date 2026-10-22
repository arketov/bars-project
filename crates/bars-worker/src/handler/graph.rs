use super::*;
use bars_core::graph::{GraphError, query, store};
use bars_protocol::{GraphQueryParams, GraphReply, GraphStatus, GraphStatusParams};

pub(super) fn graph_error(error: GraphError) -> RpcError {
    let kind = match error {
        GraphError::InvalidParams(_) => ErrorKind::InvalidParams,
        GraphError::StaleRef(_) => ErrorKind::StaleRef,
        GraphError::SnapshotExpired => ErrorKind::SnapshotExpired,
        GraphError::CursorMismatch => ErrorKind::CursorMismatch,
        GraphError::NotReady(_) => ErrorKind::GraphNotReady,
        GraphError::CapabilityUnavailable(_) => ErrorKind::GraphCapabilityUnavailable,
        GraphError::Cancelled => ErrorKind::Cancelled,
        _ => return RpcError::standard(INTERNAL_ERROR, error.to_string()),
    };
    RpcError::bars(kind, error.to_string())
}
impl WorkerHandler {
    pub(super) async fn graph_search(
        &self,
        params: bars_protocol::GraphSearchParams,
    ) -> Result<bars_protocol::GraphSearchOutput, RpcError> {
        use bars_core::graph::{GraphRequest, Operation, RetrievalMode, Scope};
        let requested = params.mode;
        if params.related_budget == Some(0) {
            return Err(graph_error(bars_core::graph::GraphError::InvalidParams(
                "related_budget должен быть положительным".into(),
            )));
        }
        if requested != RetrievalMode::Mix
            && (!params.include_related || params.related_budget.is_some())
        {
            return Err(graph_error(bars_core::graph::GraphError::InvalidParams(
                "include_related/related_budget применимы только к mix".into(),
            )));
        }
        if requested == RetrievalMode::Graph {
            let status = self
                .worker
                .status(&self.usable(&params.search.target)?)
                .map_err(worker_error)?;
            let request = GraphRequest {
                operation: Operation::Relations,
                query: Some(params.search.query),
                scope: Scope {
                    modules: params.search.modules,
                    files: params.search.files,
                },
                budgets: params.graph_budgets,
                limit: params.search.limit,
                ..Default::default()
            };
            let graph_results = self
                .graph_query(GraphQueryParams {
                    target: params.search.target,
                    request,
                })
                .await?;
            return Ok(bars_protocol::GraphSearchOutput {
                search: SearchOutput {
                    results: vec![],
                    applied: AppliedParams {
                        results: 0,
                        candidates: 0,
                        rerank_candidates: 0,
                        min_score: 0.0,
                        score_kind: ScoreKind::Cosine,
                        clamped: false,
                    },
                    indexing: status.state == ProjectState::Indexing,
                    pending: to_u32(status.pending),
                    rerank_error: None,
                },
                requested_mode: requested,
                applied_mode: requested,
                fallback_reason: None,
                related: None,
                graph_results: Some(graph_results),
            });
        }
        let budgets = if requested == RetrievalMode::Mix && params.include_related {
            let mut budgets = params.graph_budgets;
            if let Some(bytes) = params.related_budget {
                budgets.max_bytes = Some(
                    budgets
                        .max_bytes
                        .map_or(bytes, |existing| existing.min(bytes)),
                );
            }
            Some(budgets)
        } else {
            None
        };
        let (search, related, mut fallback_reason) = self
            .search_with_graph(params.search, budgets, false)
            .await?;
        if requested == RetrievalMode::Mix && !params.include_related {
            fallback_reason = Some("Связи отключены: include_related=false".into());
        }
        let applied_mode = if fallback_reason.is_some() {
            RetrievalMode::Hybrid
        } else {
            requested
        };
        Ok(bars_protocol::GraphSearchOutput {
            search,
            requested_mode: requested,
            applied_mode,
            fallback_reason,
            related,
            graph_results: None,
        })
    }
    pub(super) async fn graph_query(
        &self,
        params: GraphQueryParams,
    ) -> Result<GraphReply, RpcError> {
        query::validate_request(&params.request).map_err(graph_error)?;
        let project = self.resolve(&params.target)?;
        let settings = if params.request.operation == bars_core::graph::Operation::Context {
            Some(self.settings(&project)?)
        } else {
            None
        };
        let mut embedder = settings
            .as_ref()
            .and_then(|s| self.worker.models().resident_local_embedder(s));
        if !index_path(&project).exists() {
            return Err(graph_error(GraphError::NotReady(
                "индекс ещё не построен".into(),
            )));
        }
        let gate = self
            .worker
            .gate(&project)
            .map_err(worker_error)?
            .read_owned()
            .await;
        tokio::task::spawn_blocking(move || {
            let _gate=gate;
            let index=Index::open_read_only(&index_path(&project)).map_err(internal)?;
            let mut data=store::load(index.conn()).map_err(|e|{
                if params.request.snapshot.is_some()&&matches!(e,GraphError::NotReady(_)){graph_error(GraphError::SnapshotExpired)}else{graph_error(e)}
            })?;
            query::validate_scope(&data, &params.request.scope).map_err(graph_error)?;
            let started=std::time::Instant::now();let mut scores=std::collections::BTreeMap::new();
            if let Some(settings)=settings{
                if let Some(model)=embedder.as_mut(){
                    match bars_core::graph::context::evidence_scores(&index,&data,model,&settings.search,&params.request){
                        Ok((values,limited))=>{scores=values;if limited{data.warnings.push("Контекст: отбор по ролям прерван по timeout_ms".into());}},
                        Err(error)=>data.warnings.push(format!("Контекст: существующая модель недоступна, использован текстовый отбор: {error}")),
                    }
                }else{data.warnings.push("Контекст: локальная embed-модель не загружена; использован текстовый отбор без загрузки модели".into());}
            }
            query::execute_ranked(&data,&params.request,&scores,started.elapsed(),||false).map_err(graph_error)
        }).await.map_err(internal)?
    }
    pub(super) async fn graph_status(
        &self,
        params: GraphStatusParams,
    ) -> Result<GraphStatus, RpcError> {
        let project = self.resolve(&params.target)?;
        let worker = self.worker.clone();
        let gate = self
            .worker
            .gate(&project)
            .map_err(worker_error)?
            .read_owned()
            .await;
        tokio::task::spawn_blocking(move || {
            let _gate = gate;
            if !index_path(&project).exists() {
                return Ok(GraphStatus {
                    state: bars_core::graph::LayerState::NotBuilt,
                    snapshot: None,
                    active_job: worker.active_graph_job(&project),
                    source_count: 0,
                    error: None,
                    method: bars_core::graph::METHOD_VERSION.into(),
                });
            }
            // Чтение статуса во время создания текстовой БД не должно запускать её миграцию.
            let db = rusqlite::Connection::open_with_flags(
                index_path(&project),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .map_err(internal)?;
            let mut state = store::status(&db).map_err(graph_error)?;
            if let Some(job) = worker.active_graph_job(&project) {
                state.active_job = Some(job);
                state.state = bars_core::graph::LayerState::Building;
            }
            Ok(state)
        })
        .await
        .map_err(internal)?
    }
    /// Срез физического графа для окна: ссылки и близость чанков из готового графа, близость документов — по centroid готовых векторов.
    pub(super) async fn graph_physical(
        &self,
        params: bars_protocol::GraphPhysicalParams,
    ) -> Result<bars_protocol::PhysicalGraphSlice, RpcError> {
        use bars_core::graph::LayerState;
        use bars_core::graph::centroid::{self, DocumentSimilarity};
        use bars_core::graph::physical::{self, Layer};
        use bars_core::graph::similarity::StopReason;
        let project = self.resolve(&params.target)?;
        if !index_path(&project).exists() {
            return Err(graph_error(GraphError::NotReady(
                "индекс ещё не построен".into(),
            )));
        }
        let gate = self
            .worker
            .gate(&project)
            .map_err(worker_error)?
            .read_owned()
            .await;
        let cache = self.physical_similarity.clone();
        tokio::task::spawn_blocking(move || {
            let _gate = gate;
            let index = Index::open_read_only(&index_path(&project)).map_err(internal)?;
            let data = store::load(index.conn()).map_err(graph_error)?;
            let request = params.request;
            query::validate_scope(&data, &request.scope).map_err(graph_error)?;
            // Расчёт близости документов дорогой, а раскрытие чанков и «Показать ещё» повторяют запрос на том же снимке.
            let cached = {
                let slot = cache.lock().unwrap_or_else(|e| e.into_inner());
                slot.as_ref()
                    .filter(|(path, snapshot, _)| path == &project && snapshot == &data.snapshot)
                    .map(|(_, _, value)| value.clone())
            };
            let similarity: std::sync::Arc<DocumentSimilarity> = match cached {
                Some(value) => value,
                None => {
                    let value = std::sync::Arc::new(
                        centroid::document_similarity(&index, &data, || false)
                            .map_err(graph_error)?,
                    );
                    *cache.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some((project.clone(), data.snapshot.clone(), value.clone()));
                    value
                }
            };
            let layer = |state: LayerState, note: String| Layer { state, note };
            let document_similarity = if similarity.documents == 0 || similarity.covered == 0 {
                layer(
                    LayerState::Failed,
                    "Близость документов недоступна: у документов нет готовых векторов".into(),
                )
            } else if similarity.covered < similarity.documents || similarity.stopped.is_some() {
                let mut note = format!(
                    "Близость документов посчитана для {} из {} документов",
                    similarity.covered, similarity.documents
                );
                if let Some((path, reason)) = similarity.skipped.first() {
                    note.push_str(&format!("; например, {path}: {reason}"));
                }
                if similarity.stopped == Some(StopReason::ComparisonLimit) {
                    note.push_str(&format!(
                        "; расчёт остановлен по пределу сравнений ({}/{} строк)",
                        similarity.rows.0, similarity.rows.1
                    ));
                }
                layer(LayerState::Partial, note)
            } else {
                layer(
                    LayerState::Ready,
                    format!(
                        "Документные представления по готовым векторам ({})",
                        centroid::METHOD
                    ),
                )
            };
            // kNN считается по всему проекту, область лишь отбирает пары: у документа в узкой области может быть меньше k соседей.
            let narrowed = !request.scope.modules.is_empty() || !request.scope.files.is_empty();
            let document_similarity = if narrowed && document_similarity.state == LayerState::Ready {
                layer(
                    LayerState::Partial,
                    format!(
                        "{}; ближайшие документы выбраны по всему проекту, область лишь отбирает пары",
                        document_similarity.note
                    ),
                )
            } else {
                document_similarity
            };
            let chunk_partial = data
                .warnings
                .iter()
                .find(|w| w.starts_with("Граф близости частичный"));
            let inputs = physical::LayerInputs {
                markdown_link: layer(
                    LayerState::Ready,
                    "Ссылки разрешены Markdown-парсером".into(),
                ),
                document_similarity,
                chunk_similarity: match chunk_partial {
                    Some(note) => layer(LayerState::Partial, note.clone()),
                    None => layer(
                        LayerState::Ready,
                        "Близость чанков по готовым векторам, k=5, tau=0,5".into(),
                    ),
                },
            };
            physical::project(&data, &similarity.pairs, &request, inputs).map_err(graph_error)
        })
        .await
        .map_err(internal)?
    }
}
