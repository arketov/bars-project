//! Обработчик методов протокола поверх воркера и `bars-core` (`docs/architecture/protocol.md`).
//!
//! `hello`, права клиента и сброс `accept_model_change` у MCP делает цикл соединения в
//! `bars-protocol`; сюда доходят уже проверенные вызовы. Окно вызывает те же методы без канала (Т-20).

mod errors;
mod graph;
mod history;
mod topics;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use bars_core::doc_path::resolve_doc_path;
use bars_core::indexing::chunking::chunk_document;
use bars_core::indexing::walk::read_utf8_path;
use bars_core::models::Reranker;
use bars_core::outline::{OutlineQuery, Under, outline};
use bars_core::search::{
    DuplicatesRequest, SearchRequest, SimilarRequest, duplicates, search, similar,
};
use bars_core::settings::Settings;
use bars_core::storage::{Index, Journal, index_path};
use bars_protocol::{
    AcceptedOutput, ActivityOutput, AppliedParams, AutostartGetParams, AutostartOutput,
    AutostartSetParams, Call, CancelOutput, CheckAllParams, ClientKind, ClientStatus,
    ClientsStatusOutput, DuplicateChange, DuplicateChunk, DuplicateCounts, DuplicateDifferences,
    DuplicateItem, DuplicateKind, DuplicatesOutput, DuplicatesParams, ErrorKind, FileProblem,
    Heading, INTERNAL_ERROR, IndexCancelParams, InstallClient, InstallOutput, InstallParams,
    InstructionOutput, JobKind as ProtocolJobKind, JobPriority, METHOD_NOT_FOUND,
    ModelDownloadParams, ModelInfo, ModelStatus, ModelsInfoOutput, ModuleInfo, OutlineOutput,
    OutlineParams, PausedOutput, ProjectInfo, ProjectProblems, ProjectState, ProjectsOutput,
    QueuedJob, ReadOutput, ReadParams, RegisterOutput, RegisterParams, ReindexParams, RpcError,
    RunningJob, SavedOutput, ScoreKind, SearchHit, SearchOutput, SearchParams, SecretOutput,
    SecretSetParams, SecretStatusParams, SessionStartParams, SetDocsRootParams, SettingsOutput,
    SettingsSaveParams, SimilarOutput, SimilarParams, StatusOutput, StatusParams, Target,
    TreeDocument, TreeOutput, TreeParams, UninstallOutput, UninstallParams, UnregisterOutput,
    UnregisterParams, output_value,
};
use serde_json::Value;

use crate::clients::{self, InstallEnv};
use crate::settings_io::{self, SaveError};
use crate::worker::{JobKind, Priority, ProjectSnapshot, Readiness, Worker, index_token, rfc3339};
use errors::{doc_path_error, outline_error, read_error, search_error, worker_error};

/// Выполняет вызовы канала на воркере.
#[derive(Clone)]
pub struct WorkerHandler {
    worker: Worker,
    history: crate::mcp_history::History,
    topics: crate::new_topics::Collector,
}

impl WorkerHandler {
    pub fn new(worker: Worker) -> Self {
        let history = crate::mcp_history::History::new(worker.bars_home().to_path_buf());
        history.start_maintenance();
        Self {
            worker,
            history,
            topics: crate::new_topics::Collector::new(),
        }
    }

    /// Сбор «Новых тем»: окно управляет им через внутренний API.
    pub fn topics(&self) -> &crate::new_topics::Collector {
        &self.topics
    }

    /// Вызов окна с внутренним контекстом (назначение, ID обращения). Контекст не часть протокола.
    pub async fn dispatch_with_context(
        &self,
        call: Call,
        context: crate::new_topics::SearchContext,
    ) -> Result<Value, RpcError> {
        crate::new_topics::CONTEXT
            .scope(context, self.dispatch(call))
            .await
    }

    /// Воркер под обработчиком: окно останавливает его при выходе.
    pub fn worker(&self) -> &Worker {
        &self.worker
    }

    /// Проект вызова: явный `project` или ближайший к `cwd` зарегистрированный (Т-15).
    fn resolve(&self, target: &Target) -> Result<PathBuf, RpcError> {
        let path = if let Some(selector) = &target.project {
            self.worker.resolve(selector).map_err(worker_error)?
        } else {
            self.worker
            .project_for_cwd(Path::new(&target.cwd))
            .ok_or_else(|| {
                RpcError::bars(
                    ErrorKind::ProjectNotFound,
                    format!(
                        "для папки {} нет зарегистрированного проекта: bars-cli register <папка проекта>",
                        target.cwd
                    ),
                )
            })?
        };
        history::applied_field(
            "resolved_project",
            serde_json::json!(path.display().to_string()),
        );
        history::applied_field(
            "resolved_project_name",
            serde_json::json!(
                self.worker
                    .projects()
                    .into_iter()
                    .find(|p| p.path == path)
                    .map(|p| p.name)
            ),
        );
        Ok(path)
    }

    /// Проект вызова, папка которого на месте.
    fn usable(&self, target: &Target) -> Result<PathBuf, RpcError> {
        let path = self.resolve(target)?;
        let status = self.worker.status(&path).map_err(worker_error)?;
        if status.state == ProjectState::Unavailable {
            return Err(RpcError::bars(
                ErrorKind::ProjectUnavailable,
                format!("папки проекта {} нет на диске", path.display()),
            ));
        }
        Ok(path)
    }

    fn settings(&self, project: &Path) -> Result<Settings, RpcError> {
        Settings::load(self.worker.bars_home(), project)
            .map(|loaded| loaded.value)
            .map_err(|error| RpcError::standard(INTERNAL_ERROR, error.to_string()))
    }

