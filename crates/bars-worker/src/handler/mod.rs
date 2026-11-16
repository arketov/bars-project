//! Обработчик методов протокола поверх воркера и `bars-core` (`docs/architecture/protocol.md`).
//!
//! `hello`, права клиента и сброс `accept_model_change` у MCP делает цикл соединения в
//! `bars-protocol`; сюда доходят уже проверенные вызовы. Окно вызывает те же методы без канала (Т-20).

mod audits;
mod disk_read;
mod errors;
mod folder;
mod graph;
pub(crate) mod graph_cache;
mod history;
mod links;
mod project_filter;
mod query_slice;
mod session_info;
mod text_search;
mod topics;
mod worktree_view;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use bars_core::doc_path::resolve_doc_path;
use bars_core::indexing::chunking::document_title;
use bars_core::indexing::walk::read_utf8_path;
use bars_core::models::Reranker;
use bars_core::outline::{OutlineQuery, Under, outline};
use bars_core::search::{
    DuplicatesRequest, IndexView, SearchRequest, SimilarRequest, duplicates, search_in_view,
    similar,
};
use bars_core::settings::Settings;
use bars_core::storage::{Index, index_path};
use bars_protocol::{
    AcceptedOutput, ActivityOutput, AppliedParams, AutostartGetParams, AutostartOutput,
    AutostartSetParams, Call, CancelOutput, CheckAllParams, ClientKind, ClientStatus,
    ClientsStatusOutput, DocumentSource, DocumentState, DuplicateChange, DuplicateChunk,
    DuplicateCounts, DuplicateDifferences, DuplicateItem, DuplicateKind, DuplicatesOutput,
    DuplicatesParams, ErrorKind, FileProblem, Heading, INTERNAL_ERROR, IndexCancelParams,
    IndexRetryParams, InstallClient, InstallOutput, InstallParams, InstructionOutput,
    JobKind as ProtocolJobKind, JobPriority, METHOD_NOT_FOUND, ModelDownloadParams, ModelInfo,
    ModelStatus, ModelsInfoOutput, ModuleInfo, OutlineOutput, OutlineParams, PausedOutput,
    ProjectDocuments, ProjectInfo, ProjectProblems, ProjectState, ProjectsOutput, QueuedDocument,
    QueuedJob, ReadOutput, ReadParams, RegisterOutput, RegisterParams, ReindexParams, RetryOutput,
    RpcError, RunningJob, SavedOutput, ScoreKind, SearchHit, SearchOutput, SearchParams,
    SecretOutput, SecretSetParams, SecretStatusParams, SessionStartParams, SetDocsRootParams,
    SettingsOutput, SettingsSaveParams, SimilarOutput, SimilarParams, StatusOutput, StatusParams,
    Target, TreeDocument, TreeOutput, TreeParams, UninstallOutput, UninstallParams,
    UnregisterOutput, UnregisterParams, WorktreeStatus, output_value,
};
use serde_json::Value;

use crate::clients::{self, InstallEnv};
use crate::settings_io::{self, SaveError};
use crate::worker::{
    JobKind, Priority, ProjectSnapshot, Readiness, ViewSelector, Worker, WtSelector, WtSource,
    index_token, rfc3339,
};
use errors::{doc_path_error, outline_error, read_error, search_error, worker_error};

/// Сколько поиск ждёт загрузки обеих встроенных моделей в память вместе (ISS-105). Замер: bge-m3
/// грузится 3,5 с, rerank 5,6 с, загрузки идут по очереди, вместе около 9 с; предел оставляет
/// двукратный запас на холодный диск и инициализацию GPU и короче срока вызова MCP (55 с).
const MODEL_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

/// Как часто поиск смотрит, загрузилась ли модель.
const MODEL_POLL: std::time::Duration = std::time::Duration::from_millis(50);

/// Пометка выдачи без смысловой части: embed-модель не успела загрузиться за предел ожидания.
const SEMANTIC_UNAVAILABLE: &str = "смысловая близость не учтена: модель загружается; результаты подобраны только по совпадению слов, оценки близости и порога нет. Повторите поиск через несколько секунд";

/// Выполняет вызовы канала на воркере.
#[derive(Clone)]
pub struct WorkerHandler {
    /// Предел ожидания загрузки моделей поиском; в тестах короче.
    model_wait: std::time::Duration,
    pub keybindings: crate::keybindings::Store,
    worker: Worker,
    history: crate::mcp_history::History,
    topics: crate::new_topics::Collector,
    /// Единственный писатель индекса ссылок (`.bars/links.sqlite3`): вызовы окна идут по очереди.
    links_lock: std::sync::Arc<std::sync::Mutex<()>>,
    text_searches: text_search::ActiveSearches,
    /// Очередь запусков аудитов знаний: один аудит на проект.
    audits: crate::audits::Audits,
    /// Проекты, для которых расписание аудитов уже записало в журнал, что корня документации нет:
    /// запись одна на смену состояния (ISS-106).
    schedule_blocked:
        std::sync::Arc<std::sync::Mutex<std::collections::HashSet<std::path::PathBuf>>>,
    /// Близость документов последнего среза физического графа: проект, снимок графа, результат.
    physical_similarity: PhysicalSimilarityCache,
    document_representations: query_slice::RepresentationCache,
    graph_cache: std::sync::Arc<graph_cache::GraphCache>,
    /// Исключённые пути поиска по worktree: пересчитываются, когда изменился владелец или дельта.
    wt_excluded: worktree_view::ExcludedCache,
}

type PhysicalSimilarityCache = std::sync::Arc<
    std::sync::Mutex<
        Option<(
            std::path::PathBuf,
            String,
            std::sync::Arc<bars_core::graph::centroid::DocumentSimilarity>,
        )>,
    >,
>;

