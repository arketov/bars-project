//! Итоги живут независимо от опроса окна и текущего отчёта проекта.
use super::job::Outcome;
use super::{Worker, lock, rfc3339};
use bars_protocol::{Operation, OperationEventsOutput, OperationEventsParams, OperationReport};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

static GENERATION: AtomicU64 = AtomicU64::new(1);
pub(super) fn generation() -> u64 {
    GENERATION.fetch_add(1, Ordering::Relaxed)
}

pub(super) fn settings_key(settings: &bars_core::settings::Settings) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    // Не передаём настройки и адреса провайдеров в журнал/UI.
    format!(
        "{:?}{:?}{}",
        settings.embed, settings.chunking, settings.docs_root
    )
    .hash(&mut hash);
    hash.finish()
}

pub(super) fn model_key(settings: &bars_core::settings::Settings, reranker: bool) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    if reranker {
        format!("{:?}", settings.rerank).hash(&mut hash);
    } else {
        format!("{:?}", settings.embed).hash(&mut hash);
    }
    hash.finish()
}

pub(super) struct Journal {
    epoch: String,
    next: u64,
    sequence: u64,
    active: BTreeMap<u64, Operation>,
    completed: VecDeque<Operation>,
    keys: HashMap<u64, u64>,
    pub model_settings: HashMap<u64, bars_core::settings::Settings>,
}

impl Default for Journal {
    fn default() -> Self {
        Self {
            epoch: format!(
                "{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos(),
                generation()
            ),
            next: 0,
            sequence: 0,
            active: BTreeMap::new(),
            completed: VecDeque::new(),
            keys: HashMap::new(),
            model_settings: HashMap::new(),
        }
    }
}

impl Journal {
    #[allow(clippy::too_many_arguments)]
    pub fn begin(
        &mut self,
        kind: &str,
        source: &str,
        project: Option<String>,
        generation: u64,
        model: Option<String>,
        variant: Option<String>,
        reranker: bool,
        key: u64,
    ) -> u64 {
        self.next += 1;
        let id = self.next;
        self.keys.insert(id, key);
        self.active.insert(
            id,
            Operation {
                id,
                sequence: self.sequence,
                kind: kind.into(),
                source: source.into(),
                project,
                generation,
                model,
                variant,
                reranker,
                state: "queued".into(),
                started_at: rfc3339(SystemTime::now()),
                finished_at: None,
                docs_done: None,
                docs_total: None,
                percent: None,
                queue_position: None,
                report: None,
                error: None,
                target_available: true,
            },
        );
        id
    }
    pub fn edit(&mut self, id: u64, f: impl FnOnce(&mut Operation)) {
        if let Some(op) = self.active.get_mut(&id) {
            f(op);
        }
    }
    pub fn finish(
        &mut self,
        id: u64,
        state: &str,
        report: Option<OperationReport>,
        error: Option<String>,
    ) {
        let Some(mut op) = self.active.remove(&id) else {
            return;
        };
        self.model_settings.remove(&id);
        self.sequence += 1;
        op.sequence = self.sequence;
        op.state = state.into();
        op.finished_at = Some(rfc3339(SystemTime::now()));
        op.report = report;
        op.error = error;
        op.percent = None;
        self.completed.push_back(op);
        while self.completed.len() > 1000 {
            if let Some(old) = self.completed.pop_front() {
                self.keys.remove(&old.id);
            }
        }
    }
    pub fn index_result(&mut self, id: u64, outcome: &Outcome, rebuild: bool) {
        let (state, report, error) = match outcome {
            Outcome::Updated(report) => (
                if report.problems.is_empty() {
                    "success"
                } else {
                    "partial"
                },
                Some(report),
                None,
            ),
            Outcome::Interrupted {
                report, cancelled, ..
            } => (
                if *cancelled { "cancelled" } else { "partial" },
                Some(report),
                Some(
                    "Обработка прервана. Проверьте ошибки документов и повторите проверку.".into(),
                ),
            ),
            Outcome::Cancelled => ("cancelled", None, None),
            Outcome::ReindexRequired => (
                "error",
                None,
                Some("Нужна переиндексация с согласием пользователя на замену индекса.".into()),
            ),
            Outcome::Unavailable => ("error", None, Some("Папка проекта недоступна.".into())),
            Outcome::Failed(_) => (
                "error",
                None,
                Some(
                    "Не удалось обновить индекс. Проверьте настройки, доступ к файлам и модели."
                        .into(),
                ),
            ),
        };
        let report = report.map(|r| OperationReport {
            added: r.added,
            changed: r.changed,
            removed: r.removed,
            unchanged: r.unchanged,
            problems: r.problems.len(),
            problem_paths: r
                .problems
                .iter()
                .take(20)
                .map(|p| p.path.chars().take(512).collect())
                .collect(),
            index_applied: !r.rebuild_not_applied && (!rebuild || state == "success"),
        });
        self.finish(id, state, report, error);
    }
    pub fn snapshot(&self, params: &OperationEventsParams) -> OperationEventsOutput {
        let same = params.epoch.as_deref() == Some(&self.epoch);
        let after = params.after.unwrap_or(self.sequence);
        let gap = params.epoch.is_some()
            && (!same
                || after > self.sequence
                || self
                    .completed
                    .front()
                    .is_some_and(|op| after.saturating_add(1) < op.sequence));
        OperationEventsOutput {
            epoch: self.epoch.clone(),
            cursor: self.sequence,
            gap,
            active: self.active.values().cloned().collect(),
            completed: if same {
                self.completed
                    .iter()
                    .filter(|op| op.sequence > after)
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            },
        }
    }
}

