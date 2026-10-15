//! Выполнение одного задания очереди: проверка актуальности или полная переиндексация проекта
//! (`docs/indexing/updates.md`, `docs/architecture/worker.md`).
//!
//! Функция блокирующая: воркер зовёт её из `spawn_blocking`. Индекс открывается на время задания
//! и закрывается в конце: так при подмене файла у воркера нет открытых соединений.

use std::path::Path;
use std::sync::Arc;

use bars_core::indexing::indexer::{
    IndexObserver, IndexingError, ReindexReason, UpdateOutcome, UpdateReport,
    rebuild_index_observed, update_index_observed,
};
use bars_core::models::ModelFingerprint;
use bars_core::settings::Settings;
use bars_core::storage::{Compatibility, Index, Journal, index_path};
use tokio::runtime::Handle;
use tokio::sync::RwLock;

use super::models::Models;
use super::queue::Job;

/// Всё, что нужно заданию от воркера.
pub struct JobEnv<'a> {
    pub bars_home: &'a Path,
    pub models: &'a Models,
    /// Доступ к проекту: поиск держит читающую сторону, подмена индекса берёт пишущую.
    pub gate: Arc<RwLock<()>>,
    pub handle: Handle,
    /// Ход, пауза и остановка задания.
    pub observer: &'a dyn IndexObserver,
}

#[derive(Debug)]
pub enum Outcome {
    Updated(UpdateReport),
    /// Индекс построен другой моделью или старой схемой; индекс не тронут (Т-25).
    ReindexRequired(ReindexReason),
    /// Задание остановил пользователь: проект остаётся как был (OQ-55).
    Cancelled,
    /// Папки проекта нет.
    Unavailable,
    /// Ошибка настроек, модели, индекса или диска; текст без ключей.
    Failed(String),
}

#[derive(Debug)]
pub struct JobResult {
    pub outcome: Outcome,
    /// Отпечаток модели индекса, если он известен.
    pub index_model: Option<String>,
    /// Отпечаток модели из настроек, если настройки прочитались.
    pub settings_model: Option<String>,
}

impl JobResult {
    fn failed(message: impl ToString) -> Self {
        Self {
            outcome: Outcome::Failed(message.to_string()),
            index_model: None,
            settings_model: None,
        }
    }
}

/// `источник:модель` — так отпечаток показывают `status` и окно.
pub fn describe(fingerprint: &ModelFingerprint) -> String {
    format!("{}:{}", fingerprint.source, fingerprint.model)
}

pub fn run(env: &JobEnv<'_>, project: &Path, job: &Job) -> JobResult {
    if !project.is_dir() {
        return JobResult {
            outcome: Outcome::Unavailable,
            index_model: None,
            settings_model: None,
        };
    }
    let settings = match Settings::load(env.bars_home, project) {
        Ok(loaded) => loaded.value,
        Err(error) => return JobResult::failed(error),
    };
    let settings_fingerprint = match env.models.fingerprint(&settings) {
        Ok(fingerprint) => fingerprint,
        Err(error) => return JobResult::failed(error),
    };
    let mut result = JobResult {
        outcome: Outcome::Failed(String::new()),
        index_model: None,
        settings_model: Some(describe(&settings_fingerprint)),
    };
    let outcome = match job.rebuild {
        Some(rebuild) => rebuild_project(
            env,
            project,
            &settings,
            &settings_fingerprint,
            rebuild.accepted,
            &mut result,
        ),
        None => check_project(env, project, &settings, &mut result),
    };
    if matches!(outcome, Outcome::Updated(_)) {
        // Т-8: инструкция сверяется после каждой успешной проверки. Сбой записи индексацию
        // не ломает: файл поправится при следующей проверке.
        let _ = bars_core::instruction::sync(project, &settings.docs_root);
    }
    result.outcome = outcome;
    result
}

fn check_project(
    env: &JobEnv<'_>,
    project: &Path,
    settings: &Settings,
    result: &mut JobResult,
) -> Outcome {
    let mut embedder = match env.models.embedder(settings) {
        Ok(embedder) => embedder,
        Err(error) => return Outcome::Failed(error.to_string()),
    };
    let mut index = match Index::open(&index_path(project), Journal::Wal) {
        Ok(index) => index,
        Err(error) => return Outcome::Failed(error.to_string()),
    };
    let updated = update_index_observed(
        project,
        &mut index,
        &settings.index_settings(),
        &mut embedder,
        env.observer,
    );
    result.index_model = index
        .stored_fingerprint()
        .ok()
        .flatten()
        .map(|fingerprint| describe(&fingerprint));
    let closed = index.close();
    match updated {
        Ok(UpdateOutcome::Updated(report)) => match closed {
            Ok(()) => Outcome::Updated(report),
            Err(error) => Outcome::Failed(error.to_string()),
        },
        Ok(UpdateOutcome::ReindexRequired(reason)) => Outcome::ReindexRequired(reason),
        Err(IndexingError::Cancelled) => Outcome::Cancelled,
        Err(error) => Outcome::Failed(error.to_string()),
    }
}

fn rebuild_project(
    env: &JobEnv<'_>,
    project: &Path,
    settings: &Settings,
    settings_fingerprint: &ModelFingerprint,
    accepted: bool,
    result: &mut JobResult,
) -> Outcome {
    let path = index_path(project);
    if path.exists() {
        // Старый индекс другой моделью без согласия не заменяется (Т-25).
        let index = match Index::open(&path, Journal::Wal) {
            Ok(index) => index,
            Err(error) => return Outcome::Failed(error.to_string()),
        };
        let compatibility = index.compatibility(settings_fingerprint);
        result.index_model = index
            .stored_fingerprint()
            .ok()
            .flatten()
            .map(|fingerprint| describe(&fingerprint));
        if let Err(error) = index.close() {
            return Outcome::Failed(error.to_string());
        }
        match compatibility {
            Ok(Compatibility::ModelChanged) if !accepted => {
                return Outcome::ReindexRequired(ReindexReason::ModelChanged);
            }
            Ok(Compatibility::SchemaOutdated) if !accepted => {
                return Outcome::ReindexRequired(ReindexReason::SchemaOutdated);
            }
            Ok(_) => {}
            Err(error) => return Outcome::Failed(error.to_string()),
        }
    }
    let mut embedder = match env.models.embedder(settings) {
        Ok(embedder) => embedder,
        Err(error) => return Outcome::Failed(error.to_string()),
    };
    let gate = Arc::clone(&env.gate);
    let handle = env.handle.clone();
    let rebuilt = rebuild_index_observed(
        project,
        &path,
        &settings.index_settings(),
        &mut embedder,
        env.observer,
        // Новые поиски ждут, текущие дочитывают; доступ возвращается после подмены файла.
        move || handle.block_on(gate.write_owned()),
    );
    match rebuilt {
        Ok(report) => {
            result.index_model = result.settings_model.clone();
            Outcome::Updated(report)
        }
        Err(IndexingError::Cancelled) => Outcome::Cancelled,
        Err(error) => Outcome::Failed(error.to_string()),
    }
}
