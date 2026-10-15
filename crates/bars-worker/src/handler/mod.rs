//! Обработчик методов протокола поверх воркера и `bars-core` (`docs/architecture/protocol.md`).
//!
//! `hello`, права клиента и сброс `accept_model_change` у MCP делает цикл соединения в
//! `bars-protocol`; сюда доходят уже проверенные вызовы. Окно вызывает те же методы без канала (Т-20).

mod errors;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use bars_core::doc_path::resolve_doc_path;
use bars_core::indexing::chunking::chunk_document;
use bars_core::indexing::walk::read_utf8_path;
use bars_core::models::Reranker;
use bars_core::outline::{OutlineQuery, Under, outline};
use bars_core::search::{SearchRequest, search};
use bars_core::settings::Settings;
use bars_core::storage::{Index, Journal, index_path};
use bars_protocol::{
    AcceptedOutput, AppliedParams, Call, ClientKind, ErrorKind, Heading, INTERNAL_ERROR,
    InstallOutput, InstallParams, ModelStatus, ModuleInfo, OutlineOutput, OutlineParams,
    ProjectInfo, ProjectState, ProjectsOutput, ReadOutput, ReadParams, RegisterOutput,
    RegisterParams, ReindexParams, RpcError, ScoreKind, SearchHit, SearchOutput, SearchParams,
    SessionStartParams, StatusOutput, StatusParams, Target, UninstallOutput, UninstallParams,
    UnregisterOutput, UnregisterParams, output_value,
};
use serde_json::Value;

use crate::clients::{self, InstallEnv};
use crate::worker::{ProjectSnapshot, Worker, rfc3339};
use errors::{doc_path_error, outline_error, read_error, search_error, worker_error};

/// Выполняет вызовы канала на воркере.
#[derive(Clone)]
pub struct WorkerHandler {
    worker: Worker,
}

impl WorkerHandler {
    pub fn new(worker: Worker) -> Self {
        Self { worker }
    }

    /// Проект вызова: явный `project` или ближайший к `cwd` зарегистрированный (Т-15).
    fn resolve(&self, target: &Target) -> Result<PathBuf, RpcError> {
        if let Some(selector) = &target.project {
            return self.worker.resolve(selector).map_err(worker_error);
        }
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
            })
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

    async fn search(&self, params: SearchParams) -> Result<SearchOutput, RpcError> {
        let project = self.usable(&params.target)?;
        let status = self.worker.status(&project).map_err(worker_error)?;
        if status.state == ProjectState::ReindexRequired {
            return Err(errors::reindex_required());
        }
        let settings = self.settings(&project)?;
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
        // Замок держится на всё время поиска: подмена индекса при полной переиндексации ждёт,
        // пока поиски дочитают (`worker.md`, «Индекс проекта в воркере»).
        let gate = self
            .worker
            .gate(&project)
            .map_err(worker_error)?
            .read_owned()
            .await;
        let mut reranker = settings
            .rerank
            .enabled
            .then(|| self.worker.models().reranker(&settings));
        let response = tokio::task::spawn_blocking(move || {
            let _gate = gate;
            let index = Index::open(&index_path(&project), Journal::Wal)?;
            // Нет rerank-модели или она не отработала: выдача идёт без неё, причина — в ответе.
            let reranker = reranker.as_mut().map(|model| model as &mut dyn Reranker);
            search(&index, &mut embedder, reranker, &settings.search, &request)
        })
        .await
        .map_err(internal)?
        .map_err(search_error)?;
        let rerank_error = response.rerank_error;
        Ok(SearchOutput {
            results: response.hits.into_iter().map(hit).collect(),
            applied: applied(response.applied),
            indexing: status.state == ProjectState::Indexing || status.queued,
            pending: to_u32(status.pending),
            rerank_error,
        })
    }

    /// Документ по пути из вызова: текст и корень проекта для заголовка.
    fn document(&self, target: &Target, path: &str) -> Result<(String, Settings), RpcError> {
        let project = self.usable(target)?;
        let settings = self.settings(&project)?;
        let file = resolve_doc_path(&project, Path::new(&settings.docs_root), path)
            .map_err(doc_path_error)?;
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
        })
    }

    fn reindex(&self, params: ReindexParams) -> Result<AcceptedOutput, RpcError> {
        let project = self.resolve(&params.target)?;
        self.worker
            .reindex(&project, params.full, params.accept_model_change)
            .map_err(worker_error)?;
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

    fn install(&self, params: InstallParams) -> Result<InstallOutput, RpcError> {
        let done = clients::install(self.install_env()?, params.client).map_err(internal)?;
        Ok(InstallOutput {
            changed_files: paths(done.changed_files),
            needs_hook_approval: done.needs_hook_approval,
        })
    }

    fn uninstall(&self, params: UninstallParams) -> Result<UninstallOutput, RpcError> {
        let changed = clients::uninstall(self.install_env()?, params.client).map_err(internal)?;
        Ok(UninstallOutput {
            changed_files: paths(changed),
        })
    }

    pub async fn dispatch(&self, call: Call) -> Result<Value, RpcError> {
        match call {
            // `hello` обрабатывает цикл соединения.
            Call::Hello(_) => Err(RpcError::standard(
                INTERNAL_ERROR,
                "hello не доходит до обработчика",
            )),
            Call::Search(params) => self.search(params).await.map(|out| output_value(&out)),
            Call::Outline(params) => self.outline(params).map(|out| output_value(&out)),
            Call::Read(params) => self.read(params).map(|out| output_value(&out)),
            Call::Projects(_) => self.projects().await.map(|out| output_value(&out)),
            Call::Status(params) => self.status(params).map(|out| output_value(&out)),
            Call::Reindex(params) => self.reindex(params).map(|out| output_value(&out)),
            Call::Register(params) => self.register(params).await.map(|out| output_value(&out)),
            Call::Unregister(params) => self.unregister(params).map(|out| output_value(&out)),
            Call::SessionStart(params) => self.session_start(params).map(|out| output_value(&out)),
            Call::Install(params) => self.install(params).map(|out| output_value(&out)),
            Call::Uninstall(params) => self.uninstall(params).map(|out| output_value(&out)),
        }
    }
}

impl bars_protocol::Handler for WorkerHandler {
    async fn handle(&self, _client: ClientKind, call: Call) -> Result<Value, RpcError> {
        self.dispatch(call).await
    }
}

fn internal(error: impl std::fmt::Display) -> RpcError {
    RpcError::standard(INTERNAL_ERROR, error.to_string())
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