pub(super) struct PendingResult(pub std::sync::Arc<std::sync::Mutex<Journal>>, pub u64);
impl Drop for PendingResult {
    fn drop(&mut self) {
        lock(&self.0).finish(
            self.1,
            "cancelled",
            None,
            Some("Задание снято до выполнения.".into()),
        );
    }
}

impl Worker {
    pub fn operation_events(&self, params: &OperationEventsParams) -> OperationEventsOutput {
        let projects = lock(&self.inner.projects);
        let current = lock(&self.inner.current);
        let queued = lock(&self.inner.queue).snapshot();
        let mut journal = lock(&self.inner.models.operations);
        for (position, (_, job)) in queued.iter().enumerate() {
            journal.edit(job.operation_id, |op| {
                op.queue_position = Some(position + usize::from(current.is_some()))
            });
        }
        if let Some(cell) = current.as_ref() {
            let progress = cell.progress();
            journal.edit(cell.operation_id, |op| {
                op.state = if cell.cancel_requested() {
                    "stop_requested"
                } else if cell.paused() {
                    "paused"
                } else if self.is_paused() {
                    "pause_requested"
                } else {
                    "running"
                }
                .into();
                op.docs_done = Some(progress.docs_done);
                op.queue_position = None;
                op.docs_total = cell.total_known().then_some(progress.docs_total);
            });
        }
        let mut output = journal.snapshot(params);
        let keys = journal.keys.clone();
        let model_settings = journal.model_settings.clone();
        // Доступность регистрации относится к тому же снимку, что epoch/cursor.
        for op in output.active.iter_mut().chain(output.completed.iter_mut()) {
            if let Some(path) = &op.project {
                op.target_available = projects
                    .get(std::path::Path::new(path))
                    .is_some_and(|r| !r.deleting && r.generation == op.generation);
            }
        }
        drop(journal);
        drop(current);
        drop(projects);
        // Файлы и provider могут ждать диск; замки воркера здесь уже свободны.
        for op in &mut output.active {
            if let Some(settings) = model_settings.get(&op.id) {
                op.percent = self.models().operation_percent(settings, op.reranker);
            }
        }
        let mut project_settings = HashMap::new();
        let global_settings = bars_core::settings::UserSettings::load(self.bars_home())
            .ok()
            .map(|s| bars_core::settings::Settings::resolve(&s.value, &Default::default()));
        for op in output.active.iter_mut().chain(output.completed.iter_mut()) {
            if op.project.is_some() && op.state == "queued" && self.is_paused() {
                op.state = "queue_paused".into();
            }
            let settings = if let Some(path) = &op.project {
                project_settings
                    .entry(path.clone())
                    .or_insert_with(|| {
                        bars_core::settings::Settings::load(
                            self.bars_home(),
                            std::path::Path::new(path),
                        )
                        .ok()
                        .map(|s| s.value)
                    })
                    .clone()
            } else {
                global_settings.clone().map(|mut settings| {
                    download_target(&mut settings, op);
                    settings
                })
            };
            op.target_available &= settings.as_ref().is_some_and(|settings| {
                keys.get(&op.id)
                    == Some(&if op.project.is_some() {
                        settings_key(settings)
                    } else {
                        model_key(settings, op.reranker)
                    })
            });
        }
        output
    }