    /// Чанки, близкие к чанку из выдачи поиска, по готовым векторам: модели не нужны.
    async fn similar(&self, params: SimilarParams) -> Result<SimilarOutput, RpcError> {
        let project = self.usable(&params.target)?;
        let status = self.worker.status(&project).map_err(worker_error)?;
        if status.state == ProjectState::ReindexRequired {
            return Err(errors::reindex_required());
        }
        let settings = self.settings(&project)?;
        let search_settings = settings.search.clone();
        let request = SimilarRequest {
            id: params.chunk_id,
            modules: params.modules,
            results: params.limit.map(|value| value as usize),
        };
        let gate = self
            .worker
            .gate(&project)
            .map_err(worker_error)?
            .read_owned()
            .await;
        let response = tokio::task::spawn_blocking(move || {
            let _gate = gate;
            let index = Index::open(&index_path(&project), Journal::Wal)?;
            similar(&index, &search_settings, &request)
        })
        .await
        .map_err(internal)?
        .map_err(search_error)?;
        Ok(SimilarOutput {
            source: hit(response.source),
            results: response.hits.into_iter().map(hit).collect(),
            limit: to_u32(response.results),
            clamped: response.clamped,
            indexing: status.state == ProjectState::Indexing || status.queued,
            pending: to_u32(status.pending),
        })
    }

    /// Повторы в области проекта по готовым текстам и векторам индекса: модели не нужны.
    async fn duplicates(&self, params: DuplicatesParams) -> Result<DuplicatesOutput, RpcError> {
        let project = self.usable(&params.target)?;
        let status = self.worker.status(&project).map_err(worker_error)?;
        if status.state == ProjectState::ReindexRequired {
            return Err(errors::reindex_required());
        }
        let settings = self.settings(&project)?;
        let search_settings = settings.search.clone();
        let request = DuplicatesRequest {
            modules: params.modules,
            results: params.limit.map(|value| value as usize),
        };
        let gate = self
            .worker
            .gate(&project)
            .map_err(worker_error)?
            .read_owned()
            .await;
        let response = tokio::task::spawn_blocking(move || {
            let _gate = gate;
            let index = Index::open(&index_path(&project), Journal::Wal)?;
            duplicates(&index, &search_settings, &request)
        })
        .await
        .map_err(internal)?
        .map_err(search_error)?;
        Ok(DuplicatesOutput {
            results: response.items.into_iter().map(duplicate_item).collect(),
            counts: DuplicateCounts {
                exact: to_u32(response.counts.exact),
                near: to_u32(response.counts.near),
                contained: to_u32(response.counts.contained),
                partial: to_u32(response.counts.partial),
                semantic: to_u32(response.counts.semantic),
            },
            total: to_u32(response.total),
            limit: to_u32(response.results),
            clamped: response.clamped,
            chunks_checked: to_u32(response.chunks_checked),
            chunks_skipped_short: to_u32(response.chunks_skipped_short),
            indexing: status.state == ProjectState::Indexing || status.queued,
            pending: to_u32(status.pending),
        })
    }

    async fn search(&self, params: SearchParams) -> Result<SearchOutput, RpcError> {
        self.search_with_graph(params, None)
            .await
            .map(|(output, _, _)| output)
    }

