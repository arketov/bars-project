//! Внутренний API окна для аудитов знаний (`docs/features/knowledge-base/audits.md`, Т-36).
//!
//! Аудит читает индекс ссылок и индекс чанков и ничего не пишет в Markdown. Запуск идёт фоновым
//! заданием воркера: один аудит на проект, остальные ждут; модель не загружается, замок модели не
//! берётся, индекс чанков читается под общим замком проекта, как у `get_duplicates`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bars_core::audits::{
    AuditKind, AuditStore, NewFinding, RunRow, RunStatus, ScheduleMode, Subject, docs_stamp,
    document_role, fingerprint,
};
use bars_core::link_index::LinkIndex;
use bars_core::search::{
    DuplicateItem, DuplicateKind, DuplicatesRequest, duplicates_from_snapshot, duplicates_snapshot,
};
use bars_core::storage::{Index, index_path};
use bars_protocol::{
    AuditCancelOutput, AuditCancelParams, AuditFinding, AuditFindingsOutput, AuditFindingsParams,
    AuditListOutput, AuditListParams, AuditRunOutput, AuditRunParams, AuditScheduleOutput,
    AuditScheduleParams, AuditSubject, AuditSummary, ErrorKind, ProjectState, RpcError,
};

use super::errors::worker_error;
use super::{WorkerHandler, internal};
use crate::audits::{CancelFlag, Live, Request, now};

/// Сколько находок `duplicates` вид готов принять за запуск; больше — запуск неполный.
const DUPLICATES_CAP: usize = 5000;

/// Доля общих окон, с которой частичная копия попадает в умолчания экрана (3.8.2: 6 из 7 — дубли).
pub(crate) const STRONG_PARTIAL: f32 = 0.4;

/// Страница находок по умолчанию и предел страницы.
const PAGE_DEFAULT: i64 = 50;
const PAGE_MAX: i64 = 200;

/// Как часто проверяется расписание.
const SCHEDULE_TICK: Duration = Duration::from_secs(60);

/// Итог расчёта вида до записи в хранилище.
struct Outcome {
    status: RunStatus,
    error: Option<String>,
    stamp: Option<String>,
    findings: Vec<NewFinding>,
}

impl Outcome {
    fn failed(message: impl Into<String>) -> Self {
        Self {
            status: RunStatus::Failed,
            error: Some(message.into()),
            stamp: None,
            findings: Vec::new(),
        }
    }
}

fn parse_kind(kind: &str) -> Result<AuditKind, RpcError> {
    AuditKind::parse(kind).ok_or_else(|| {
        RpcError::bars(
            ErrorKind::InvalidParams,
            format!("неизвестный вид аудита: {kind}"),
        )
    })
}

fn audit_error(error: impl std::fmt::Display) -> RpcError {
    internal(error)
}

fn schedules_of(kind: AuditKind) -> Vec<String> {
    [
        ScheduleMode::Off,
        ScheduleMode::Daily,
        ScheduleMode::Weekly,
        ScheduleMode::OnChange,
    ]
    .into_iter()
    .filter(|mode| kind.allows(*mode))
    .map(|mode| mode.as_str().to_string())
    .collect()
}

fn subject(path: &str, first: usize, last: usize, heading: &str) -> Subject {
    Subject {
        role: document_role(path).to_string(),
        path: path.to_string(),
        first_line: u32::try_from(first).unwrap_or(u32::MAX),
        last_line: u32::try_from(last).unwrap_or(u32::MAX),
        heading: heading.to_string(),
    }
}

/// Находка вида `broken_links`: источник, строка и текст ссылки. Отпечаток не зависит от строки.
fn broken_finding(link: &bars_core::link_index::BrokenLink, occurrence: usize) -> NewFinding {
    let ordinal = occurrence.to_string();
    NewFinding {
        fingerprint: fingerprint(&["broken_links", &link.src, &link.raw, &link.text, &ordinal]),
        tag: "broken".into(),
        group: "main".into(),
        reason: "Ссылка на документ, которого нет".into(),
        score: None,
        subjects: vec![subject(
            &link.src,
            link.line as usize,
            link.line as usize,
            "",
        )],
        detail: if link.text.is_empty() {
            link.raw.clone()
        } else {
            format!("{} → {}", link.text, link.raw)
        },
    }
}