    pub fn retry_operation(
        &self,
        params: &bars_protocol::OperationRetryParams,
    ) -> Result<(), super::WorkerError> {
        let _request = self
            .inner
            .requests
            .try_read()
            .map_err(|_| std::io::Error::other("идёт удаление проекта"))?;
        let (op, key) = {
            let journal = lock(&self.inner.models.operations);
            if journal.epoch != params.epoch {
                return Err(std::io::Error::other(
                    "Воркер перезапущен; прежняя операция недоступна",
                )
                .into());
            }
            let op = journal
                .completed
                .iter()
                .find(|op| {
                    op.id == params.id
                        && matches!(op.state.as_str(), "error" | "partial" | "cancelled")
                })
                .cloned()
                .ok_or_else(|| std::io::Error::other("Операция недоступна для повтора"))?;
            let key = journal.keys[&op.id];
            (op, key)
        };
        if let Some(path) = op.project {
            let path = std::path::PathBuf::from(path);
            let settings = bars_core::settings::Settings::load(self.bars_home(), &path)
                .map_err(|_| std::io::Error::other("Не удалось прочитать настройки проекта"))?
                .value;
            let projects = lock(&self.inner.projects);
            if !projects
                .get(&path)
                .is_some_and(|r| !r.deleting && r.generation == op.generation)
            {
                return Err(std::io::Error::other(
                    "Регистрация проекта изменилась; повтор недоступен",
                )
                .into());
            }
            if settings_key(&settings) != key {
                return Err(std::io::Error::other(
                    "Настройки изменились; запустите проверку из проекта",
                )
                .into());
            }
            // Согласие старой попытки не переносится (Т-25).
            match projects[&path].status.state {
                bars_protocol::ProjectState::ReindexRequired => {
                    return Err(super::WorkerError::ConsentRequired(path));
                }
                bars_protocol::ProjectState::Unavailable => {
                    return Err(super::WorkerError::Unavailable(path));
                }
                _ => {}
            }
            if op.kind == "scan" {
                drop(projects);
                self.scan_receipt(&path)?;
                return Ok(());
            }
            let job = if op.kind == "rebuild" {
                super::queue::Job::rebuild(false)
            } else {
                super::queue::Job::check(super::Priority::Explicit)
            };
            self.inner.enqueue_for(&path, job, &projects, key);
        } else {
            let settings = bars_core::settings::UserSettings::load(self.bars_home())
                .map_err(|_| std::io::Error::other("Не удалось прочитать настройки моделей"))?
                .value;
            let mut settings =
                bars_core::settings::Settings::resolve(&settings, &Default::default());
            download_target(&mut settings, &op);
            if model_key(&settings, op.reranker) != key {
                return Err(std::io::Error::other(
                    "Настройки моделей изменились; повторите действие в настройках",
                )
                .into());
            }
            if op.kind == "download" {
                if !self.start_download(op.model.as_deref().unwrap_or_default(), op.reranker) {
                    return Err(std::io::Error::other("Эта модель уже скачивается").into());
                }
            } else {
                self.models()
                    .retry_preparation(&settings, op.reranker)
                    .map_err(std::io::Error::other)?;
            }
        }
        Ok(())
    }
}

fn download_target(settings: &mut bars_core::settings::Settings, op: &Operation) {
    if op.kind != "download" {
        return;
    }
    if op.reranker {
        settings.rerank.model = op.model.clone().unwrap_or_default();
        settings.rerank.source = bars_core::settings::ModelSource::Builtin;
    } else {
        settings.embed.model = op.model.clone().unwrap_or_default();
        settings.embed.source = bars_core::settings::ModelSource::Builtin;
    }
}