    async fn search_with_graph(
        &self,
        params: SearchParams,
        graph_budgets: Option<bars_core::graph::Budgets>,
    ) -> Result<
        (
            SearchOutput,
            Option<bars_protocol::GraphReply>,
            Option<String>,
        ),
        RpcError,
    > {
        let project = self.usable(&params.target)?;
        let status = self.worker.status(&project).map_err(worker_error)?;
        if status.state == ProjectState::ReindexRequired {
            return Err(errors::reindex_required());
        }
        let settings = self.settings(&project)?;
        history::applied_field(
            "models",
            serde_json::json!({"embed":settings.embed.model,"rerank":settings.rerank.model,"rerank_requested":settings.rerank.enabled}),
        );
        // Скачивание и загрузка встроенной модели поиск не ждёт: он сразу отвечает `model_loading`.
        match self
            .worker
            .models()
            .embed_readiness(&settings)
            .map_err(errors::embed_error)?
        {
            Readiness::Ready => {}
            Readiness::Loading { percent } => return Err(errors::model_loading(percent)),
            Readiness::Failed(message) => {
                return Err(RpcError::bars(ErrorKind::ProviderError, message));
            }
        }
        let mut embedder = self
            .worker
            .models()
            .embedder(&settings)
            .map_err(errors::embed_error)?;
        let request = SearchRequest {
            query: params.query,
            modules: params.modules,
            files: params.files,
            results: params.limit.map(|value| value as usize),
            candidates: params.candidates.map(|value| value as usize),
            rerank_candidates: None,
            min_score: params.min_score,
        };
        // «Новые темы»: допуск и область определяются до поиска, чтобы очистка списка во время
        // поиска отклонила запоздавшее наблюдение. Диагностика и CLI спроса не создают.
        let context = crate::new_topics::current_context();
        let collecting = context.purpose == crate::new_topics::Purpose::Request
            && matches!(
                context.origin,
                crate::new_topics::Origin::Window | crate::new_topics::Origin::Mcp
            );
        let generation = if collecting {
            self.topics.admit(&project).await
        } else {
            None
        };
        let topic_source = generation.map(|generation| {
            (
                generation,
                context
                    .request_id
                    .clone()
                    .unwrap_or_else(|| self.topics.next_request_id(context.origin)),
                bars_core::new_topics::EmbeddingSpace::from_fingerprint(
                    &bars_core::models::Embedder::fingerprint(&embedder).clone(),
                ),
                request.query.clone(),
                topic_scope(&request.modules, &request.files),
                project.clone(),
            )
        });
        // Замок держится на всё время поиска: подмена индекса при полной переиндексации ждёт,
        // пока поиски дочитают (`worker.md`, «Индекс проекта в воркере»).
        let gate = self
            .worker
            .gate(&project)
            .map_err(worker_error)?
            .read_owned()
            .await;
        // Rerank-модель, пока она скачивается или грузится, поиск тоже не ждёт: выдача идёт без неё.
        let mut skipped_rerank = None;
        let mut reranker = None;
        if settings.rerank.enabled {
            match self.worker.models().rerank_readiness(&settings) {
                Readiness::Ready => reranker = Some(self.worker.models().reranker(&settings)),
                Readiness::Loading { percent } => {
                    history::applied_field(
                        "rerank_fallback_reason",
                        serde_json::json!("model_loading"),
                    );
                    skipped_rerank = Some(format!(
                        "rerank-модель ещё скачивается или грузится ({percent} %): выдача без rerank"
                    ));
                }
                Readiness::Failed(message) => {
                    history::applied_field(
                        "rerank_fallback_reason",
                        serde_json::json!("model_unavailable"),
                    );
                    skipped_rerank = Some(message);
                }
            }
        }
        let response = tokio::task::spawn_blocking(move || {
            let _gate = gate;
            let index = Index::open(&index_path(&project), Journal::Wal)?;
            // Нет rerank-модели или она не отработала: выдача идёт без неё, причина — в ответе.
            let reranker = reranker.as_mut().map(|model| model as &mut dyn Reranker);
            let mut related = None;
            let mut fallback = None;
            let result = if let Some(budgets) = graph_budgets {
                match bars_core::graph::store::load(index.conn()) {
                    Ok(data) => {
                        let mut expand = |index: &Index, seeds: &[i64], limit: usize| {
                            let (ids, reply) = bars_core::graph::retrieval::expand(
                                index,
                                &data,
                                &request,
                                seeds,
                                limit,
                                budgets.clone(),
                            )?;
                            related = Some(reply);
                            Ok(ids)
                        };
                        bars_core::search::search_expanded(
                            &index,
                            &mut embedder,
                            reranker,
                            &settings.search,
                            &request,
                            Some(&mut expand),
                        )
                    }
                    Err(error) => {
                        fallback = Some(error.to_string());
                        search(&index, &mut embedder, reranker, &settings.search, &request)
                    }
                }
            } else {
                search(&index, &mut embedder, reranker, &settings.search, &request)
            };
            result.map(|result| (result, related, fallback))
        })
        .await
        .map_err(internal)?
        .map_err(search_error)?;
        let (mut response, related, fallback) = response;
        if response.rerank_error.is_some() {
            history::applied_field(
                "rerank_fallback_reason",
                serde_json::json!("rerank_execution_failed"),
            );
        }
        let query_vector = response.query_vector.take();
        let rerank_error = response.rerank_error.or(skipped_rerank);
        let output = SearchOutput {
            results: response.hits.into_iter().map(hit).collect(),
            applied: applied(response.applied),
            indexing: status.state == ProjectState::Indexing || status.queued,
            pending: to_u32(status.pending),
            rerank_error,
        };
        // Только успешный поиск с нулём результатов при готовом индексе без известной ошибки обновления
        // (NT-02): индексация, очередь и техническая ошибка пробелом не считаются.
        if let Some((generation, event_id, space, text, scope, project)) = topic_source
            && output.results.is_empty()
            && related.is_none()
            && status.state == ProjectState::Ready
            && !status.queued
            && status.pending == 0
            && status.last_error.is_none()
        {
            let source = match context.origin {
                crate::new_topics::Origin::Mcp => bars_core::new_topics::Source::Mcp,
                _ => bars_core::new_topics::Source::Window,
            };
            self.topics.submit(
                project,
                bars_core::new_topics::Observation {
                    event_id,
                    generation,
                    text,
                    scope,
                    source,
                    reason: bars_core::new_topics::Reason::EmptyResult,
                    params: serde_json::to_string(&output.applied).unwrap_or_default(),
                    index_stamp: status.updated_at.map(index_token),
                    vector: query_vector.map(|vector| (space, vector)),
                    at: crate::mcp_history::now(),
                },
            );
        }
        Ok((output, related, fallback))
    }

    /// Документ по пути из вызова: текст и корень проекта для заголовка.
    fn document(&self, target: &Target, path: &str) -> Result<(String, Settings), RpcError> {
        let project = self.usable(target)?;
        let settings = self.settings(&project)?;
        let file = resolve_doc_path(&project, Path::new(&settings.docs_root), path)
            .map_err(doc_path_error)?;
        history::applied_field("path", serde_json::json!(file.display().to_string()));
        history::applied_field("range", serde_json::json!("whole_document"));
        let text = read_utf8_path(&file).map_err(read_error)?;
        Ok((text, settings))
    }

    fn outline(&self, params: OutlineParams) -> Result<OutlineOutput, RpcError> {
        let (text, _) = self.document(&params.target, &params.path)?;
        let under = match (params.under, params.under_line) {
            (Some(_), Some(_)) => {
                return Err(RpcError::bars(
                    ErrorKind::InvalidParams,
                    "укажите либо under, либо under_line",
                ));
            }
            (Some(chain), None) => Some(Under::Chain(chain)),
            (None, Some(line)) => Some(Under::Line(line as usize)),
            (None, None) => None,
        };
        let query = OutlineQuery {
            levels: params
                .levels
                .iter()
                .map(|&level| usize::from(level))
                .collect(),
            under,
        };
        let headings = outline(&text, &query).map_err(outline_error)?;
        if let Some(anchor) = history::outline_anchor(&text, &query) {
            history::applied_field("anchor", anchor);
        }
        Ok(OutlineOutput {
            headings: headings
                .into_iter()
                .map(|heading| Heading {
                    line: to_u32(heading.line),
                    // Уровень заведомо 1…6.
                    level: heading.level as u8,
                    text: heading.text,
                })
                .collect(),
        })
    }

