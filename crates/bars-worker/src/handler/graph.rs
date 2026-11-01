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
        GraphError::TimedOut => ErrorKind::GraphTimeout,
        _ => return RpcError::standard(INTERNAL_ERROR, error.to_string()),
    };
    let message = if kind == ErrorKind::GraphNotReady {
        format!(
            "{error}; граф обновляется автоматически после обновления индекса; \
            ручной запуск — из окна или bars-cli graph build. \
            bars_reindex не заменяет построение графа и не помогает при model_loading. \
            Пока используйте обычный bars_rag_search без mode или bars_read; \
            состояние — bars_status"
        )
    } else {
        error.to_string()
    };
    let next = match &error {
        GraphError::NotReady(_) => "next: bars_rag_search без mode",
        GraphError::StaleRef(_) => {
            "next: повторите исходную операцию без cursor и возьмите новый ObjectRef; старую revision не заменять вручную"
        }
        GraphError::SnapshotExpired | GraphError::CursorMismatch => {
            "next: повторите операцию без cursor и snapshot"
        }
        GraphError::TimedOut => {
            "next: сузьте modules/files или roles; timeout_ms можно увеличить до 5000"
        }
        GraphError::CapabilityUnavailable(_) => {
            "Поддержаны documents/roots, query и estimated budget у context, roots/documents у impact. next: уберите указанное неподдерживаемое поле"
        }
        GraphError::Cancelled => "next: повторите операцию, если результат ещё нужен",
        _ => "next: исправьте указанный параметр и повторите операцию",
    };
    RpcError::bars(
        kind,
        if message.contains("next:") {
            message
        } else {
            format!("{message}\n{next}")
        },
    )
}
#[cfg(test)]
mod readiness_tests {
    use super::*;