impl WorkerHandler {
    pub fn new(worker: Worker) -> Self {
        let graph_cache = worker.graph_cache();
        let history = crate::mcp_history::History::new(worker.bars_home().to_path_buf());
        history.start_maintenance();
        Self {
            model_wait: MODEL_WAIT,
            keybindings: crate::keybindings::Store::default(),
            worker,
            history,
            topics: crate::new_topics::Collector::new(),
            links_lock: Default::default(),
            text_searches: Default::default(),
            audits: Default::default(),
            schedule_blocked: Default::default(),
            physical_similarity: Default::default(),
            document_representations: Default::default(),
            graph_cache,
            wt_excluded: Default::default(),
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

    /// Проект вызова: явный `project` или ближайший к `cwd` зарегистрированный (Т-15). Вид
    /// (`wt`) здесь проверяется, но не возвращается: поиск и чтение по виду — шаги 6.3.x.
    fn resolve(&self, target: &Target) -> Result<PathBuf, RpcError> {
        self.resolve_view(target).map(|(path, _)| path)
    }

    /// Проект и вид вызова (`docs/features/projects/worktrees.md`, «Разрешение цели вызова»):
    /// `wt="."` — основной checkout; `wt` — имя или путь worktree проекта (нет такого —
    /// `worktree_not_found` со списком); без `wt` и при `cwd` в живом worktree владельца-проекта —
    /// этот worktree. Явный `project`, не равный владельцу копии, неявного вида не даёт.
    fn resolve_view(&self, target: &Target) -> Result<(PathBuf, ViewSelector), RpcError> {
        let wanted = target.wt();
        let cwd_hit = (target.project.is_none() || wanted.is_none())
            .then(|| self.worker.resolve_cwd(Path::new(&target.cwd)))
            .flatten();
        let path = if let Some(selector) = &target.project {
            self.worker.resolve(selector).map_err(worker_error)?
        } else {
            cwd_hit
                .as_ref()
                .map(|(path, _)| path.clone())
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
        let (view, source) = match wanted {
            Some(".") => (ViewSelector::Main, WtSource::Param),
            Some(value) => {
                let found = self
                    .worker
                    .find_worktree(&path, WtSelector::parse(value))
                    .map_err(|available| {
                        let name = self
                            .worker
                            .project_metadata(&path)
                            .map_or_else(|| path.display().to_string(), |(name, _)| name);
                        errors::worktree_not_found(&name, value, &available)
                    })?;
                (ViewSelector::Worktree(found), WtSource::Param)
            }
            None => match cwd_hit.filter(|(owner, _)| *owner == path) {
                Some((_, Some(reference))) => {
                    let found = self
                        .worker
                        .find_worktree(&path, WtSelector::Name(&reference.name))
                        .unwrap_or_else(|_| bars_core::worktree::LinkedWorktree {
                            name: reference.name.clone(),
                            admin_dir: reference.admin_dir.clone(),
                            path: reference.path.clone(),
                            view_root: reference.view_root.clone(),
                            branch: None,
                            locked: None,
                        });
                    (ViewSelector::Worktree(found), WtSource::Cwd)
                }
                _ => (ViewSelector::Main, WtSource::Main),
            },
        };
        history::applied_field(
            "resolved_project",
            serde_json::json!(path.display().to_string()),
        );
        history::applied_field(
            "resolved_project_name",
            serde_json::json!(self.worker.project_metadata(&path).map(|(name, _)| name)),
        );
        history::applied_field("resolved_wt", serde_json::json!(view.label()));
        history::applied_field("wt_source", serde_json::json!(source.name()));
        Ok((path, view))
    }

    /// Проект вызова, папка которого на месте.
    fn usable(&self, target: &Target) -> Result<PathBuf, RpcError> {
        self.usable_view(target).map(|(path, _)| path)
    }

    fn settings(&self, project: &Path) -> Result<Settings, RpcError> {
        Settings::load(self.worker.bars_home(), project)
            .map(|loaded| loaded.value)
            .map_err(|error| RpcError::standard(INTERNAL_ERROR, error.to_string()))
    }

    /// Чанки, близкие к чанку из выдачи поиска, по готовым векторам: модели не нужны.
    async fn similar(&self, params: SimilarParams) -> Result<SimilarOutput, RpcError> {
        let (project, view) = self.usable_view(&params.target)?;
        if let ViewSelector::Worktree(item) = &view {
            return Err(worktree_view::worktree_unsupported(&item.name, "similar"));
        }
        let status = self.worker.status(&project).map_err(worker_error)?;
        if status.state == ProjectState::ReindexRequired {
            return Err(errors::reindex_required());
        }
        let settings = self.settings(&project)?;
        let search_settings = settings.search.clone();
        let request = SimilarRequest {
            id: params.chunk_id,
            modules: params.modules,
            paths: params.paths,
            exclude_document: params.exclude_document,
            results: params.limit.map(|value| value as usize),
        };
        let project_gate = self.worker.gate(&project).map_err(worker_error)?;
        let gate = project_gate.clone().read_owned().await;
        let response = tokio::task::spawn_blocking(move || {
            let _gate = gate;
            let index = Index::open_read_only(&index_path(&project))?;
            similar(&index, &search_settings, &request)
        })
        .await
        .map_err(internal)?
        .map_err(search_error)?;
        Ok(SimilarOutput {
            source: similar_hit(response.source),
            results: response.hits.into_iter().map(similar_hit).collect(),
            limit: to_u32(response.results),
            clamped: response.clamped,
            indexing: status.state == ProjectState::Indexing || status.queued,
            pending: to_u32(status.pending),
        })
    }

    /// Агентский `like`: похожие разделы других документов и входящие ссылки одним вызовом.
    /// Источник — `путь:строки` из выдачи поиска или ID чанка.
    async fn like(
        &self,
        params: bars_protocol::LikeParams,
    ) -> Result<bars_protocol::LikeOutput, RpcError> {
        let (project, view) = self.usable_view(&params.target)?;
        // Worktree: `like` идёт по виду (дельта и владелец), ссылки — у владельца с пометкой.
        if let ViewSelector::Worktree(item) = &view {
            return self.like_in_worktree(project, item, params).await;
        }
        let status = self.worker.status(&project).map_err(worker_error)?;
        if status.state == ProjectState::ReindexRequired {
            return Err(errors::reindex_required());
        }
        let docs_root = self.settings(&project)?.docs_root;
        let source = params.source.trim().to_string();
        let chunk_id = match like_path_line(&source) {
            Some((path, line)) => {
                let gate = self
                    .worker
                    .gate(&project)
                    .map_err(worker_error)?
                    .read_owned()
                    .await;
                tokio::task::spawn_blocking(move || {
                    let _gate = gate;
                    let index = Index::open_read_only(&index_path(&project))?;
                    bars_core::search::chunk_public_at_line(&index, &docs_root, &path, line)
                })
                .await
                .map_err(internal)?
                .map_err(search_error)?
            }
            None => source,
        };
        let similar = self
            .similar(bars_protocol::SimilarParams {
                target: params.target.clone(),
                chunk_id,
                modules: Vec::new(),
                paths: params.paths,
                exclude_document: true,
                limit: params.limit,
            })
            .await?;
        let linked_from = self
            .linked_from(&params.target, &similar.source.path)
            .await?;
        Ok(bars_protocol::LikeOutput {
            source: similar.source,
            results: similar.results,
            limit: similar.limit,
            clamped: similar.clamped,
            linked_from,
            indexing: similar.indexing,
            pending: similar.pending,
            worktree_note: None,
        })
    }

    /// Документы, ссылающиеся на документ (входящие ссылки свежей сверки) — для агентского `like`.
    async fn linked_from(&self, target: &Target, path: &str) -> Result<Vec<String>, RpcError> {
        let project = self.usable(target)?;
        let docs = self.worker.docs_root(&project).map_err(worker_error)?;
        let stored = disk_read::links_document(&project, &docs, path)?;
        let lock = std::sync::Arc::clone(&self.links_lock);
        let sources = tokio::task::spawn_blocking(move || {
            let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
            let mut index = bars_core::link_index::LinkIndex::open(&project).map_err(internal)?;
            index
                .refresh_checked(&project, Path::new(&docs))
                .map_err(internal)?;
            let page = index.page(&stored, true, 0, 200).map_err(internal)?;
            Ok::<_, RpcError>(
                page.items
                    .iter()
                    .filter_map(|item| item["source"].as_str().map(str::to_string))
                    .collect::<std::collections::BTreeSet<_>>(),
            )
        })
        .await
        .map_err(internal)??;
        Ok(sources.into_iter().collect())
    }

    /// Повторы в области проекта по готовым текстам и векторам индекса: модели не нужны.
    async fn duplicates(&self, params: DuplicatesParams) -> Result<DuplicatesOutput, RpcError> {
        let (project, view) = self.usable_view(&params.target)?;
        if let ViewSelector::Worktree(item) = &view {
            return Err(worktree_view::worktree_unsupported(
                &item.name,
                "duplicates",
            ));
        }
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
            let index = Index::open_read_only(&index_path(&project))?;
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
        self.search_with_graph(params, None, false)
            .await
            .map(|(output, _, _)| output)
    }

    async fn search_with_graph(
        &self,
        params: SearchParams,
        graph_budgets: Option<bars_core::graph::Budgets>,
        require_rerank: bool,
    ) -> Result<
        (
            SearchOutput,
            Option<bars_protocol::GraphReply>,
            Option<String>,
        ),
        RpcError,
    > {
        let (project, view) = self.usable_view(&params.target)?;
        let status = self.worker.status(&project).map_err(worker_error)?;
        if status.state == ProjectState::ReindexRequired {
            return Err(errors::reindex_required());
        }
        // Поиск с `wt` или из cwd в worktree — первое обращение к виду: дельта строится лениво.
        // Дельта готова — владелец и дельта в одном поиске; строится или упала — ответ по владельцу
        // с пометкой (`worktrees.md`, «Поиск по worktree»).
        let wt_plan = match &view {
            ViewSelector::Worktree(item) => Some(self.plan_worktree_search(&project, item)),
            ViewSelector::Main => None,
        };
        let wt_merge = wt_plan
            .as_ref()
            .filter(|plan| plan.merge)
            .map(|plan| plan.info.name.clone());
        // Графа у вида нет: связанные источники по слиянию не добавляются.
        let graph_budgets = graph_budgets.filter(|_| wt_merge.is_none());
        let mut settings = self.settings(&project)?;
        // Проверка знаний требует rerank независимо от переключателя обычного поиска.
        if require_rerank {
            settings.rerank.enabled = true;
        }
        history::applied_field(
            "models",
            serde_json::json!({"embed":settings.embed.model,"rerank":settings.rerank.model,"rerank_requested":settings.rerank.enabled}),
        );
        // Скачивание встроенной модели поиск не ждёт: он сразу отвечает `model_loading`. Загрузку
        // уже скачанной модели в память (после выгрузки по таймеру) он ждёт до `model_wait`, дальше
        // идёт без смысловой части (ISS-105). Одновременные поиски делят одну заявку на загрузку
        // (`Loads::claim`), а ожидание асинхронное и воркер не блокирует. `wait_models: false`
        // («Искать по словам сейчас», OQ-280) — предел нулевой: грузящаяся модель не ждётся ни
        // embed, ни rerank, ответ идёт сразу с той же пометкой, что и по истечении предела.
        let wait = if params.wait_models == Some(false) {
            std::time::Duration::ZERO
        } else {
            self.model_wait
        };
        let deadline = tokio::time::Instant::now() + wait;
        let mut semantic = true;
        let mut rerank_requested = false;
        loop {
            match self
                .worker
                .models()
                .embed_readiness(&settings)
                .map_err(errors::embed_error)?
            {
                Readiness::Ready => break,
                Readiness::Failed(failure) => return Err(errors::builtin_model_error(failure)),
                Readiness::Loading { percent } if percent < 100 => {
                    return Err(errors::model_loading(percent));
                }
                Readiness::Loading { .. } => {
                    // Загрузка rerank-модели стартует вместе с embed, а не после неё.
                    if settings.rerank.enabled && !rerank_requested {
                        rerank_requested = true;
                        let _ = self.worker.models().rerank_readiness(&settings);
                    }
                    if tokio::time::Instant::now() >= deadline {
                        semantic = false;
                        history::applied_field(
                            "semantic_fallback_reason",
                            serde_json::json!("model_loading"),
                        );
                        break;
                    }
                    tokio::time::sleep(MODEL_POLL).await;
                }
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
            paths: params.paths,
            results: params.limit.map(|value| value as usize),
            candidates: params.candidates.map(|value| value as usize),
            rerank_candidates: None,
            min_score: params.min_score,
        };
        let scope = bars_protocol::SearchScope {
            modules: request.modules.clone(),
            files: request.files.clone(),
            paths: request.paths.clone(),
            excluded_projects: self.worker.nested_projects(&project, false),
            worktree: None,
        };
        // Rerank-модель, пока она скачивается, поиск не ждёт: выдача идёт без неё. Ожидание идёт
        // до замка проекта, чтобы не держать его и не задерживать подмену индекса.
        let mut skipped_rerank = None;
        let mut reranker = None;
        if settings.rerank.enabled {
            // Как и embed: скачанную модель, которая грузится в память, ждём до общего предела.
            let readiness = loop {
                match self.worker.models().rerank_readiness(&settings) {
                    Readiness::Loading { percent }
                        if percent >= 100 && tokio::time::Instant::now() < deadline =>
                    {
                        tokio::time::sleep(MODEL_POLL).await;
                    }
                    other => break other,
                }
            };
            match readiness {
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
                Readiness::Failed(failure) => {
                    history::applied_field(
                        "rerank_fallback_reason",
                        serde_json::json!("model_unavailable"),
                    );
                    skipped_rerank = Some(failure.message);
                }
            }
        }
        // «Новые темы»: допуск и область определяются до поиска, чтобы очистка списка во время
        // поиска отклонила запоздавшее наблюдение. Диагностика и CLI спроса не создают.
        let context = crate::new_topics::current_context();
        // Поиск по worktree спрос не записывает: выдача по виду неполна по определению.
        let collecting = !params.diagnostic
            && wt_plan.is_none()
            && context.purpose == crate::new_topics::Purpose::Request
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
        let project_gate = self.worker.gate(&project).map_err(worker_error)?;
        let gate = project_gate.clone().read_owned().await;
        let graph_requested = graph_budgets.is_some();
        let datasets = self.graph_cache.clone();
        let excluded_cache = self.wt_excluded.clone();
        let response = tokio::task::spawn_blocking(move || {
            let _gate = gate;
            let index = Index::open_read_only(&index_path(&project))?;
            // Вид worktree: индекс владельца без исключённых путей и дельта. Не открылась дельта —
            // ответ по владельцу с пометкой об ошибке.
            let mut wt_open_error = None;
            let delta = wt_merge.as_deref().and_then(|name| {
                match Index::open_read_only(&bars_core::indexing::worktree_delta::delta_path(
                    &project, name,
                )) {
                    Ok(delta) => Some(delta),
                    Err(error) => {
                        wt_open_error = Some(error.to_string());
                        None
                    }
                }
            });
            let mut wt_stats = None;
            let view = match (&delta, wt_merge.as_deref()) {
                (Some(delta), Some(name)) => {
                    match worktree_view::excluded_for(&excluded_cache, &project, name, &index, delta)
                        .map_err(bars_core::search::SearchError::from)
                        .and_then(|excluded| {
                            IndexView::worktree(&index, delta, &excluded.paths)
                                .map(|view| (view, (excluded.pending, excluded.delta_documents)))
                        }) {
                        Ok((view, stats)) => {
                            wt_stats = Some(stats);
                            view
                        }
                        Err(error) => {
                            wt_open_error = Some(error.to_string());
                            IndexView::single(&index)
                        }
                    }
                }
                _ => IndexView::single(&index),
            };
            // Нет rerank-модели или она не отработала: выдача идёт без неё, причина — в ответе.
            let reranker = reranker.as_mut().map(|model| model as &mut dyn Reranker);
            let mut related = None;
            let mut fallback = None;
            // Без вектора запроса связанные источники не добавляются (нет косинуса кандидатов).
            let result = if let Some(budgets) = graph_budgets.filter(|_| semantic) {
                let graph_started = std::time::Instant::now();
                let timeout = budgets
                    .resolve()
                    .map_err(|e| bars_core::search::SearchError::InvalidParams(e.to_string()))?
                    .0
                    .timeout_ms;
                let deadline = graph_started + std::time::Duration::from_millis(u64::from(timeout));
                let loaded = datasets
                    .load(&project, &project_gate, deadline)
                    .and_then(|data| {
                        if data
                            .layers
                            .values()
                            .any(|l| l.freshness == bars_core::graph::Freshness::Stale)
                        {
                            Err(bars_core::graph::GraphError::NotReady(
                                "граф устарел".into(),
                            ))
                        } else {
                            Ok(data)
                        }
                    });
                let load_elapsed = graph_started.elapsed();
                match loaded {
                    Ok(data) => {
                        let mut expand = |index: &Index, seeds: &[i64], limit: usize| {
                            match bars_core::graph::retrieval::expand_timed(
                                index,
                                &data,
                                &request,
                                seeds,
                                limit,
                                budgets.clone(),
                                load_elapsed,
                            ) {
                                Ok((ids, reply)) => {
                                    related = Some(reply);
                                    Ok(ids)
                                }
                                // Пакет не помещается даже пустым: выдача без него (ISS-88).
                                Err(bars_core::search::SearchError::InvalidParams(message))
                                    if message.contains(
                                        bars_core::graph::query::ENVELOPE_DOES_NOT_FIT,
                                    ) =>
                                {
                                    fallback = Some(
                                        "Связанные источники не выданы: пакет не помещается в \
                                         related_budget (или graph_budgets.max_bytes) даже пустым; \
                                         увеличьте related_budget"
                                            .into(),
                                    );
                                    Ok(Vec::new())
                                }
                                Err(error) => Err(error),
                            }
                        };
                        search_in_view(
                            &view,
                            &mut embedder,
                            reranker,
                            &settings.search,
                            &request,
                            Some(&mut expand),
                            true,
                            None,
                        )
                    }
                    Err(error) => {
                        fallback = Some(error.to_string());
                        search_in_view(
                            &view,
                            &mut embedder,
                            reranker,
                            &settings.search,
                            &request,
                            None,
                            true,
                            None,
                        )
                    }
                }
            } else {
                if graph_requested {
                    fallback = Some(
                        "Связанные источники не выданы: смысловая близость не учтена, модель загружается"
                            .into(),
                    );
                }
                search_in_view(
                    &view,
                    &mut embedder,
                    reranker,
                    &settings.search,
                    &request,
                    None,
                    semantic,
                    None,
                )
            };
            result.map(|result| (result, related, fallback, wt_stats, wt_open_error))
        })
        .await
        .map_err(internal)?
        .map_err(search_error)?;
        let (mut response, related, fallback, wt_stats, wt_open_error) = response;
        if response.rerank_error.is_some() {
            history::applied_field(
                "rerank_fallback_reason",
                serde_json::json!("rerank_execution_failed"),
            );
        }
        let query_vector = response.query_vector.take();
        let rerank_error = response.rerank_error.or(skipped_rerank);
        let mut by_module = bars_protocol::ModuleBreakdown {
            candidates: response
                .candidate_modules
                .into_iter()
                .map(|(name, n)| (name, to_u32(n)))
                .collect(),
            results: Default::default(),
        };
        for result in &response.hits {
            *by_module.results.entry(result.module.clone()).or_default() += 1;
        }
        let mut applied = applied(response.applied);
        applied.scope = scope;
        applied.scope.worktree = wt_plan.map(|plan| plan.finish(wt_stats, wt_open_error));
        // Слова запроса нужны прокси для отрывков и сортировки «other headers» в ступенчатой выдаче.
        let query_words: Vec<String> = response
            .understood
            .text
            .split_whitespace()
            .map(str::to_string)
            .collect();
        let output = SearchOutput {
            results: response.hits.into_iter().map(hit).collect(),
            understood: understood(response.understood),
            weak: response.weak.into_iter().map(weak_hit).collect(),
            query_words,
            applied,
            by_module,
            indexing: status.state == ProjectState::Indexing || status.queued,
            pending: to_u32(status.pending),
            rerank_error,
            semantic_error: (!semantic).then(|| SEMANTIC_UNAVAILABLE.to_string()),
        };
        // Только успешный поиск с нулём результатов при готовом индексе без известной ошибки обновления
        // (NT-02): индексация, очередь и техническая ошибка пробелом не считаются.
        // Выдача без смысловой части пробелом знаний не считается: пустой результат мог дать вектор.
        eprintln!(
            "NT-GATE: source={} semantic={} empty={} related={} state={:?} queued={} pending={} err={:?} collecting={:?} diagnostic={} origin={:?} purpose={:?}",
            topic_source.is_some(),
            semantic,
            output.results.is_empty(),
            related.is_none(),
            status.state,
            status.queued,
            status.pending,
            status.last_error,
            collecting,
            params.diagnostic,
            context.origin,
            context.purpose,
        );
        if let Some((generation, event_id, space, text, scope, project)) = topic_source
            && semantic
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
        let (project, view) = self.usable_view(target)?;
        let settings = self.settings(&project)?;
        // По виду worktree документ читается с диска W: допуск путей — от корня вида.
        let root = worktree_view::view_root(&project, &view);
        let file = resolve_doc_path(&root, Path::new(&settings.docs_root), path)
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
        let (text, _) = self.document(&params.target, &params.path)?;
        let selection = if params.lines.is_some() || params.under.is_some() {
            Some(
                bars_core::document_read::select(
                    &text,
                    params.lines.as_deref(),
                    params.under.as_deref(),
                )
                .map_err(|e| RpcError::bars(ErrorKind::InvalidParams, e))?,
            )
        } else {
            None
        };
        let title = document_title(&params.path, &text);
        let revision = blake3::hash(text.as_bytes()).to_hex().to_string();
        let mut output = ReadOutput {
            title,
            revision,
            ..ReadOutput::default()
        };
        // Агентский автоматический режим: длинный документ отдаётся структурой с подсказкой
        // прочитать часть или весь текст, а не текстом на тысячи строк.
        if selection.is_none()
            && let Some(max_lines) = params.max_lines
        {
            let total = bars_protocol::count_lines(&text);
            if total > max_lines {
                output.lines_total = Some(total);
                output.headings = Some(
                    outline(&text, &bars_core::outline::OutlineQuery::default())
                        .map_err(outline_error)?
                        .into_iter()
                        .map(|heading| Heading {
                            line: to_u32(heading.line),
                            level: heading.level as u8,
                            text: heading.text,
                        })
                        .collect(),
                );
                history::applied_field("range", serde_json::json!("outline_only"));
                return Ok(output);
            }
        }
        if let Some(selection) = selection {
            output.lines_total = Some(selection.lines_total);
            for part in selection.parts {
                output.text.push_str(&text[part.bytes]);
                if let Some(heading_chain) = part.heading_chain {
                    output.sections.push(bars_protocol::ReadSection {
                        from: part.from,
                        to: part.to,
                        heading_chain,
                    });
                } else {
                    output.range = Some(bars_protocol::ReadRange {
                        from: part.from,
                        to: part.to,
                    });
                }
            }
        } else {
            output.text = text;
        }
        if !output.sections.is_empty() {
            history::applied_field("range", serde_json::json!("sections"));
        }
        Ok(output)
    }

    async fn projects(
        &self,
        params: bars_protocol::ProjectsParams,
    ) -> Result<ProjectsOutput, RpcError> {
        let filter = bars_protocol::filter_text(params.filter.as_deref()).map(str::to_string);
        if filter.is_some() && params.target.is_some() {
            return Err(RpcError::bars(
                ErrorKind::InvalidParams,
                "filter searches the whole registry; drop project",
            ));
        }
        let selected = params
            .target
            .as_ref()
            .map(|target| self.resolve(target))
            .transpose()
            .map_err(|mut error| {
                if error.kind() == Some(ErrorKind::ProjectNotFound) {
                    error
                        .message
                        .push_str("; список всех проектов — bars_status(all=true)");
                }
                error
            })?;
        let all = self.worker.projects();
        if let Some(path) = selected {
            let snapshot = all
                .iter()
                .find(|snapshot| snapshot.path == path)
                .ok_or_else(|| {
                    RpcError::bars(
                        ErrorKind::ProjectNotFound,
                        "проект удалён из реестра; повторите выбор проекта",
                    )
                })?;
            // Ленивое состояние берётся из реестра: проверок у такого проекта ещё не было.
            let state = if snapshot.lazy {
                ProjectState::Lazy
            } else {
                self.worker.status(&path).map_err(worker_error)?.state
            };
            let modules = if state == ProjectState::Unavailable {
                Vec::new()
            } else {
                self.modules(&path, &snapshot.docs_root).await?
            };
            let nested = self.worker.nested_projects(&path, false);
            return Ok(ProjectsOutput {
                projects: vec![self.assemble_info(snapshot, &all, state, modules, nested)],
                registry_total: None,
            });
        }
        // Весь реестр или только совпавшие с фильтром записи; предок идёт раньше подпроектов.
        let order = match &filter {
            Some(text) => project_filter::matching(&all, text),
            None => project_filter::parents_first(&all),
        };
        let mut projects = Vec::with_capacity(order.len());
        for index in order {
            let mut info = self.project_info(&all[index], &all).await;
            if let Some(text) = &filter {
                info.matched_worktrees = project_filter::worktree_matches(&all[index], text);
            }
            projects.push(info);
        }
        Ok(ProjectsOutput {
            projects,
            registry_total: filter.is_some().then(|| all.len() as u32),
        })
    }

    async fn project_info(
        &self,
        snapshot: &ProjectSnapshot,
        all: &[ProjectSnapshot],
    ) -> ProjectInfo {
        let modules = if snapshot.state == ProjectState::Unavailable {
            Vec::new()
        } else {
            self.modules(&snapshot.path, &snapshot.docs_root)
                .await
                .unwrap_or_default()
        };
        let nested = self.worker.nested_projects(&snapshot.path, false);
        self.assemble_info(snapshot, all, snapshot.state, modules, nested)
    }

    /// Строка `projects`: собственные поля проекта и его место среди подпроектов (Т-49).
    fn assemble_info(
        &self,
        snapshot: &ProjectSnapshot,
        all: &[ProjectSnapshot],
        state: ProjectState,
        modules: Vec<ModuleInfo>,
        nested: Vec<bars_protocol::NestedProjectInfo>,
    ) -> ProjectInfo {
        // Прямые подпроекты: ближайший предок — этот проект; порядок реестра.
        let subprojects = all
            .iter()
            .filter(|child| child.parent.as_deref() == Some(snapshot.path.as_path()))
            .map(|child| {
                let path = child.path.display().to_string();
                bars_protocol::SubprojectInfo {
                    name: child
                        .qualified_name
                        .clone()
                        .unwrap_or_else(|| child.name.clone()),
                    excluded: nested.iter().any(|area| area.path == path),
                    path,
                    state: child.state,
                    discovered: child.discovered,
                }
            })
            .collect();
        ProjectInfo {
            path: snapshot.path.display().to_string(),
            name: snapshot.name.clone(),
            docs_root: snapshot.docs_root.clone(),
            state,
            modules,
            nested,
            parent: snapshot
                .parent
                .as_ref()
                .map(|parent| parent.display().to_string()),
            parent_name: snapshot.parent.as_ref().and_then(|parent| {
                all.iter()
                    .find(|candidate| &candidate.path == parent)
                    .map(|candidate| {
                        candidate
                            .qualified_name
                            .clone()
                            .unwrap_or_else(|| candidate.name.clone())
                    })
            }),
            qualified_name: snapshot.qualified_name.clone(),
            discovered: snapshot.discovered,
            subprojects,
            discovery_truncated: snapshot.discovery_truncated,
            matched_worktrees: Vec::new(),
        }
    }

    /// Модули проекта и число документов в индексе. Проект без индекса — без модулей.
    async fn modules(&self, project: &Path, docs_root: &str) -> Result<Vec<ModuleInfo>, RpcError> {
        if !index_path(project).exists() {
            return Ok(Vec::new());
        }
        let gate = self
            .worker
            .gate(project)
            .map_err(worker_error)?
            .read_owned()
            .await;
        let project = project.to_path_buf();
        let docs_root = docs_root.replace('\\', "/");
        let docs_root = docs_root.trim_end_matches('/');
        let prefix = if docs_root == "." || docs_root.is_empty() {
            String::new()
        } else {
            format!("{docs_root}/")
        };
        tokio::task::spawn_blocking(move || {
            let _gate = gate;
            let index = Index::open_read_only(&index_path(&project)).map_err(internal)?;
            let mut statement = index
                .conn()
                // Один проход метаданных; вложенный Index не задаёт назначение родителя.
                .prepare(
                    "SELECT module, count(*), MAX(CASE WHEN lower(path) = lower(?1 || \
                    CASE WHEN module = '.' THEN '' ELSE module || '/' END || 'index.md') \
                    THEN title END) FROM documents GROUP BY module ORDER BY module",
                )
                .map_err(internal)?;
            let rows = statement
                .query_map([prefix], |row| {
                    Ok(ModuleInfo {
                        name: row.get(0)?,
                        documents: row.get(1)?,
                        title: row.get(2)?,
                    })
                })
                .map_err(internal)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(internal)
        })
        .await
        .map_err(internal)?
    }

    fn status(&self, params: StatusParams) -> Result<StatusOutput, RpcError> {
        let (project, view) = self.resolve_view(&params.target)?;
        let status = self.worker.status(&project).map_err(worker_error)?;
        // Worktree сессии: `cwd` внутри живого worktree этого проекта.
        let session = self
            .worker
            .resolve_cwd(Path::new(&params.target.cwd))
            .filter(|(owner, _)| *owner == project)
            .and_then(|(_, reference)| reference)
            .map(|reference| reference.name);
        // С явным `wt` — только этот worktree; без него и с `wt="."` — все живые.
        let only = match (&view, params.target.wt()) {
            (ViewSelector::Worktree(item), Some(value)) if value != "." => Some(item.name.clone()),
            _ => None,
        };
        let worktrees = self
            .worker
            .linked_worktrees(&project)
            .into_iter()
            .filter(|item| only.as_ref().is_none_or(|name| *name == item.name))
            .map(|item| {
                let delta = self.worker.worktree_delta_status(&project, &item.name);
                WorktreeStatus {
                    current: session.as_deref() == Some(item.name.as_str()),
                    name: item.name,
                    path: item.path.display().to_string(),
                    branch: item.branch,
                    locked: item.locked,
                    delta,
                }
            })
            .collect();
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
            worktrees,
        })
    }

    fn reindex(&self, params: ReindexParams) -> Result<AcceptedOutput, RpcError> {
        let (project, view) = self.resolve_view(&params.target)?;
        if let ViewSelector::Worktree(item) = &view {
            // Вид worktree: проверка актуальности — сверка дельты (первое обращение, если дельты
            // нет). Полная переиндексация и смена модели у дельты не бывают: достаточно удалить
            // worktree или сменить модель владельца.
            if params.full || params.accept_model_change {
                let mut extra = serde_json::Map::new();
                extra.insert("kind".into(), serde_json::json!("worktree_unsupported"));
                extra.insert("worktree".into(), serde_json::json!(item.name));
                return Err(RpcError::bars_with(
                    ErrorKind::InvalidParams,
                    format!(
                        "полная переиндексация не работает по worktree «{}»: у дельты есть только \
                         сверка (reindex --wt). Полная переиндексация — у основного checkout, wt=\".\"",
                        item.name
                    ),
                    extra,
                ));
            }
            if !self.worker.resync_worktree(&project, &item.name) {
                return Err(RpcError::bars(
                    ErrorKind::ProjectUnavailable,
                    format!(
                        "сверка worktree «{}» не поставлена: проект-владелец не готов (индекс \
                         строится, нужна переиндексация или папка недоступна)",
                        item.name
                    ),
                ));
            }
            history::applied_field("job_kind", serde_json::json!("worktree_delta"));
            return Ok(AcceptedOutput { accepted: true });
        }
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
        let all = self.worker.projects();
        let snapshot = all
            .iter()
            .find(|snapshot| snapshot.path == registered.path)
            .ok_or_else(|| {
                RpcError::standard(
                    INTERNAL_ERROR,
                    "проект не найден в реестре после регистрации",
                )
            })?;
        Ok(RegisterOutput {
            project: self.project_info(snapshot, &all).await,
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

    /// Очередь документов проектов: состояния, оставшаяся пауза, ошибки (до 200 записей на
    /// проект); проекты без записей в список не попадают.
    fn documents(&self) -> Vec<ProjectDocuments> {
        const LIMIT: usize = 200;
        self.worker
            .projects()
            .into_iter()
            .filter_map(|snapshot| {
                let (documents, counts) = self.worker.doc_queue(&snapshot.path, LIMIT).ok()?;
                (counts.total() > 0).then(|| ProjectDocuments {
                    project: snapshot.path.display().to_string(),
                    documents: documents
                        .into_iter()
                        .map(|document| QueuedDocument {
                            path: document.path,
                            state: match document.state {
                                crate::worker::DocState::Waiting => DocumentState::Waiting,
                                crate::worker::DocState::Queued => DocumentState::Queued,
                                crate::worker::DocState::Indexing => DocumentState::Indexing,
                                crate::worker::DocState::Error => DocumentState::Error,
                            },
                            source: document_source(document.source),
                            due_in: document.due_in.map(|left| {
                                to_u32(
                                    left.as_secs() as usize + usize::from(left.subsec_nanos() > 0),
                                )
                            }),
                            error: document.error,
                        })
                        .collect(),
                    total: to_u32(counts.total()),
                    waiting: to_u32(counts.waiting),
                    queued: to_u32(counts.queued),
                    indexing: to_u32(counts.indexing),
                    error: to_u32(counts.error),
                })
            })
            .collect()
    }

    fn index_retry(&self, params: IndexRetryParams) -> Result<RetryOutput, RpcError> {
        let project = self.worker.resolve(&params.project).map_err(worker_error)?;
        let retried = self
            .worker
            .doc_retry(&project, &params.path)
            .map_err(worker_error)?;
        Ok(RetryOutput { retried })
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
            documents: self.documents(),
        }
    }

    /// Документы проекта из индекса. Индекс читается без модели, поэтому дерево видно и в
    /// состоянии `reindex_required` (Т-25). Для вида worktree — документы владельца, поверх них
    /// документы готовой дельты (заголовки и чанки вида); первое обращение ставит построение
    /// дельты. Дерево по диску W окно читает само (`tree_read` с `wt`).
    async fn tree(&self, params: TreeParams) -> Result<TreeOutput, RpcError> {
        let (project, view) = self.usable_view(&params.target)?;
        let docs_root = self.settings(&project)?.docs_root;
        let mut documents = if index_path(&project).exists() {
            let gate = self
                .worker
                .gate(&project)
                .map_err(worker_error)?
                .read_owned()
                .await;
            let db = index_path(&project);
            tokio::task::spawn_blocking(move || {
                let _gate = gate;
                worktree_view::tree_documents(&db)
            })
            .await
            .map_err(internal)?
            .map_err(internal)?
        } else {
            Vec::new()
        };
        if let ViewSelector::Worktree(item) = &view {
            self.worker.ensure_worktree_delta(&project, &item.name);
            let state = self
                .worker
                .worktree_delta_status(&project, &item.name)
                .state;
            if state == bars_protocol::DeltaState::Ready {
                let db = bars_core::indexing::worktree_delta::delta_path(&project, &item.name);
                // Дельта производная: недоступная (ещё не открылась, удаляется) — дерево по владельцу.
                if let Ok(Ok((own, in_view))) = tokio::task::spawn_blocking(move || {
                    let own = worktree_view::tree_documents(&db)?;
                    let delta = bars_core::storage::Index::open_read_only(&db)?;
                    let in_view: std::collections::BTreeSet<String> =
                        bars_core::indexing::worktree_delta::view_files(&delta)?
                            .into_iter()
                            .map(|file| file.path)
                            .collect();
                    Ok::<_, bars_core::storage::StorageError>((own, in_view))
                })
                .await
                {
                    // Документы владельца, которых в worktree нет (удалены в его ветке), в дерево
                    // вида не попадают.
                    let mut merged: std::collections::BTreeMap<String, TreeDocument> = documents
                        .into_iter()
                        .filter(|document| in_view.contains(&document.path))
                        .map(|document| (document.path.clone(), document))
                        .collect();
                    for document in own {
                        merged.insert(document.path.clone(), document);
                    }
                    documents = merged.into_values().collect();
                }
            }
        }
        Ok(TreeOutput {
            docs_root,
            documents,
        })
    }

    // --- методы окна: настройки, модели, ключи, клиенты ---

    fn settings_get(&self) -> Result<SettingsOutput, RpcError> {
        settings_io::load_user_settings(self.worker.bars_home()).map_err(save_error)
    }

    /// Сохраняет настройки и ставит все проекты на проверку: проекты, чей индекс построен другой
    /// моделью, перейдут в `reindex_required`, и окно покажет диалог согласия (Т-25). Эмбеддинг
    /// при этом не запускается.
    fn settings_save(
        &self,
        params: SettingsSaveParams,
    ) -> Result<bars_protocol::SettingsSaveOutput, RpcError> {
        let output =
            settings_io::save_changes(self.worker.bars_home(), params).map_err(save_error)?;
        if output.status == bars_protocol::SettingsSaveStatus::Saved {
            self.worker.refresh_model_lifecycle();
            self.worker.check_all();
        }
        Ok(output)
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
            download_error: download_errors
                .get(id)
                .map(|failure| failure.message.clone()),
            download_error_reason: download_errors
                .get(id)
                .filter(|_| id != bars_core::models::BGE_RERANKER_V2_M3_ID)
                .map(|failure| failure.kind.as_str().to_string()),
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
        let clients = [
            InstallClient::ClaudeCode,
            InstallClient::Codex,
            InstallClient::KimiCode,
            InstallClient::Ouroboros,
            InstallClient::Iterion,
        ]
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
            self.audits_begin_delete(&project_path).await;
            let deleted = self.worker.delete_project(&params.project).await;
            self.audits.end_delete(&project_path);
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
                .graph_build_layer(&project, params.layer)
                .await
                .map(|out| output_value(&out))
                .map_err(graph::graph_error);
        }
        // Удаление ждёт запросы, уже открывшие индекс или настройки; новые ждут очистки.
        let _request = self.worker.request_guard().await;
        // Граф и ссылки по worktree отвечают по основному checkout: в ответе строка пометки.
        let owner_only_note = match &call {
            Call::GraphSearch(params) => self.owner_only_note_for(&params.search.target),
            Call::GraphQuery(params) => self.owner_only_note_for(&params.target),
            Call::GraphStatus(params) => self.owner_only_note_for(&params.target),
            Call::DiskLinks(params) => self.owner_only_note_for(&params.target),
            _ => None,
        };
        let result = match call {
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
            Call::GraphPhysical(params) => self
                .graph_physical(params)
                .await
                .map(|out| output_value(&out)),
            Call::GraphBuild(_) => unreachable!(),
            Call::GraphQuerySlice(params) => self
                .graph_query_slice(params)
                .await
                .map(|out| output_value(&out)),
            Call::GraphCancel(params) => {
                let project = self.resolve(&params.target)?;
                self.worker
                    .graph_cancel(&project, &params.job_id)
                    .map(|cancelled| output_value(&CancelOutput { cancelled }))
                    .map_err(graph::graph_error)
            }
            Call::ProjectDelete(_) => unreachable!(),
            Call::ProjectRename(params) => {
                let name = params.name.trim();
                if name.is_empty() || name.chars().any(char::is_control) {
                    return Err(RpcError::bars(
                        ErrorKind::InvalidParams,
                        "введите имя проекта без управляющих символов",
                    ));
                }
                self.worker
                    .rename_project(&params.project, name.to_string())
                    .map_err(worker_error)?;
                Ok(output_value(&SavedOutput { saved: true }))
            }
            Call::ProjectObserve(params) => {
                if params.window.is_empty() || params.window.len() > 128 {
                    return Err(RpcError::standard(
                        bars_protocol::INVALID_PARAMS,
                        "некорректный ID окна",
                    ));
                }
                self.worker
                    .observe_window(&params.window, params.project.as_deref().map(Path::new))
                    .map_err(worker_error)?;
                Ok(output_value(&SavedOutput { saved: true }))
            }
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
            Call::Like(params) => self.like(params).await.map(|out| output_value(&out)),
            Call::Duplicates(params) => self.duplicates(params).await.map(|out| output_value(&out)),
            Call::Outline(params) => self.outline(params).map(|out| output_value(&out)),
            Call::Read(params) if bars_protocol::is_folder_path(&params.path) => {
                self.read_folder(params).await.map(|out| output_value(&out))
            }
            Call::Read(params) => self.read(params).map(|out| output_value(&out)),
            Call::Projects(params) => self.projects(params).await.map(|out| output_value(&out)),
            Call::SessionInfo(params) => self
                .session_info(params)
                .await
                .map(|out| output_value(&out)),
            Call::Status(params) => {
                // Ошибка графа не роняет сводку индекса (ISS-110): граф читается отдельным
                // методом, а окно видит его состояние на своей странице графа.
                let target = params.target.clone();
                let mut out = self.status(params)?;
                match self
                    .graph_status(bars_protocol::GraphStatusParams { target })
                    .await
                {
                    Ok(graph) => out.graph = Some(graph),
                    Err(error) => {
                        log::warn!("Статус графа не получен: {error}");
                    }
                }
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
            Call::IndexRetry(params) => self.index_retry(params).map(|out| output_value(&out)),
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
            Call::ModelRetry(_) => {
                self.worker.models().reset_pauses();
                Ok(output_value(&AcceptedOutput { accepted: true }))
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
            Call::NewTopicsMerge(params) => self
                .new_topics_merge(params)
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
            Call::LinksOf(params) => self.links_of(params).await.map(|out| output_value(&out)),
            Call::LinksInto(params) => self.links_into(params).await.map(|out| output_value(&out)),
            Call::DiskText(params) => self.disk_text(params).await.map(|out| output_value(&out)),
            Call::DiskLinks(params) => self.disk_links(params).await.map(|out| output_value(&out)),
            Call::TextSearch(params) => {
                self.text_search(params).await.map(|out| output_value(&out))
            }
            Call::AuditList(params) => self.audit_list(params).await.map(|out| output_value(&out)),
            Call::AuditSummary(params) => self
                .audit_summary(params)
                .await
                .map(|out| output_value(&out)),
            Call::AuditIgnore(params) => self
                .audit_ignore(params)
                .await
                .map(|out| output_value(&out)),
            Call::AuditRun(params) => self.audit_run(params).await.map(|out| output_value(&out)),
            Call::AuditCancel(params) => self.audit_cancel(params).map(|out| output_value(&out)),
            Call::AuditFindings(params) => self
                .audit_findings(params)
                .await
                .map(|out| output_value(&out)),
            Call::AuditSchedule(params) => self
                .audit_schedule(params)
                .await
                .map(|out| output_value(&out)),
            Call::AutostartGet(AutostartGetParams {}) => {
                self.autostart_get().map(|out| output_value(&out))
            }
            Call::AutostartSet(params) => self.autostart_set(params).map(|out| output_value(&out)),
        };
        worktree_view::with_note(result, owner_only_note)
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

/// Источник агентского `like` в форме `путь:строки` или `путь:строка`: путь отделяется от
/// строк последним `:` с числовым хвостом. Хвост с `-` (ID чанка вида `123-abcdef12`)
/// сюда не попадает — у него нет `:`.
fn like_path_line(source: &str) -> Option<(String, i64)> {
    let (path, lines) = source.rsplit_once(':')?;
    let well_formed = !path.is_empty()
        && !lines.is_empty()
        && !lines.starts_with('-')
        && !lines.ends_with('-')
        && lines.matches('-').count() <= 1
        && lines.bytes().all(|b| b.is_ascii_digit() || b == b'-');
    if !well_formed {
        return None;
    }
    let line: i64 = lines
        .split('-')
        .next()
        .expect("непустой хвост")
        .parse()
        .ok()?;
    (line > 0).then(|| (path.to_string(), line))
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

fn document_source(source: crate::worker::DocSource) -> DocumentSource {
    match source {
        crate::worker::DocSource::Explicit => DocumentSource::Explicit,
        crate::worker::DocSource::Edit => DocumentSource::Edit,
        crate::worker::DocSource::Watch => DocumentSource::Watch,
        crate::worker::DocSource::Periodic => DocumentSource::Periodic,
    }
}

fn job_priority(priority: Priority) -> JobPriority {
    match priority {
        Priority::Explicit => JobPriority::Explicit,
        Priority::Edit => JobPriority::Edit,
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
        bars_core::search::ScoreKind::None => ScoreKind::None,
    }
}

/// Результат `similar`: кусок без баллов по сигналам и без счёта кусков раздела.
fn similar_hit(found: bars_core::search::SearchHit) -> SearchHit {
    SearchHit {
        signals: None,
        parts: 0,
        parts_found: 0,
        ..hit(found)
    }
}

/// «Слабые совпадения» пустой выдачи: раздел ниже порога без текста.
fn weak_hit(hit: bars_core::search::WeakHit) -> bars_protocol::WeakHit {
    bars_protocol::WeakHit {
        path: hit.path,
        heading_chain: hit.heading_chain,
        first_line: to_u32(hit.first_line),
        last_line: to_u32(hit.last_line),
        score: hit.score,
    }
}

fn hit(hit: bars_core::search::SearchHit) -> SearchHit {
    SearchHit {
        source_hash: hit.source_hash,
        chunk_id: hit.public_id,
        path: hit.path,
        module: hit.module,
        title: hit.title,
        sections: merged_sections(&hit.text),
        heading_chain: hit.heading_chain,
        first_line: to_u32(hit.first_line),
        last_line: to_u32(hit.last_line),
        text: hit.text,
        score: hit.score,
        score_kind: score_kind(hit.score_kind),
        signals: Some(bars_protocol::HitSignals {
            rerank: hit.signals.rerank,
            cosine: hit.signals.cosine,
            fulltext: hit.signals.fulltext,
            exact: hit.signals.exact,
            links: hit.signals.links,
        }),
        parts: to_u32(hit.parts),
        parts_found: to_u32(hit.parts_found),
    }
}

/// Заголовки разделов, приклеенных к чанку после первого (ISS-28): заголовки в тексте чанка до
/// списка `other headers:`, по правилам нарезки — ATX вне блоков кода.
fn merged_sections(text: &str) -> Vec<String> {
    let own = if text.starts_with("other headers:") {
        ""
    } else {
        text.split("\nother headers:").next().unwrap_or(text)
    };
    outline(own, &OutlineQuery::default())
        .map(|headings| {
            headings
                .into_iter()
                .map(|h| format!("{} {}", "#".repeat(h.level), h.text))
                .collect()
        })
        .unwrap_or_default()
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
        scope: Default::default(),
        weights: bars_protocol::ScoreWeights {
            cosine: applied.weights.cosine,
            fulltext: applied.weights.fulltext,
            exact: applied.weights.exact,
            links: applied.weights.links,
            fold_bonus: applied.weights.fold_bonus,
        },
        signals: applied
            .signals
            .iter()
            .map(|name| (*name).to_string())
            .collect(),
    }
}

/// «Понято как»: запрос, разобранный на операторы; без операторов — `None`.
fn understood(parsed: bars_core::search::ParsedQuery) -> Option<bars_protocol::UnderstoodQuery> {
    parsed
        .has_operators()
        .then_some(bars_protocol::UnderstoodQuery {
            text: parsed.text,
            phrases: parsed.phrases,
            excluded_words: parsed.excluded_words,
            excluded_phrases: parsed.excluded_phrases,
            paths: parsed.paths,
        })
}