    fn read(&self, params: ReadParams) -> Result<ReadOutput, RpcError> {
        let (text, settings) = self.document(&params.target, &params.path)?;
        let title = chunk_document(&params.path, &text, &settings.chunking).title;
        Ok(ReadOutput { title, text })
    }

    async fn projects(&self) -> Result<ProjectsOutput, RpcError> {
        let mut projects = Vec::new();
        for snapshot in self.worker.projects() {
            projects.push(self.project_info(snapshot).await);
        }
        Ok(ProjectsOutput { projects })
    }

    async fn project_info(&self, snapshot: ProjectSnapshot) -> ProjectInfo {
        let modules = if snapshot.state == ProjectState::Unavailable {
            Vec::new()
        } else {
            self.modules(&snapshot.path).await.unwrap_or_default()
        };
        ProjectInfo {
            path: snapshot.path.display().to_string(),
            name: snapshot.name,
            docs_root: snapshot.docs_root,
            state: snapshot.state,
            modules,
        }
    }

    /// Модули проекта и число документов в индексе. Проект без индекса — без модулей.
    async fn modules(&self, project: &Path) -> Option<Vec<ModuleInfo>> {
        if !index_path(project).exists() {
            return None;
        }
        let gate = self.worker.gate(project).ok()?.read_owned().await;
        let project = project.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let _gate = gate;
            let index = Index::open(&index_path(&project), Journal::Wal).ok()?;
            let mut statement = index
                .conn()
                .prepare("SELECT module, count(*) FROM documents GROUP BY module ORDER BY module")
                .ok()?;
            let rows = statement
                .query_map([], |row| {
                    Ok(ModuleInfo {
                        name: row.get(0)?,
                        documents: row.get(1)?,
                    })
                })
                .ok()?;
            rows.collect::<Result<Vec<_>, _>>().ok()
        })
        .await
        .ok()
        .flatten()
    }

    fn status(&self, params: StatusParams) -> Result<StatusOutput, RpcError> {
        let project = self.resolve(&params.target)?;
        let status = self.worker.status(&project).map_err(worker_error)?;
        Ok(StatusOutput {
            path: project.display().to_string(),
            state: status.state,
            updated_at: status.updated_at.map(rfc3339),
            pending: to_u32(status.pending),
            model: ModelStatus {
                index: status.model_index,
                settings: status.model_settings.unwrap_or_default(),
            },
            last_error: status.last_error,
            watching: status.watching,
            graph: None,
        })
    }

    fn reindex(&self, params: ReindexParams) -> Result<AcceptedOutput, RpcError> {
        let project = self.resolve(&params.target)?;
        let rebuild = params.full || params.accept_model_change;
        let (id, merged, kind) = if rebuild {
            self.worker
                .reindex_receipt(&project, params.full, params.accept_model_change)
        } else {
            self.worker.scan_receipt(&project)
        }
        .map_err(worker_error)?;
        history::applied_field("operation_id", serde_json::json!(id));
        history::applied_field("merged", serde_json::json!(merged));
        history::applied_field(
            "job_kind",
            serde_json::json!(match kind {
                JobKind::Rebuild => "rebuild",
                JobKind::Check =>
                    if rebuild {
                        "check"
                    } else {
                        "scan"
                    },
                JobKind::Files => "files",
            }),
        );
        Ok(AcceptedOutput { accepted: true })
    }

    async fn register(&self, params: RegisterParams) -> Result<RegisterOutput, RpcError> {
        let registered = self
            .worker
            .register(Path::new(&params.path))
            .map_err(worker_error)?;
        let snapshot = self
            .worker
            .projects()
            .into_iter()
            .find(|snapshot| snapshot.path == registered.path)
            .ok_or_else(|| {
                RpcError::standard(
                    INTERNAL_ERROR,
                    "проект не найден в реестре после регистрации",
                )
            })?;
        Ok(RegisterOutput {
            project: self.project_info(snapshot).await,
        })
    }

    fn unregister(&self, params: UnregisterParams) -> Result<UnregisterOutput, RpcError> {
        let project = self.resolve(&params.target)?;
        self.worker
            .unregister(&project.to_string_lossy())
            .map_err(worker_error)?;
        Ok(UnregisterOutput { removed: true })
    }

    fn session_start(&self, params: SessionStartParams) -> Result<AcceptedOutput, RpcError> {
        self.worker
            .session_start(Path::new(&params.cwd))
            .map_err(worker_error)?;
        Ok(AcceptedOutput { accepted: true })
    }

    fn install_env(&self) -> Result<&InstallEnv, RpcError> {
        self.worker.install_env().ok_or_else(|| {
            RpcError::standard(
                INTERNAL_ERROR,
                "не удалось определить домашнюю папку или расположение exe bars",
            )
        })
    }

    async fn install(&self, params: InstallParams) -> Result<InstallOutput, RpcError> {
        let env = self.install_env()?.clone();
        let done = tokio::task::spawn_blocking(move || {
            let result = clients::install(&env, params.client).map(Some);
            clients::report(params.client, "установлено", &result);
            result.map(|done| done.expect("install вернул результат"))
        })
        .await
        .map_err(internal)?
        .map_err(internal)?;
        Ok(InstallOutput {
            changed_files: paths(done.changed_files),
            needs_hook_approval: done.needs_hook_approval,
        })
    }

    async fn uninstall(&self, params: UninstallParams) -> Result<UninstallOutput, RpcError> {
        let env = self.install_env()?.clone();
        let changed = tokio::task::spawn_blocking(move || {
            let result = clients::uninstall(&env, params.client).map(|changed_files| {
                Some(clients::Installed {
                    changed_files,
                    hook_changed: false,
                    needs_hook_approval: false,
                })
            });
            clients::report(params.client, "удалено", &result);
            result.map(|done| done.expect("uninstall вернул результат").changed_files)
        })
        .await
        .map_err(internal)?
        .map_err(internal)?;
        Ok(UninstallOutput {
            changed_files: paths(changed),
        })
    }

    fn index_pause(&self) -> PausedOutput {
        self.worker.pause();
        PausedOutput { paused: true }
    }

    fn index_resume(&self) -> PausedOutput {
        self.worker.resume();
        PausedOutput { paused: false }
    }

    fn index_cancel(&self, params: IndexCancelParams) -> Result<CancelOutput, RpcError> {
        let project = self.worker.resolve(&params.project).map_err(worker_error)?;
        let cancelled = self.worker.cancel(&project).map_err(worker_error)?;
        Ok(CancelOutput { cancelled })
    }

    /// Ход индексации, очередь и проблемы проектов: то, что показывают страница индексации и трей.
    fn activity(&self) -> ActivityOutput {
        let names: std::collections::HashMap<PathBuf, String> = self
            .worker
            .projects()
            .into_iter()
            .map(|snapshot| (snapshot.path, snapshot.name))
            .collect();
        let name_of = |path: &Path| names.get(path).cloned().unwrap_or_default();
        ActivityOutput {
            paused: self.worker.is_paused(),
            running: self.worker.progress().map(|progress| RunningJob {
                embed_device: progress.embed_device,
                attempt: to_u32(progress.attempt),
                project: progress.project.display().to_string(),
                name: name_of(&progress.project),
                kind: job_kind(progress.kind),
                priority: job_priority(progress.priority),
                docs_total: to_u32(progress.docs_total),
                docs_done: to_u32(progress.docs_done),
                chunks_done: to_u32(progress.chunks_done),
                chunks_per_second: progress.chunks_per_second,
                elapsed_seconds: u32::try_from(progress.elapsed.as_secs()).unwrap_or(u32::MAX),
                current_file: progress.current_file,
            }),
            queue: self
                .worker
                .queued()
                .into_iter()
                .map(|job| QueuedJob {
                    project: job.project.display().to_string(),
                    name: name_of(&job.project),
                    kind: job_kind(job.kind),
                    priority: job_priority(job.priority),
                    files: to_u32(job.files),
                })
                .collect(),
            problems: self
                .worker
                .problems()
                .into_iter()
                .map(|issues| ProjectProblems {
                    rebuild_not_applied: issues.rebuild_not_applied,
                    project: issues.project.display().to_string(),
                    name: name_of(&issues.project),
                    files: issues
                        .files
                        .into_iter()
                        .map(|problem| FileProblem {
                            path: problem.path,
                            reason: problem.reason,
                            stage: problem.stage.into(),
                            attempts: to_u32(problem.attempts),
                            previous_version: problem.previous_version,
                        })
                        .collect(),
                    last_error: issues.last_error,
                })
                .collect(),
        }
    }

    /// Документы проекта из индекса. Индекс читается без модели, поэтому дерево видно и в
    /// состоянии `reindex_required` (Т-25).
    async fn tree(&self, params: TreeParams) -> Result<TreeOutput, RpcError> {
        let project = self.usable(&params.target)?;
        let docs_root = self.settings(&project)?.docs_root;
        let documents = if index_path(&project).exists() {
            let gate = self
                .worker
                .gate(&project)
                .map_err(worker_error)?
                .read_owned()
                .await;
            tokio::task::spawn_blocking(move || {
                let _gate = gate;
                let index = Index::open(&index_path(&project), Journal::Wal)?;
                let mut statement = index.conn().prepare(
                    "SELECT d.path, d.module, d.title, \
                            (SELECT count(*) FROM chunks c WHERE c.document_id = d.id) \
                     FROM documents d ORDER BY d.path",
                )?;
                let rows = statement.query_map([], |row| {
                    Ok(TreeDocument {
                        path: row.get(0)?,
                        module: row.get(1)?,
                        title: row.get(2)?,
                        chunks: row.get(3)?,
                    })
                })?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(bars_core::storage::StorageError::from)
            })
            .await
            .map_err(internal)?
            .map_err(internal)?
        } else {
            Vec::new()
        };
        Ok(TreeOutput {
            docs_root,
            documents,
        })
    }

    // --- методы окна: настройки, модели, ключи, клиенты ---

    fn settings_get(&self) -> Result<SettingsOutput, RpcError> {
        let loaded = bars_core::settings::UserSettings::load(self.worker.bars_home())
            .map_err(|error| RpcError::standard(INTERNAL_ERROR, error.to_string()))?;
        Ok(SettingsOutput {
            file: bars_core::settings::user_settings_path(self.worker.bars_home())
                .display()
                .to_string(),
            settings: settings_io::to_doc(&loaded.value),
            warnings: loaded.warnings.iter().map(ToString::to_string).collect(),
        })
    }

    /// Сохраняет настройки и ставит все проекты на проверку: проекты, чей индекс построен другой
    /// моделью, перейдут в `reindex_required`, и окно покажет диалог согласия (Т-25). Эмбеддинг
    /// при этом не запускается.
    fn settings_save(&self, params: SettingsSaveParams) -> Result<SavedOutput, RpcError> {
        settings_io::save_user_settings(self.worker.bars_home(), &params.settings)
            .map_err(save_error)?;
        self.worker.refresh_model_lifecycle();
        self.worker.check_all();
        Ok(SavedOutput { saved: true })
    }

    fn models_info(&self) -> ModelsInfoOutput {
        let dir = self.worker.models_dir();
        let downloading = self.worker.downloading();
        let download_errors = self.worker.download_errors();
        let runtime = self.worker.model_runtime();
        let rerank_accelerator = if runtime
            .as_ref()
            .is_some_and(|r| r.rerank.actual.as_deref() == Some("cpu"))
        {
            bars_core::models::Accelerator::Cpu
        } else {
            bars_core::settings::UserSettings::load(self.worker.bars_home())
                .ok()
                .map(|s| s.value.rerank.accelerator)
                .unwrap_or(bars_core::models::Accelerator::Auto)
        };
        let info = |id: &str, dim: usize, backend: &str, warnings: Vec<String>| ModelInfo {
            id: id.to_string(),
            dim: to_u32(dim),
            backend: backend.to_string(),
            warnings,
            downloaded: if id == bars_core::models::BGE_RERANKER_V2_M3_ID {
                bars_core::models::reranker_is_downloaded(&dir, rerank_accelerator)
            } else {
                bars_core::models::is_downloaded(&dir, id)
            },
            downloading: downloading.contains(id),
            download_percent: if id == bars_core::models::BGE_RERANKER_V2_M3_ID {
                Some(bars_core::models::reranker_download_percent(
                    &dir,
                    rerank_accelerator,
                ))
            } else {
                bars_core::models::download_percent(&dir, id)
            },
            download_error: download_errors.get(id).cloned(),
        };
        ModelsInfoOutput {
            runtime,
            embed: bars_core::models::builtin_models()
                .iter()
                .map(|model| {
                    let backend = match model.backend {
                        bars_core::models::Backend::Onnx => "onnx",
                        bars_core::models::Backend::Gguf => "gguf",
                        _ => "candle",
                    };
                    info(
                        model.id,
                        model.dim,
                        backend,
                        model.warnings.iter().map(ToString::to_string).collect(),
                    )
                })
                .collect(),
            rerank: vec![info(
                bars_core::models::BGE_RERANKER_V2_M3_ID,
                0,
                "onnx",
                Vec::new(),
            )],
            models_dir: dir.display().to_string(),
        }
    }

    fn model_download(&self, params: ModelDownloadParams) -> Result<AcceptedOutput, RpcError> {
        let reranker = params.id == bars_core::models::BGE_RERANKER_V2_M3_ID;
        if !reranker && bars_core::models::builtin_model(&params.id).is_none() {
            return Err(RpcError::bars(
                ErrorKind::InvalidParams,
                format!("модели `{}` нет во встроенном каталоге", params.id),
            ));
        }
        self.worker.start_download(&params.id, reranker);
        Ok(AcceptedOutput { accepted: true })
    }

    fn secret_set(&self, params: SecretSetParams) -> Result<SecretOutput, RpcError> {
        let name = params.name.trim();
        if name.is_empty() || params.value.is_empty() {
            return Err(RpcError::bars(
                ErrorKind::InvalidParams,
                "нужны имя записи и ключ",
            ));
        }
        self.worker
            .secrets()
            .set(name, &params.value)
            .map_err(|error| RpcError::bars(ErrorKind::InvalidParams, error.to_string()))?;
        Ok(SecretOutput { present: true })
    }

    fn secret_status(&self, params: SecretStatusParams) -> Result<SecretOutput, RpcError> {
        let present = self
            .worker
            .secrets()
            .get(&params.name)
            .map_err(|error| RpcError::standard(INTERNAL_ERROR, error.to_string()))?
            .is_some();
        Ok(SecretOutput { present })
    }

    fn clients_status(&self) -> Result<ClientsStatusOutput, RpcError> {
        let env = self.install_env()?;
        let clients = [InstallClient::ClaudeCode, InstallClient::Codex]
            .into_iter()
            .map(|client| {
                let state = clients::state(env, client);
                ClientStatus {
                    client,
                    installed: state.installed,
                    stale: state.stale,
                    files: paths(state.files),
                    // Одобрен ли хук в Codex, bars не знает: напоминание остаётся, пока установлено.
                    needs_hook_approval: client == InstallClient::Codex && state.installed,
                }
            })
            .collect();
        Ok(ClientsStatusOutput { clients })
    }

    fn instruction(&self) -> Result<InstructionOutput, RpcError> {
        let env = self.install_env()?;
        let (cli, mcp) = (
            env.cli_exe.display().to_string(),
            env.mcp_exe.display().to_string(),
        );
        Ok(InstructionOutput {
            text: bars_protocol::agent_install_text(&cli, &mcp),
            mcp_config: bars_protocol::mcp_config_json(&mcp),
        })
    }

    fn set_docs_root(&self, params: SetDocsRootParams) -> Result<AcceptedOutput, RpcError> {
        let project = self.worker.resolve(&params.project).map_err(worker_error)?;
        let root = params.docs_root.trim();
        if root.is_empty() {
            return Err(RpcError::bars(
                ErrorKind::InvalidParams,
                "корень документации не может быть пустым",
            ));
        }
        self.worker
            .set_docs_root(&project, root)
            .map_err(worker_error)?;
        Ok(AcceptedOutput { accepted: true })
    }

    fn check_all(&self) -> AcceptedOutput {
        self.worker.check_all();
        AcceptedOutput { accepted: true }
    }

    fn autostart_output(state: crate::autostart::AutostartState) -> AutostartOutput {
        AutostartOutput {
            supported: state.supported,
            enabled: state.enabled,
            stale: state.stale,
            command: state.command,
        }
    }

    fn autostart_get(&self) -> Result<AutostartOutput, RpcError> {
        let state = self.worker.autostart().state().map_err(internal)?;
        Ok(Self::autostart_output(state))
    }

    fn autostart_set(&self, params: AutostartSetParams) -> Result<AutostartOutput, RpcError> {
        let autostart = self.worker.autostart();
        if !autostart.state().map_err(internal)?.supported {
            return Err(RpcError::bars(
                ErrorKind::InvalidParams,
                "автозапуск на этой ОС не поддерживается",
            ));
        }
        let state = autostart.set(params.enabled).map_err(internal)?;
        Ok(Self::autostart_output(state))
    }

    pub async fn dispatch(&self, call: Call) -> Result<Value, RpcError> {
        if let Call::ProjectDelete(params) = call {
            if !params.confirmed || !Path::new(&params.project).is_absolute() {
                return Err(RpcError::bars(
                    ErrorKind::InvalidParams,
                    "удаление требует точного пути и явного подтверждения",
                ));
            }
            // БД новых тем закрывается до удаления файлов и не открывается заново, пока оно идёт.
            let project_path = PathBuf::from(&params.project);
            self.topics.begin_delete(&project_path);
            let deleted = self.worker.delete_project(&params.project).await;
            self.topics.end_delete(&project_path);
            deleted.map_err(worker_error)?;
            return Ok(output_value(&UnregisterOutput { removed: true }));
        }
        if let Call::GraphBuild(params) = call {
            // Задание само удерживает owned request guard до конца. Повторный read
            // под этим guard мог бы зависнуть за ожидающим writer удаления проекта.
            let project = self.resolve(&params.target)?;
            return self
                .worker
                .graph_build(&project)
                .await
                .map(|out| output_value(&out))
                .map_err(graph::graph_error);
        }
        // Удаление ждёт запросы, уже открывшие индекс или настройки; новые ждут очистки.
        let _request = self.worker.request_guard().await;
        match call {
            Call::GraphSearch(params) => self
                .graph_search(params)
                .await
                .map(|out| output_value(&out)),
            Call::GraphQuery(params) => {
                self.graph_query(params).await.map(|out| output_value(&out))
            }
            Call::GraphStatus(params) => self
                .graph_status(params)
                .await
                .map(|out| output_value(&out)),
            Call::GraphBuild(_) => unreachable!(),
            Call::GraphCancel(params) => {
                let project = self.resolve(&params.target)?;
                self.worker
                    .graph_cancel(&project, &params.job_id)
                    .map(|cancelled| output_value(&CancelOutput { cancelled }))
                    .map_err(graph::graph_error)
            }
            Call::ProjectDelete(_) => unreachable!(),
            // `hello` обрабатывает цикл соединения.
            Call::Hello(_) => Err(RpcError::standard(
                INTERNAL_ERROR,
                "hello не доходит до обработчика",
            )),
            Call::Search(params) => {
                let start = std::time::Instant::now();
                let result = self.search(params).await;
                match &result {
                    Ok(out) if out.rerank_error.is_some() => {
                        log::warn!("Поиск выполнен без rerank; подробности — в ответе поиска")
                    }
                    Ok(out) => log::debug!(
                        "Поиск завершён: {} результатов, {} мс",
                        out.results.len(),
                        start.elapsed().as_millis()
                    ),
                    Err(error) => log::warn!(
                        "Поиск не выполнен: код {}; подробности — в ответе поиска",
                        error.code
                    ),
                }
                result.map(|out| output_value(&out))
            }
            Call::Similar(params) => self.similar(params).await.map(|out| output_value(&out)),
            Call::Duplicates(params) => self.duplicates(params).await.map(|out| output_value(&out)),
            Call::Outline(params) => self.outline(params).map(|out| output_value(&out)),
            Call::Read(params) => self.read(params).map(|out| output_value(&out)),
            Call::Projects(_) => self.projects().await.map(|out| output_value(&out)),
            Call::Status(params) => {
                let graph = self
                    .graph_status(bars_protocol::GraphStatusParams {
                        target: params.target.clone(),
                    })
                    .await?;
                let mut out = self.status(params)?;
                out.graph = Some(graph);
                Ok(output_value(&out))
            }
            Call::Reindex(params) => self.reindex(params).map(|out| output_value(&out)),
            Call::Register(params) => self.register(params).await.map(|out| output_value(&out)),
            Call::Unregister(params) => self.unregister(params).map(|out| output_value(&out)),
            Call::SessionStart(params) => self.session_start(params).map(|out| output_value(&out)),
            Call::Install(params) => self.install(params).await.map(|out| output_value(&out)),
            Call::Uninstall(params) => self.uninstall(params).await.map(|out| output_value(&out)),
            Call::IndexPause(_) => Ok(output_value(&self.index_pause())),
            Call::IndexResume(_) => Ok(output_value(&self.index_resume())),
            Call::IndexCancel(params) => self.index_cancel(params).map(|out| output_value(&out)),
            Call::Activity(_) => Ok(output_value(&self.activity())),
            Call::OperationEvents(params) => {
                Ok(output_value(&self.worker.operation_events(&params)))
            }
            Call::OperationRetry(params) => self
                .worker
                .retry_operation(&params)
                .map(|()| output_value(&AcceptedOutput { accepted: true }))
                .map_err(worker_error),
            Call::Tree(params) => self.tree(params).await.map(|out| output_value(&out)),
            Call::SettingsGet(_) => self.settings_get().map(|out| output_value(&out)),
            Call::SettingsSave(params) => self.settings_save(params).map(|out| output_value(&out)),
            Call::ModelsInfo(_) => Ok(output_value(&self.models_info())),
            Call::ModelDownload(params) => {
                self.model_download(params).map(|out| output_value(&out))
            }
            Call::SecretSet(params) => self.secret_set(params).map(|out| output_value(&out)),
            Call::SecretStatus(params) => self.secret_status(params).map(|out| output_value(&out)),
            Call::ClientsStatus(_) => self.clients_status().map(|out| output_value(&out)),
            Call::Instruction(_) => self.instruction().map(|out| output_value(&out)),
            Call::SetDocsRoot(params) => self.set_docs_root(params).map(|out| output_value(&out)),
            Call::CheckAll(CheckAllParams {}) => Ok(output_value(&self.check_all())),
            Call::NewTopicsList(params) => self
                .new_topics_list(params)
                .await
                .map(|out| output_value(&out)),
            Call::NewTopicsGet(params) => self
                .new_topics_get(params)
                .await
                .map(|out| output_value(&out)),
            Call::NewTopicsMarkMissing(params) => self
                .new_topics_mark_missing(params)
                .await
                .map(|out| output_value(&out)),
            Call::NewTopicsCheck(params) => self
                .new_topics_check(params)
                .await
                .map(|out| output_value(&out)),
            Call::NewTopicsSetStatus(params) => self
                .new_topics_set_status(params)
                .await
                .map(|out| output_value(&out)),
            Call::NewTopicsRename(params) => self
                .new_topics_rename(params)
                .await
                .map(|out| output_value(&out)),
            Call::NewTopicsDelete(params) => self
                .new_topics_delete(params)
                .await
                .map(|out| output_value(&out)),
            Call::NewTopicsClear(params) => self
                .new_topics_clear(params)
                .await
                .map(|out| output_value(&out)),
            Call::NewTopicsSettings(params) => self
                .new_topics_settings(params)
                .await
                .map(|out| output_value(&out)),
            Call::AutostartGet(AutostartGetParams {}) => {
                self.autostart_get().map(|out| output_value(&out))
            }
            Call::AutostartSet(params) => self.autostart_set(params).map(|out| output_value(&out)),
        }
    }
}