    #[test]
    fn graph_not_ready_preserves_reason_and_explains_recovery() {
        let error = graph_error(GraphError::NotReady("нет структурного слоя".into()));
        assert_eq!(error.kind(), Some(ErrorKind::GraphNotReady));
        for part in [
            "нет структурного слоя",
            "автоматически",
            "bars-cli graph build",
            "bars_reindex не заменяет",
            "bars_rag_search без mode",
            "bars_status",
        ] {
            assert!(error.message.contains(part), "{}", error.message);
        }
        let other = graph_error(GraphError::InvalidParams("неверная область".into()));
        assert_eq!(other.kind(), Some(ErrorKind::InvalidParams));
        assert!(!other.message.contains("bars_reindex"));
    }
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
            let scope = bars_protocol::SearchScope {
                modules: params.search.modules.clone(),
                files: params.search.files.clone(),
            };
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
                    text: false,
                    target: params.search.target,
                    request,
                })
                .await?;
            let mut output = bars_protocol::GraphSearchOutput {
                search: SearchOutput {
                    results: vec![],
                    applied: AppliedParams {
                        results: 0,
                        candidates: 0,
                        rerank_candidates: 0,
                        min_score: 0.0,
                        score_kind: ScoreKind::Cosine,
                        clamped: false,
                        scope,
                    },
                    by_module: Default::default(),
                    indexing: status.state == ProjectState::Indexing,
                    pending: to_u32(status.pending),
                    rerank_error: None,
                },
                requested_mode: requested,
                applied_mode: requested,
                fallback_reason: None,
                related: None,
                graph_results: Some(graph_results),
            };
            if params.text {
                bars_protocol::graph_text::prepare_graph_search_text(&mut output)
                    .map_err(graph_error)?;
            }
            return Ok(output);
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
        let mut output = bars_protocol::GraphSearchOutput {
            search,
            requested_mode: requested,
            applied_mode,
            fallback_reason,
            related,
            graph_results: None,
        };
        if params.text {
            bars_protocol::graph_text::prepare_graph_search_text(&mut output)
                .map_err(graph_error)?;
        }
        Ok(output)
    }
    pub(super) async fn graph_query(
        &self,
        params: GraphQueryParams,
    ) -> Result<GraphReply, RpcError> {
        query::validate_request(&params.request).map_err(graph_error)?;
        let started = std::time::Instant::now();
        let deadline = started
            + std::time::Duration::from_millis(u64::from(
                params
                    .request
                    .budgets
                    .resolve()
                    .map_err(graph_error)?
                    .0
                    .timeout_ms,
            ));
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
        let project_gate = self.worker.gate(&project).map_err(worker_error)?;
        let gate = tokio::time::timeout_at(deadline.into(), project_gate.clone().read_owned())
            .await
            .map_err(|_| graph_error(GraphError::TimedOut))?;
        let cache = self.graph_cache.clone();
        tokio::task::spawn_blocking(move || {
            let _gate=gate;
            let index=Index::open_read_only(&index_path(&project)).map_err(internal)?;
            let data=cache.load(&project, &project_gate, deadline).map_err(|e|{
                if params.request.snapshot.is_some()&&matches!(e,GraphError::NotReady(_)){graph_error(GraphError::SnapshotExpired)}else{graph_error(e)}
            })?;
            let mut overlay = query::QueryOverlay::default();
            query::validate_scope(&data, &params.request.scope).map_err(graph_error)?;
            if params.request.operation == bars_core::graph::Operation::Context {
                let current = store::document_manifest(index.conn()).map_err(graph_error)?;
                if !data.sources.iter().any(|s| params.request.scope.allows(&s.meta) && current.get(&s.meta.path) == Some(&s.meta.hash)) {
                    return Err(graph_error(GraphError::NotReady("В прежнем снимке нет подтверждённых текущих источников для контекста; обновите граф".into())));
                }
                overlay.current_sources = Some(current);
            }
            super::graph_cache::check(deadline).map_err(graph_error)?;
            let mut scores=std::collections::BTreeMap::new();
            if let Some(settings)=settings{
                if let Some(model)=embedder.as_mut(){
                    let mut request = params.request.clone();
                    request.budgets.timeout_ms = Some(deadline.saturating_duration_since(std::time::Instant::now()).as_millis().max(1) as u32);
                    match bars_core::graph::context::evidence_scores_current(&index,&data,model,&settings.search,&request,overlay.current_sources.as_ref()){
                        Ok((values,limited))=>{scores=values;if limited{overlay.warnings.push("Контекст: отбор по ролям прерван по timeout_ms".into());}},
                        Err(error)=>overlay.warnings.push(format!("Контекст: существующая модель недоступна, использован текстовый отбор: {error}")),
                    }
                }else{overlay.warnings.push("Контекст: локальная embed-модель не загружена; использован текстовый отбор без загрузки модели".into());}
            }
            let mut reply = query::execute_overlay(&data,&params.request,&scores,started.elapsed(),&overlay,||false).map_err(graph_error)?;
            if params.text { bars_protocol::graph_text::prepare_graph_text(&mut reply, bars_protocol::graph_text::MAX_TEXT_CHARS).map_err(graph_error)?; }
            Ok(reply)
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
                    freshness: bars_core::graph::Freshness::NotBuilt,
                    completeness: bars_core::graph::Completeness::Unknown,
                    availability: bars_core::graph::Availability::Enabled,
                    job_state: if worker.active_graph_job(&project).is_some() {
                        bars_core::graph::JobState::Queued
                    } else {
                        bars_core::graph::JobState::Idle
                    },
                    state: bars_core::graph::LayerState::NotBuilt,
                    snapshot: None,
                    active_job: worker.active_graph_job(&project),
                    source_count: 0,
                    error: None,
                    method: bars_core::graph::METHOD_VERSION.into(),
                    progress: None,
                    file_errors: vec![],
                    file_errors_total: 0,
                    layers: Default::default(),
                    paused: false,
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
                if state.job_state != bars_core::graph::JobState::Running {
                    state.job_state = bars_core::graph::JobState::Queued;
                }
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
        let project_gate = self.worker.gate(&project).map_err(worker_error)?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let gate = tokio::time::timeout_at(deadline.into(), project_gate.clone().read_owned())
            .await
            .map_err(|_| graph_error(GraphError::TimedOut))?;
        let cache = self.physical_similarity.clone();
        let representations = self.document_representations.clone();
        let datasets = self.graph_cache.clone();
        tokio::task::spawn_blocking(move || {
            let _gate = gate;
            let index = Index::open_read_only(&index_path(&project)).map_err(internal)?;
            let data = datasets.load(&project, &project_gate, deadline).map_err(graph_error)?;
            let stale = data.layers.values().any(|l| l.freshness == bars_core::graph::Freshness::Stale);
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
                _ if stale => std::sync::Arc::new(DocumentSimilarity { pairs: vec![], documents: data.sources.len(), covered: 0, skipped: vec![], stopped: None, rows: (0, 0) }),
                Some(value) => value,
                None => {
                    let value = std::sync::Arc::new(
                        centroid::similarity_from_representations(&data, super::query_slice::representations(&representations, &project, &index, &data, deadline).map_err(graph_error)?.as_ref(), || std::time::Instant::now() >= deadline)
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
                .find(|w| bars_core::graph::is_similarity_warning(w));
            let inputs = physical::LayerInputs {
                markdown_link: layer(
                    if data.failed_sources.is_empty() { LayerState::Ready } else { LayerState::Partial },
                    if data.failed_sources.is_empty() { "Ссылки разрешены Markdown-парсером".into() } else { "Ссылки исключённых файлов не обработаны; подробности в ошибках файлов".into() },
                ),
                document_similarity,
                chunk_similarity: match chunk_partial {
                    _ if data.similarity_state == LayerState::NotBuilt => layer(LayerState::NotBuilt, "Близость чанков ещё не построена".into()),
                    Some(note) => layer(LayerState::Partial, note.clone()),
                    None if data.similarity_state == LayerState::Partial => layer(LayerState::Partial,
                        data.layers.get("similarity").map(|l| format!("Близость посчитана для {} из {} чанков; {}", l.coverage.processed, l.coverage.total, l.coverage.reason.as_deref().unwrap_or("есть известные пропуски"))).unwrap_or_else(|| "Полнота прежнего расчёта близости неизвестна".into())),
                    None => layer(
                        LayerState::Ready,
                        "Близость чанков по готовым векторам, k=5, tau=0,5".into(),
                    ),
                },
            };
            let mut inputs = inputs;
            if stale {
                for layer in [&mut inputs.markdown_link, &mut inputs.document_similarity, &mut inputs.chunk_similarity] {
                    layer.state = LayerState::Stale;
                    layer.note = "Граф устарел · Показан предыдущий снимок; близость документов ждёт обновления".into();
                }
            }
            physical::project(&data, &similarity.pairs, &request, inputs).map_err(graph_error)
        })
        .await
        .map_err(internal)?
    }
}