fn kind_tag(kind: DuplicateKind) -> &'static str {
    match kind {
        DuplicateKind::Exact => "exact",
        DuplicateKind::Near => "near",
        DuplicateKind::Contained => "contained",
        DuplicateKind::Partial => "partial",
        DuplicateKind::Semantic => "semantic",
    }
}

fn kind_reason(kind: DuplicateKind) -> &'static str {
    match kind {
        DuplicateKind::Exact => "Одинаковый текст",
        DuplicateKind::Near => "Почти одинаковый текст",
        DuplicateKind::Contained => "Раздел целиком внутри другого",
        DuplicateKind::Partial => "Заметная часть текста скопирована",
        DuplicateKind::Semantic => "Близкий смысл, не копия",
    }
}

/// Находка вида `duplicates`. В умолчания экрана идут точные, близкие, содержащиеся и сильные
/// частичные; смысловые и слабые частичные — по переключателю «Перекрытие содержания» (3.8.2).
fn duplicate_finding(item: &DuplicateItem) -> NewFinding {
    let tag = kind_tag(item.kind);
    let main = match item.kind {
        DuplicateKind::Exact | DuplicateKind::Near | DuplicateKind::Contained => true,
        DuplicateKind::Partial => item.score >= STRONG_PARTIAL,
        DuplicateKind::Semantic => false,
    };
    let mut keys: Vec<String> = item
        .chunks
        .iter()
        .map(|chunk| format!("{}#{}", chunk.path, chunk.heading_chain))
        .collect();
    keys.sort();
    let joined = keys.join("\n");
    let mut reason = kind_reason(item.kind).to_string();
    if item.repeated_block {
        reason.push_str(" (скорее шаблон)");
    }
    let detail = item
        .shared
        .iter()
        .take(2)
        .map(|text| text.chars().take(400).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n…\n");
    NewFinding {
        fingerprint: fingerprint(&["duplicates", tag, &joined]),
        tag: tag.into(),
        group: if main { "main" } else { "overlap" }.into(),
        reason,
        score: Some(item.score),
        subjects: item
            .chunks
            .iter()
            .map(|chunk| {
                subject(
                    &chunk.path,
                    chunk.first_line,
                    chunk.last_line,
                    &chunk.heading_chain,
                )
            })
            .collect(),
        detail,
    }
}

fn run_summary(kind: AuditKind, store: &AuditStore, live: Live) -> Result<AuditSummary, RpcError> {
    let latest = store.latest_run(kind).map_err(audit_error)?;
    let result = store.latest_complete(kind).map_err(audit_error)?;
    let schedule = store.schedule(kind).map_err(audit_error)?;
    let (state, running_since) = match live {
        Live::Running { since } => ("running", Some(since)),
        Live::Queued => ("queued", None),
        Live::Idle => (
            match latest.as_ref().map(|run| run.status) {
                None => "never",
                Some(RunStatus::Complete) => "complete",
                Some(RunStatus::Partial | RunStatus::Cancelled) => "partial",
                Some(RunStatus::Failed) => "failed",
            },
            None,
        ),
    };
    let counts: Option<&RunRow> = result.as_ref();
    let (total, new_count, gone_count, overlap_total) = counts
        .map(|r| store.visible_counts(r.id))
        .transpose()
        .map_err(audit_error)?
        .unwrap_or_default();
    Ok(AuditSummary {
        kind: kind.as_str().to_string(),
        state: state.to_string(),
        running_since,
        last_run_at: latest.as_ref().map(|run| run.finished),
        last_status: latest.as_ref().map(|run| run.status.as_str().to_string()),
        error: latest
            .as_ref()
            .filter(|run| run.status != RunStatus::Complete)
            .map(|run| {
                run.error.clone().unwrap_or_else(|| {
                    if run.status == RunStatus::Cancelled {
                        "Запуск остановлен".to_string()
                    } else {
                        String::new()
                    }
                })
            })
            .filter(|text| !text.is_empty()),
        result_run_id: counts.map(|run| run.id),
        result_at: counts.map(|run| run.finished),
        total,
        new_count,
        gone_count,
        overlap_total,
        schedule: schedule.as_str().to_string(),
        schedules: schedules_of(kind),
    })
}

impl WorkerHandler {
    pub(super) async fn audit_ignore(
        &self,
        params: bars_protocol::AuditIgnoreParams,
    ) -> Result<bars_protocol::AuditIgnoreOutput, RpcError> {
        let project = self.audits_project(&params.project)?;
        let kind = parse_kind(&params.kind)?;
        if params.fingerprint.len() != 32
            || !params.fingerprint.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(RpcError::bars(
                ErrorKind::InvalidParams,
                "некорректный отпечаток находки",
            ));
        }
        let ignored = params.ignored;
        let found = tokio::task::spawn_blocking(move || {
            AuditStore::open(&project)?.set_ignored(kind, &params.fingerprint, ignored)
        })
        .await
        .map_err(internal)?
        .map_err(audit_error)?;
        if !found {
            return Err(RpcError::bars(
                ErrorKind::InvalidParams,
                "находка больше недоступна: обновите список",
            ));
        }
        Ok(bars_protocol::AuditIgnoreOutput { ignored })
    }
    /// Проект вызова: зарегистрированный корень по абсолютному пути или однозначному имени
    /// (ISS-111: раньше принимался только абсолютный путь).
    fn audits_project(&self, project: &str) -> Result<PathBuf, RpcError> {
        let root = self.worker.resolve(project).map_err(worker_error)?;
        if !root.is_dir() {
            return Err(RpcError::bars(
                ErrorKind::ProjectUnavailable,
                "Папки проекта нет на диске",
            ));
        }
        Ok(root)
    }

    /// Префиксы области находок в формах хранения (от корня проекта). Обе формы пути принимаются
    /// (OQ-169); путь, под которым нет документов, — `invalid_params` со списком ближайших,
    /// как область `in` у поиска.
    async fn checked_prefixes(
        &self,
        project: &Path,
        docs_root: &str,
        paths: &[String],
    ) -> Result<Vec<String>, RpcError> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let inputs: Vec<String> = paths.to_vec();
        let forms: Vec<Vec<String>> = paths
            .iter()
            .map(|path| bars_core::doc_path::stored_candidates(docs_root, path))
            .collect();
        let gate = self
            .worker
            .gate(project)
            .map_err(worker_error)?
            .read_owned()
            .await;
        let docs_root = docs_root.to_string();
        tokio::task::spawn_blocking(move || {
            let _gate = gate;
            let index = Index::open_read_only(&index_path(&project)).map_err(internal)?;
            let mut prefixes = Vec::new();
            for (input, candidates) in inputs.iter().zip(&forms) {
                let mut chosen = None;
                for candidate in candidates {
                    let known = index
                        .conn()
                        .prepare_cached(
                            "SELECT EXISTS(SELECT 1 FROM documents \
                             WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\')",
                        )
                        .and_then(|mut statement| {
                            statement.query_row(
                                rusqlite::params![
                                    candidate,
                                    format!("{}/%", bars_core::doc_path::like_escape(candidate))
                                ],
                                |row| row.get::<_, bool>(0),
                            )
                        })
                        .map_err(internal)?;
                    if known {
                        chosen = Some(candidate.clone());
                        break;
                    }
                }
                if chosen.is_none() {
                    let mut statement = index
                        .conn()
                        .prepare("SELECT path FROM documents")
                        .map_err(internal)?;
                    let known = statement
                        .query_map([], |row| row.get::<_, String>(0))
                        .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
                        .map_err(internal)?;
                    return Err(RpcError::bars(
                        ErrorKind::InvalidParams,
                        bars_core::doc_path::unknown_file_message(
                            &docs_root,
                            known.iter().map(String::as_str),
                            input,
                        ),
                    ));
                }
                prefixes.push(chosen.expect("проверено выше"));
            }
            Ok(prefixes)
        })
        .await
        .map_err(internal)?
    }

    pub(super) async fn audit_list(
        &self,
        params: AuditListParams,
    ) -> Result<AuditListOutput, RpcError> {
        let project = self.audits_project(&params.project)?;
        let status = self.worker.status(&project).map_err(worker_error)?;
        let lives: Vec<(AuditKind, Live)> = AuditKind::ALL
            .into_iter()
            .map(|kind| (kind, self.audits.live(&project, kind)))
            .collect();
        let root = project.clone();
        let audits = tokio::task::spawn_blocking(move || {
            let store = AuditStore::open(&root).map_err(audit_error)?;
            lives
                .into_iter()
                .map(|(kind, live)| run_summary(kind, &store, live))
                .collect::<Result<Vec<_>, RpcError>>()
        })
        .await
        .map_err(internal)??;
        Ok(AuditListOutput {
            audits,
            reindex_required: status.state == ProjectState::ReindexRequired,
            indexing: status.state == ProjectState::Indexing || status.queued || status.pending > 0,
        })
    }

    pub(super) async fn audit_run(
        &self,
        params: AuditRunParams,
    ) -> Result<AuditRunOutput, RpcError> {
        let project = self.audits_project(&params.project)?;
        let kind = parse_kind(&params.kind)?;
        self.audit_start(&project, kind)
    }

    /// Запуск вида: сразу, если проект свободен, иначе в очередь проекта.
    fn audit_start(&self, project: &Path, kind: AuditKind) -> Result<AuditRunOutput, RpcError> {
        if self.worker.is_stopping() {
            return Err(RpcError::bars(ErrorKind::Cancelled, "Воркер завершается"));
        }
        match self.audits.request(project, kind) {
            Request::Start(cancel) => {
                self.spawn_audit(project.to_path_buf(), kind, cancel);
                Ok(AuditRunOutput {
                    accepted: true,
                    already: false,
                    queued: false,
                })
            }
            Request::Already => Ok(AuditRunOutput {
                accepted: true,
                already: true,
                queued: false,
            }),
            Request::Queued => Ok(AuditRunOutput {
                accepted: true,
                already: false,
                queued: true,
            }),
            Request::AlreadyQueued => Ok(AuditRunOutput {
                accepted: true,
                already: true,
                queued: true,
            }),
            Request::Deleting => Err(RpcError::bars(
                ErrorKind::ProjectUnavailable,
                "проект удаляется: аудит не запускается",
            )),
        }
    }

    pub(super) fn audit_cancel(
        &self,
        params: AuditCancelParams,
    ) -> Result<AuditCancelOutput, RpcError> {
        let project = self.audits_project(&params.project)?;
        let kind = parse_kind(&params.kind)?;
        Ok(AuditCancelOutput {
            cancelled: self.audits.cancel(&project, kind),
        })
    }

    pub(super) async fn audit_schedule(
        &self,
        params: AuditScheduleParams,
    ) -> Result<AuditScheduleOutput, RpcError> {
        let project = self.audits_project(&params.project)?;
        let kind = parse_kind(&params.kind)?;
        let mode = ScheduleMode::parse(&params.mode)
            .filter(|mode| kind.allows(*mode))
            .ok_or_else(|| {
                RpcError::bars(
                    ErrorKind::InvalidParams,
                    format!(
                        "расписание {} недоступно для вида {}",
                        params.mode, params.kind
                    ),
                )
            })?;
        tokio::task::spawn_blocking(move || {
            AuditStore::open(&project)
                .and_then(|store| store.set_schedule(kind, mode))
                .map_err(audit_error)
        })
        .await
        .map_err(internal)??;
        Ok(AuditScheduleOutput {
            mode: mode.as_str().to_string(),
        })
    }

    /// Сводка находок вида: числа по файлам, причинам и видам без постраничного чтения
    /// (агентский `bars_audit action=summary`).
    pub(super) async fn audit_summary(
        &self,
        params: bars_protocol::AuditSummaryParams,
    ) -> Result<bars_protocol::AuditSummaryOutput, RpcError> {
        let project = self.audits_project(&params.project)?;
        let kind = parse_kind(&params.kind)?;
        let docs_root = self.worker.docs_root(&project).map_err(worker_error)?;
        let prefixes = self
            .checked_prefixes(&project, &docs_root, &params.paths)
            .await?;
        let summary = tokio::task::spawn_blocking(move || {
            let store = AuditStore::open(&project)?;
            let Some(run) = store.latest_complete(kind)? else {
                return Ok(None);
            };
            let summary = store.findings_summary(run.id, &prefixes)?;
            Ok::<_, bars_core::audits::AuditError>(Some((run, summary)))
        })
        .await
        .map_err(internal)?
        .map_err(audit_error)?;
        let Some((run, summary)) = summary else {
            return Ok(bars_protocol::AuditSummaryOutput {
                run_id: None,
                finished: None,
                total: 0,
                files: Vec::new(),
                reasons: Vec::new(),
                tags: Vec::new(),
            });
        };
        Ok(bars_protocol::AuditSummaryOutput {
            run_id: Some(run.id),
            finished: Some(run.finished),
            total: summary.total,
            files: summary
                .files
                .into_iter()
                .map(|(path, count)| bars_protocol::AuditFileCount { path, count })
                .collect(),
            reasons: summary
                .reasons
                .into_iter()
                .map(|(reason, count)| bars_protocol::AuditReasonCount { reason, count })
                .collect(),
            tags: summary
                .tags
                .into_iter()
                .map(|(tag, count)| bars_protocol::AuditTagCount { tag, count })
                .collect(),
        })
    }

    pub(super) async fn audit_findings(
        &self,
        params: AuditFindingsParams,
    ) -> Result<AuditFindingsOutput, RpcError> {
        let project = self.audits_project(&params.project)?;
        let kind = parse_kind(&params.kind)?;
        if let Some(group) = &params.group
            && group != "main"
            && group != "overlap"
        {
            return Err(RpcError::bars(
                ErrorKind::InvalidParams,
                "группа находок: main или overlap",
            ));
        }
        let offset = params.cursor.unwrap_or(0).max(0);
        let limit = params
            .limit
            .map_or(PAGE_DEFAULT, |value| i64::from(value).clamp(1, PAGE_MAX));
        let docs_root = self.worker.docs_root(&project).map_err(worker_error)?;
        let prefixes = self
            .checked_prefixes(&project, &docs_root, &params.paths)
            .await?;
        let page = tokio::task::spawn_blocking(move || {
            let store = AuditStore::open(&project)?;
            let Some(run) = store.latest_complete(kind)? else {
                return Ok(None);
            };
            let page = store.findings(
                run.id,
                params.group.as_deref(),
                params.include_gone,
                params.include_ignored,
                &prefixes,
                offset,
                limit,
            )?;
            Ok::<_, bars_core::audits::AuditError>(Some((run, page)))
        })
        .await
        .map_err(internal)?
        .map_err(audit_error)?;
        let Some((run, page)) = page else {
            return Ok(AuditFindingsOutput {
                run_id: None,
                finished: None,
                items: Vec::new(),
                total: 0,
                next: None,
            });
        };
        Ok(AuditFindingsOutput {
            run_id: Some(run.id),
            finished: Some(run.finished),
            total: page.total,
            next: page.next,
            items: page
                .items
                .into_iter()
                .map(|row| AuditFinding {
                    fingerprint: row.fingerprint,
                    ignored: row.ignored,
                    standing: row.standing.as_str().to_string(),
                    tag: row.tag,
                    group: row.group,
                    reason: row.reason,
                    score: row.score,
                    subjects: row
                        .subjects
                        .into_iter()
                        .map(|s| AuditSubject {
                            path: s.path,
                            first_line: s.first_line,
                            last_line: s.last_line,
                            heading: s.heading,
                            role: s.role,
                        })
                        .collect(),
                    detail: row.detail,
                })
                .collect(),
        })
    }

    /// Фоновое задание: выполняет запуск, затем ждущие виды проекта по одному.
    fn spawn_audit(&self, project: PathBuf, kind: AuditKind, cancel: CancelFlag) {
        let handler = self.clone();
        self.worker.spawn_task(async move {
            let mut current = (kind, cancel);
            loop {
                {
                    let run = handler.run_audit(&project, current.0, &current.1);
                    tokio::pin!(run);
                    tokio::select! {
                        _ = &mut run => {},
                        _ = handler.worker.stopping() => {
                            for kind in AuditKind::ALL { handler.audits.cancel(&project, kind); }
                            current.1.store(true, std::sync::atomic::Ordering::SeqCst);
                            run.await;
                        }
                    }
                }
                match handler.audits.finish(&project) {
                    Some(next) => current = next,
                    None => break,
                }
            }
        });
    }

    async fn run_audit(&self, project: &Path, kind: AuditKind, cancel: &CancelFlag) {
        let started = now();
        let mut outcome = self.compute_audit(project, kind, cancel).await;
        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
            // Остановка перекрывает любой итог: неполный результат прежний полный не затирает.
            outcome = Outcome {
                status: RunStatus::Cancelled,
                error: None,
                stamp: outcome.stamp,
                findings: Vec::new(),
            };
        }
        if self.audits.is_deleting(project) {
            return;
        }
        let root = project.to_path_buf();
        let finished = now();
        let written = tokio::task::spawn_blocking(move || {
            let mut store = AuditStore::open(&root)?;
            store.finish_run(
                kind,
                started,
                finished,
                outcome.status,
                outcome.error.as_deref(),
                outcome.stamp.as_deref(),
                outcome.findings,
            )
        })
        .await;
        match written {
            Ok(Ok(row)) => log::info!(
                "Аудит {}: {} ({} находок, {} новых)",
                kind.as_str(),
                row.status.as_str(),
                row.total,
                row.new_count
            ),
            Ok(Err(error)) => log::warn!("Аудит {}: итог не записан: {error}", kind.as_str()),
            Err(error) => log::warn!("Аудит {}: задание оборвано: {error}", kind.as_str()),
        }
    }

    async fn compute_audit(&self, project: &Path, kind: AuditKind, cancel: &CancelFlag) -> Outcome {
        match kind {
            AuditKind::BrokenLinks => self.compute_broken_links(project).await,
            AuditKind::Duplicates => self.compute_duplicates(project, cancel).await,
            AuditKind::Contradictions => self.compute_contradictions(project, cancel).await,
        }
    }

    async fn compute_broken_links(&self, project: &Path) -> Outcome {
        let docs_root = match self.worker.docs_root(project) {
            Ok(root) => root,
            Err(error) => return Outcome::failed(error.to_string()),
        };
        let lock = std::sync::Arc::clone(&self.links_lock);
        let root = project.to_path_buf();
        let result = tokio::task::spawn_blocking(move || {
            // Единственный писатель индекса ссылок: аудит ждёт вызовы окна, как они ждут друг друга.
            let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
            let mut index = LinkIndex::open(&root).map_err(|e| e.to_string())?;
            index
                .refresh(&root, Path::new(&docs_root))
                .map_err(|e| e.to_string())?;
            let stamp = docs_stamp(&root, Path::new(&docs_root)).map_err(|e| e.to_string())?;
            let broken = index.broken_links().map_err(|e| e.to_string())?;
            Ok::<_, String>((stamp, broken))
        })
        .await;
        match result {
            Ok(Ok((stamp, broken))) => {
                let mut seen: std::collections::HashMap<(String, String, String), usize> =
                    Default::default();
                let findings = broken
                    .iter()
                    .map(|link| {
                        let key = (link.src.clone(), link.raw.clone(), link.text.clone());
                        let count = seen.entry(key).or_insert(0);
                        let finding = broken_finding(link, *count);
                        *count += 1;
                        finding
                    })
                    .collect();
                Outcome {
                    status: RunStatus::Complete,
                    error: None,
                    stamp: Some(stamp),
                    findings,
                }
            }
            Ok(Err(message)) => Outcome::failed(message),
            Err(error) => Outcome::failed(error.to_string()),
        }
    }

    async fn compute_contradictions(&self, project: &Path, cancel: &CancelFlag) -> Outcome {
        let status = match self.worker.status(project) {
            Ok(s) => s,
            Err(e) => return Outcome::failed(e.to_string()),
        };
        if matches!(
            status.state,
            ProjectState::ReindexRequired | ProjectState::Unavailable
        ) {
            return Outcome::failed("Индекс недоступен или требует переиндексации");
        }
        let busy = |s: &crate::worker::ProjectStatus| {
            s.state == ProjectState::Indexing || s.queued || s.pending > 0
        };
        let before = busy(&status);
        let generation = status.updated_at;
        let gate = match self.worker.gate(project) {
            Ok(g) => g.read_owned().await,
            Err(e) => return Outcome::failed(e.to_string()),
        };
        let root = project.to_path_buf();
        let cancel = cancel.clone();
        let result = tokio::task::spawn_blocking(move || {
            let chunks = {
                let _gate = gate;
                let index = Index::open_read_only(&index_path(&root)).map_err(|e|e.to_string())?;
                let mut stmt = index.conn().prepare("SELECT d.path,d.module,c.heading_chain,c.first_line,c.text,c.last_line FROM chunks c JOIN documents d ON d.id=c.document_id ORDER BY d.path,c.ordinal").map_err(|e|e.to_string())?;
                stmt.query_map([], |r| Ok(bars_core::audits::contradictions::Chunk {path:r.get(0)?,module:r.get(1)?,heading:r.get(2)?,first_line:r.get(3)?,text:r.get(4)?,last_line:r.get(5)?})).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?
            };
            bars_core::audits::contradictions::find(&chunks, &cancel)
        }).await;
        let unchanged = self.worker.status(project).is_ok_and(|s| {
            s.state == ProjectState::Ready && !busy(&s) && s.updated_at == generation
        });
        let stamp = generation.map(crate::worker::index_token);
        match result {
            Ok(Ok(findings)) if !before && unchanged => Outcome {
                status: RunStatus::Complete,
                error: None,
                stamp,
                findings,
            },
            Ok(Ok(_)) => Outcome {
                status: RunStatus::Partial,
                error: Some(
                    "Индекс изменился или идёт индексация: результат может быть неполным".into(),
                ),
                stamp,
                findings: Vec::new(),
            },
            Ok(Err(e)) => Outcome {
                status: RunStatus::Partial,
                error: Some(e),
                stamp,
                findings: Vec::new(),
            },
            Err(e) => Outcome::failed(e.to_string()),
        }
    }

    async fn compute_duplicates(&self, project: &Path, cancel: &CancelFlag) -> Outcome {
        let status = match self.worker.status(project) {
            Ok(status) => status,
            Err(error) => return Outcome::failed(error.to_string()),
        };
        match status.state {
            ProjectState::ReindexRequired => {
                return Outcome::failed("Индекс требует переиндексации: сначала обновите индекс");
            }
            ProjectState::Unavailable => {
                return Outcome::failed("Папки проекта нет на диске");
            }
            _ => {}
        }
        let settings = match self.settings(project) {
            Ok(settings) => settings,
            Err(error) => return Outcome::failed(error.message),
        };
        let mut search = settings.search.clone();
        search.max_results = DUPLICATES_CAP;
        let busy = |status: &crate::worker::ProjectStatus| {
            status.state == ProjectState::Indexing || status.queued || status.pending > 0
        };
        let indexing_before = busy(&status);
        let generation = status.updated_at;
        let gate = match self.worker.gate(project) {
            Ok(gate) => gate.read_owned().await,
            Err(error) => return Outcome::failed(error.to_string()),
        };
        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
            return Outcome::failed("Запуск остановлен");
        }
        let root = project.to_path_buf();
        let request = DuplicatesRequest {
            modules: Vec::new(),
            results: Some(DUPLICATES_CAP),
        };
        let result = tokio::task::spawn_blocking(move || {
            let snapshot = {
                let _gate = gate;
                let index = Index::open_read_only(&index_path(&root)).map_err(|e| e.to_string())?;
                duplicates_snapshot(&index, &request).map_err(|e| e.to_string())?
            };
            duplicates_from_snapshot(snapshot, &search, request.results).map_err(|e| e.to_string())
        })
        .await;
        let response = match result {
            Ok(Ok(response)) => response,
            Ok(Err(message)) => return Outcome::failed(message),
            Err(error) => return Outcome::failed(error.to_string()),
        };
        let after = self.worker.status(project);
        let indexing_after = after.as_ref().is_ok_and(busy);
        let changed = !after
            .as_ref()
            .is_ok_and(|s| s.state == ProjectState::Ready && s.updated_at == generation);
        let stamp = generation.map(crate::worker::index_token);
        let findings: Vec<NewFinding> = response.items.iter().map(duplicate_finding).collect();
        let (status, error) = if indexing_before || indexing_after {
            (
                RunStatus::Partial,
                Some("Идёт индексация: результат может быть неполным".to_string()),
            )
        } else if changed {
            (
                RunStatus::Partial,
                Some("Индекс изменился за время расчёта: прежний полный результат сохранён".into()),
            )
        } else if response.total > findings.len() {
            (
                RunStatus::Partial,
                Some(format!(
                    "Найдено {} повторов, учтены первые {}",
                    response.total,
                    findings.len()
                )),
            )
        } else {
            (RunStatus::Complete, None)
        };
        Outcome {
            status,
            error,
            stamp,
            findings,
        }
    }

    /// Есть ли у проекта папка корня документации. Без неё расписание проект пропускает; в журнал
    /// идёт одна запись при переходе в это состояние, возврат папки сбрасывает пометку (ISS-106).
    pub(super) fn schedule_docs_root_present(
        &self,
        project: &Path,
        docs_root: Option<&str>,
    ) -> bool {
        let present = docs_root.is_some_and(|docs| project.join(docs).is_dir());
        let mut blocked = self
            .schedule_blocked
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if present {
            blocked.remove(project);
        } else if blocked.insert(project.to_path_buf()) {
            log::warn!(
                "Расписание аудитов пропускает {}: нет папки корня документации",
                project.display()
            );
        }
        present
    }

    /// Одна проверка расписания: вид запускается, если его режим включён и пришло время.
    pub async fn audit_schedule_tick(&self) {
        if self.worker.is_stopping() {
            return;
        }
        let projects: Vec<PathBuf> = self
            .worker
            .projects()
            .into_iter()
            .filter(|p| p.state != ProjectState::Unavailable)
            .map(|p| p.path)
            .collect();
        for project in projects {
            if self.audits.is_deleting(&project) {
                continue;
            }
            let root = project.clone();
            let docs_root = self.worker.docs_root(&project).ok();
            if !self.schedule_docs_root_present(&project, docs_root.as_deref()) {
                continue;
            }
            let due = tokio::task::spawn_blocking(move || {
                let store = match AuditStore::open(&root) {
                    Ok(store) => store,
                    Err(error) => {
                        log::warn!("Расписание аудитов {}: {error}", root.display());
                        return None;
                    }
                };
                let now = now();
                let mut due = Vec::new();
                // Ленивая метка общая для всех видов, включая ошибку обхода.
                let stamp = std::cell::OnceCell::new();
                for kind in AuditKind::ALL {
                    let (mode, latest) = match store
                        .schedule(kind)
                        .and_then(|mode| store.latest_run(kind).map(|latest| (mode, latest)))
                    {
                        Ok(value) => value,
                        Err(error) => {
                            log::warn!("Расписание аудитов {} {kind:?}: {error}", root.display());
                            continue;
                        }
                    };
                    let is_due = match mode {
                        ScheduleMode::Off => false,
                        ScheduleMode::Daily | ScheduleMode::Weekly => {
                            latest.as_ref().is_none_or(|run| {
                                now - run.finished >= mode.interval_secs().unwrap_or(i64::MAX)
                            })
                        }
                        ScheduleMode::OnChange => {
                            let stamp = stamp.get_or_init(|| {
                                docs_root.as_deref().and_then(|docs| {
                                    match docs_stamp(&root, Path::new(docs)) {
                                        Ok(stamp) => Some(stamp),
                                        Err(error) => {
                                            log::warn!(
                                                "Обход для расписания {}: {error}",
                                                root.display()
                                            );
                                            None
                                        }
                                    }
                                })
                            });
                            stamp.is_some() && latest.as_ref().is_none_or(|run| &run.stamp != stamp)
                        }
                    };
                    if is_due {
                        due.push(kind);
                    }
                }
                Some(due)
            })
            .await
            .unwrap_or_else(|error| {
                log::warn!("Проверка расписания {}: {error}", project.display());
                None
            })
            .unwrap_or_default();
            for kind in due {
                let _ = self.audit_start(&project, kind);
            }
        }
    }

    /// Расписание аудитов: первая проверка через минуту после старта (пропущенный при выключенном
    /// воркере запуск выполняется тогда же), дальше раз в минуту.
    pub fn start_audit_scheduler(&self) {
        let handler = self.clone();
        self.worker.spawn_task(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(SCHEDULE_TICK) => {},
                    _ = handler.worker.stopping() => break,
                }
                handler.audit_schedule_tick().await;
            }
        });
    }

    /// Останавливает аудиты проекта и ждёт их конца: после этого можно удалять файлы проекта.
    pub(super) async fn audits_begin_delete(&self, project: &Path) {
        self.audits.begin_delete(project);
        self.audits.wait_idle(project).await;
    }
}