impl bars_protocol::Handler for WorkerHandler {
    async fn handle_observed(
        &self,
        session: &str,
        client: ClientKind,
        call: Call,
        context: Option<Value>,
    ) -> Result<Value, RpcError> {
        self.observed(session, client, call, context).await
    }
    async fn handle_session(
        &self,
        session: &str,
        client: ClientKind,
        call: Call,
    ) -> Result<Value, RpcError> {
        self.observed(session, client, call, None).await
    }
    async fn handle(&self, _client: ClientKind, call: Call) -> Result<Value, RpcError> {
        self.handle_plain(_client, call).await
    }
}
fn internal(error: impl std::fmt::Display) -> RpcError {
    RpcError::standard(INTERNAL_ERROR, error.to_string())
}

/// Каноническая область обращения для «Новых тем»: `project` без фильтров либо отсортированные
/// модули и файлы. Запрос в области не становится запросом ко всему проекту.
fn topic_scope(modules: &[String], files: &[String]) -> String {
    if modules.is_empty() && files.is_empty() {
        return "project".to_string();
    }
    let canonical = |items: &[String]| {
        let mut items = items.to_vec();
        items.sort();
        items.dedup();
        items.join(",")
    };
    format!("m={};f={}", canonical(modules), canonical(files))
}

fn save_error(error: SaveError) -> RpcError {
    match error {
        SaveError::Invalid(message) => RpcError::bars(ErrorKind::InvalidParams, message),
        other => RpcError::standard(INTERNAL_ERROR, other.to_string()),
    }
}

fn job_kind(kind: JobKind) -> ProtocolJobKind {
    match kind {
        JobKind::Rebuild => ProtocolJobKind::Rebuild,
        JobKind::Check => ProtocolJobKind::Check,
        JobKind::Files => ProtocolJobKind::Files,
    }
}

fn job_priority(priority: Priority) -> JobPriority {
    match priority {
        Priority::Explicit => JobPriority::Explicit,
        Priority::Watch => JobPriority::Watch,
        Priority::Periodic => JobPriority::Periodic,
    }
}

fn paths(files: Vec<PathBuf>) -> Vec<String> {
    files
        .into_iter()
        .map(|file| file.display().to_string())
        .collect()
}

fn to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn score_kind(kind: bars_core::search::ScoreKind) -> ScoreKind {
    match kind {
        bars_core::search::ScoreKind::Rerank => ScoreKind::Rerank,
        bars_core::search::ScoreKind::Cosine => ScoreKind::Cosine,
    }
}

fn hit(hit: bars_core::search::SearchHit) -> SearchHit {
    SearchHit {
        chunk_id: hit.public_id,
        path: hit.path,
        module: hit.module,
        title: hit.title,
        heading_chain: hit.heading_chain,
        first_line: to_u32(hit.first_line),
        last_line: to_u32(hit.last_line),
        text: hit.text,
        score: hit.score,
        score_kind: score_kind(hit.score_kind),
    }
}

fn duplicate_item(item: bars_core::search::DuplicateItem) -> DuplicateItem {
    use bars_core::search::DuplicateKind as Core;
    let differences = item.differences.map(|differences| DuplicateDifferences {
        changes: differences
            .changes
            .into_iter()
            .map(|change| DuplicateChange {
                first: change.first,
                second: change.second,
            })
            .collect(),
        changes_truncated: differences.changes_truncated,
        numbers_only_first: differences.numbers_only_first,
        numbers_only_second: differences.numbers_only_second,
        negations_first: to_u32(differences.negations.0),
        negations_second: to_u32(differences.negations.1),
    });
    DuplicateItem {
        kind: match item.kind {
            Core::Exact => DuplicateKind::Exact,
            Core::Near => DuplicateKind::Near,
            Core::Contained => DuplicateKind::Contained,
            Core::Partial => DuplicateKind::Partial,
            Core::Semantic => DuplicateKind::Semantic,
        },
        chunks: item
            .chunks
            .into_iter()
            .map(|chunk| DuplicateChunk {
                chunk_id: chunk.public_id,
                path: chunk.path,
                module: chunk.module,
                heading_chain: chunk.heading_chain,
                first_line: to_u32(chunk.first_line),
                last_line: to_u32(chunk.last_line),
            })
            .collect(),
        score: item.score,
        shared: item.shared,
        differences,
        repeated_block: item.repeated_block,
    }
}

fn applied(applied: bars_core::search::AppliedParams) -> AppliedParams {
    AppliedParams {
        results: to_u32(applied.results),
        candidates: to_u32(applied.candidates),
        rerank_candidates: to_u32(applied.rerank_candidates),
        min_score: applied.min_score,
        score_kind: score_kind(applied.score_kind),
        clamped: applied.clamped,
    }
}
