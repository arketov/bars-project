//! Индексация проекта: проверка актуальности и полная переиндексация (`docs/features/indexing/updates.md`,
//! Т-9, Т-22, Т-25, НФТ-1).
//!
//! Функции работают с одним проектом и одним индексом. Очередь, слежение за файлами и состояния
//! воркера — дело воркера.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::chunking::{ChunkingParams, chunk_document};
use super::walk::{DocFile, ReadError, Walk, WalkError, WalkProblem, read_utf8, walk_docs_indexed};
use crate::models::{EmbedError, Embedder};
use crate::storage::{
    Compatibility, DocumentRecord, Index, Journal, Layout, StorageError, StoredDocument,
};

/// Настройки, от которых зависит нарезка проекта (`docs/features/settings/settings.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexSettings {
    pub chunking: ChunkingParams,
    /// Корень документации относительно корня проекта (Т-7).
    pub docs_root: String,
}

impl IndexSettings {
    pub(crate) fn layout(&self) -> Layout {
        Layout {
            chunking: self.chunking,
            docs_root: self.docs_root.clone(),
        }
    }
}

/// Файл или папка, которые не удалось обойти или прочитать.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Problem {
    pub stage: &'static str,
    pub attempts: usize,
    pub previous_version: bool,
    /// Путь от корня проекта.
    pub path: String,
    pub reason: String,
}

/// Итог проверки проекта.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct UpdateReport {
    pub aborted: bool,
    pub rebuild_not_applied: bool,
    pub added: usize,
    /// Документы, которые пересобраны: содержимое изменилось или шла полная проверка.
    pub changed: usize,
    pub unchanged: usize,
    pub removed: usize,
    /// Документы, ждущие паузы после правки или явного повтора ошибки.
    pub deferred: usize,
    /// Векторы, которые посчитала модель (одинаковые тексты считаются один раз).
    pub chunks_embedded: usize,
    /// Чанки, для которых модель не вызывалась: вектор взят из индекса или у такого же чанка документа.
    pub chunks_reused: usize,
    /// Нарезка или корень документации изменились: все документы режутся заново.
    pub full_check: bool,
    pub problems: Vec<Problem>,
}

/// Наблюдатель за ходом индексации: воркер показывает по нему прогресс и ставит паузу и остановку
/// (OQ-55). Методы вызываются из потока индексации; пауза — это блокировка в [`Self::should_stop`].
pub trait IndexObserver {
    fn execution(&self, _info: crate::models::ExecutionInfo) {}
    fn report(&self, _report: &UpdateReport) {}
    fn attempt(&self, _attempt: usize) {}
    fn wait_retry(&self, delay: std::time::Duration) -> bool {
        let until = std::time::Instant::now() + delay;
        loop {
            if self.should_stop() {
                return true;
            }
            let remaining = until.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return false;
            }
            std::thread::sleep(remaining.min(std::time::Duration::from_millis(50)));
        }
    }
    /// Обход закончен: столько документов будет просмотрено.
    fn begin(&self, _documents: usize) {}
    /// Документ взят в работу.
    fn file(&self, _path: &str) {}
    /// Расхождения метаданных или содержимого, включая удаления.
    fn discovered(&self, _paths: &[String]) {}
    /// Приоритет документа внутри прохода: старший обрабатывается раньше.
    fn priority(&self, _path: &str) -> u8 {
        0
    }
    /// Итог одного документа. При сбое всего задания вызывается только для завершённых.
    fn document_done(&self, _path: &str, _error: Option<&str>) {}
    /// Документ обработан: всего просмотрено документов и чанков (посчитанных и взятых из индекса).
    fn progress(&self, _documents_done: usize, _chunks_done: usize) {}
    /// Документ ждёт паузы после правки (Т-41) или явного повтора ошибки:
    /// проверка его пропускает, индекс остаётся как был.
    /// `full_check` требует новой нарезки: пауза правки не мешает ей, ошибка ждёт явного повтора.
    fn defer(&self, _path: &str, _full_check: bool) -> bool {
        false
    }
    /// Неблокирующая проверка отмены: допустима под замком подмены индекса.
    fn cancel_requested(&self) -> bool {
        false
    }
    /// Вызывается перед каждым документом. Может ждать, пока снята пауза. `true` — остановить
    /// индексацию: уже записанные документы остаются, при полной переиндексации старый индекс цел.
    fn should_stop(&self) -> bool {
        self.cancel_requested()
    }
}

/// Наблюдатель, который ничего не делает.
pub struct NoObserver;

impl IndexObserver for NoObserver {}

/// Почему индекс нельзя обновлять без переиндексации (Т-25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReindexReason {
    ModelChanged,
    SchemaOutdated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateOutcome {
    Updated(UpdateReport),
    /// Индекс не тронут, эмбеддинг не вызывался.
    ReindexRequired(ReindexReason),
}

#[derive(Debug, thiserror::Error)]
pub enum IndexingError {
    #[error("{error}")]
    DocumentFailed { error: EmbedError, attempts: usize },
    #[error("{source}")]
    Partial {
        report: Box<UpdateReport>,
        source: Box<IndexingError>,
    },
    #[error("Full rebuild not applied: documents failed to embed")]
    Incomplete(Box<UpdateReport>),
    #[error("Stopped after 3 consecutive documents failed to embed")]
    ConsecutiveFailures,
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Walk(#[from] WalkError),
    #[error(transparent)]
    Embed(#[from] EmbedError),
    #[error("ошибка файла: {0}")]
    Io(#[from] io::Error),
    /// Индексацию остановил пользователь (OQ-55).
    #[error("индексация остановлена")]
    Cancelled,
    /// Подмена не удалась. `rollback` — ошибка возврата старого индекса: если она есть, проект
    /// остался без `index.sqlite`, а прежний индекс лежит в `index.sqlite.old` до восстановления.
    #[error("{}", swap_message(.cause, .rollback))]
    Swap {
        cause: io::Error,
        rollback: Option<io::Error>,
    },
}

/// Проверяет актуальность индекса и приводит его в соответствие с диском.
///
/// Порядок и правила — `docs/features/indexing/updates.md`. Ошибка эмбеддинга прерывает проверку:
/// записанные документы остаются, параметры нарезки в индексе не обновляются.
pub fn update_index(
    project_root: &Path,
    index: &mut Index,
    settings: &IndexSettings,
    embedder: &mut dyn Embedder,
) -> Result<UpdateOutcome, IndexingError> {
    update_index_observed(project_root, index, settings, embedder, &NoObserver)
}

/// То же, что [`update_index`], но с наблюдателем за ходом.
pub fn update_index_observed(
    project_root: &Path,
    index: &mut Index,
    settings: &IndexSettings,
    embedder: &mut dyn Embedder,
    observer: &dyn IndexObserver,
) -> Result<UpdateOutcome, IndexingError> {
    update_index_paths_observed(project_root, index, settings, embedder, observer, None)
}

/// Применяет найденные документы/каталоги; смена layout требует всей области.
pub fn update_index_paths_observed(
    project_root: &Path,
    index: &mut Index,
    settings: &IndexSettings,
    embedder: &mut dyn Embedder,
    observer: &dyn IndexObserver,
    paths: Option<&HashSet<String>>,
) -> Result<UpdateOutcome, IndexingError> {
    match index.compatibility(embedder.fingerprint())? {
        Compatibility::ModelChanged => {
            return Ok(UpdateOutcome::ReindexRequired(ReindexReason::ModelChanged));
        }
        Compatibility::SchemaOutdated => {
            return Ok(UpdateOutcome::ReindexRequired(
                ReindexReason::SchemaOutdated,
            ));
        }
        Compatibility::Fresh => {
            if observer.should_stop() {
                return Err(IndexingError::Cancelled);
            }
            embedder.prepare()?;
            index.write_fingerprint(&embedder.fingerprint().clone())?;
        }
        Compatibility::Compatible => {}
    }
    run_update(project_root, index, settings, embedder, observer, paths).map(UpdateOutcome::Updated)
}

/// Строит индекс заново во временном файле рядом с `index_path` и подменяет им старый.
///
/// Старые векторы не используются. Если построение прервалось, старый индекс остаётся целым.
/// Подмена заменяет файл, поэтому соединения со старым индексом должны быть закрыты: иначе
/// на Windows подмена заканчивается ошибкой [`IndexingError::Swap`], а старый индекс остаётся.
pub fn rebuild_index(
    project_root: &Path,
    index_path: &Path,
    settings: &IndexSettings,
    embedder: &mut dyn Embedder,
) -> Result<UpdateReport, IndexingError> {
    rebuild_index_with(project_root, index_path, settings, embedder, || ())
}

/// То же, что [`rebuild_index`], но перед подменой файла вызывает `before_swap`; возвращённое
/// значение живёт до конца подмены. Воркер держит в нём закрытый на запись и чтение доступ к
/// проекту (`docs/architecture/worker.md`): поиск не открывает файл, пока он заменяется.
pub fn rebuild_index_with<G>(
    project_root: &Path,
    index_path: &Path,
    settings: &IndexSettings,
    embedder: &mut dyn Embedder,
    before_swap: impl FnOnce() -> G,
) -> Result<UpdateReport, IndexingError> {
    rebuild_index_observed(
        project_root,
        index_path,
        settings,
        embedder,
        &NoObserver,
        before_swap,
    )
}

/// То же, что [`rebuild_index_with`], но с наблюдателем за ходом: остановка до подмены оставляет
/// старый индекс на месте.
pub fn rebuild_index_observed<G>(
    project_root: &Path,
    index_path: &Path,
    settings: &IndexSettings,
    embedder: &mut dyn Embedder,
    observer: &dyn IndexObserver,
    before_swap: impl FnOnce() -> G,
) -> Result<UpdateReport, IndexingError> {
    let new_path = with_suffix(index_path, ".new");
    let old_path = with_suffix(index_path, ".old");
    remove_index_files(&new_path)?;
    remove_index_files(&old_path)?;

    let built = build_new(project_root, &new_path, settings, embedder, observer);
    match built {
        Ok(mut report) if report.problems.iter().any(|p| p.stage == "embedding") => {
            report.rebuild_not_applied = true;
            let _ = remove_index_files(&new_path);
            Err(IndexingError::Incomplete(Box::new(report)))
        }
        Ok(report) => {
            if observer.should_stop() {
                let _ = remove_index_files(&new_path);
                return Err(IndexingError::Cancelled);
            }
            let _guard = before_swap();
            // Остановка во время последнего документа или ожидания читателей запрещает подмену.
            if observer.cancel_requested() {
                let _ = remove_index_files(&new_path);
                return Err(IndexingError::Cancelled);
            }
            swap(index_path, &new_path, &old_path)?;
            Ok(report)
        }
        Err(mut error) => {
            if let IndexingError::Partial { report, .. } = &mut error {
                report.rebuild_not_applied = true;
            }
            // Остаток неудачной сборки не нужен; ошибку чистки скрываем за исходной ошибкой.
            let _ = remove_index_files(&new_path);
            Err(error)
        }
    }
}

fn build_new(
    project_root: &Path,
    new_path: &Path,
    settings: &IndexSettings,
    embedder: &mut dyn Embedder,
    observer: &dyn IndexObserver,
) -> Result<UpdateReport, IndexingError> {
    let mut index = Index::open(new_path, Journal::Delete)?;
    if observer.should_stop() {
        return Err(IndexingError::Cancelled);
    }
    embedder.prepare()?;
    index.write_fingerprint(&embedder.fingerprint().clone())?;
    let report = run_update(project_root, &mut index, settings, embedder, observer, None)?;
    index.close()?;
    Ok(report)
}

fn run_update(
    project_root: &Path,
    index: &mut Index,
    settings: &IndexSettings,
    embedder: &mut dyn Embedder,
    observer: &dyn IndexObserver,
    paths: Option<&HashSet<String>>,
) -> Result<UpdateReport, IndexingError> {
    let mut walk = walk_docs_indexed(project_root, Path::new(&settings.docs_root))?;
    let layout = settings.layout();
    let mut report = UpdateReport {
        full_check: index.stored_layout()?.as_ref() != Some(&layout),
        ..UpdateReport::default()
    };
    let mut stored: HashMap<String, StoredDocument> = index
        .documents()?
        .into_iter()
        .map(|document| (document.path.clone(), document))
        .collect();

    if let Some(paths) = paths.filter(|_| !report.full_check) {
        walk.files.retain(|file| in_update_scope(paths, &file.path));
        stored.retain(|path, _| in_update_scope(paths, path));
        walk.problems.retain(|problem| {
            in_update_scope(paths, &problem.path)
                || paths
                    .iter()
                    .any(|path| is_covered_by_problem(path, std::slice::from_ref(problem)))
        });
    }
    report
        .problems
        .extend(walk.problems.iter().map(problem_from_walk));

    let on_disk: HashSet<&str> = walk.files.iter().map(|file| file.path.as_str()).collect();
    let changed: Vec<String> = walk
        .files
        .iter()
        .filter(|file| {
            report.full_check
                || stored
                    .get(&file.path)
                    .is_none_or(|old| !same_stat(old, file))
        })
        .map(|file| file.path.clone())
        .chain(
            stored
                .keys()
                .filter(|path| {
                    !on_disk.contains(path.as_str()) && !is_covered_by_problem(path, &walk.problems)
                })
                .cloned(),
        )
        .collect();
    observer.discovered(&changed);
    walk.files
        .sort_by_cached_key(|file| std::cmp::Reverse(observer.priority(&file.path)));
    observer.begin(walk.files.len());
    observer.execution(embedder.execution());
    let mut consecutive = 0;
    let result = (|| {
        for (position, file) in walk.files.iter().enumerate() {
            if observer.should_stop() {
                return Err(IndexingError::Cancelled);
            }
            if observer.defer(&file.path, report.full_check) {
                report.deferred += 1;
                observer.progress(position + 1, report.chunks_embedded + report.chunks_reused);
                continue;
            }
            observer.file(&file.path);
            observer.attempt(0);
            let before = report.chunks_embedded;
            let problems_before = report.problems.len();
            let outcome = process_file(
                index,
                embedder,
                settings,
                file,
                &stored,
                &mut report,
                observer,
            );
            observer.execution(embedder.execution());
            match outcome {
                Err(IndexingError::DocumentFailed { error, attempts }) => {
                    report.problems.push(Problem {
                        path: file.path.clone(),
                        reason: error.to_string(),
                        stage: "embedding",
                        attempts,
                        previous_version: stored.contains_key(&file.path),
                    });
                    consecutive += 1;
                }
                Err(error) => return Err(error),
                Ok(()) if report.chunks_embedded > before => consecutive = 0,
                Ok(()) => {}
            }
            observer.document_done(
                &file.path,
                report.problems[problems_before..]
                    .iter()
                    .rev()
                    .find(|p| p.path == file.path)
                    .map(|p| p.reason.as_str()),
            );
            observer.progress(position + 1, report.chunks_embedded + report.chunks_reused);
            observer.report(&report);
            if consecutive >= 3 {
                return Err(IndexingError::ConsecutiveFailures);
            }
        }
        finish_update(index, &walk, &stored, &layout, &mut report, observer)
    })();
    match result {
        Ok(()) => Ok(report),
        Err(source) => {
            report.aborted = true;
            observer.report(&report);
            Err(IndexingError::Partial {
                report: Box::new(report),
                source: Box::new(source),
            })
        }
    }
}

pub(crate) fn in_update_scope(paths: &HashSet<String>, mut path: &str) -> bool {
    loop {
        if paths.contains(path) {
            return true;
        }
        let Some((parent, _)) = path.rsplit_once('/') else {
            return false;
        };
        path = parent;
    }
}

/// Приводит к виду индекса один файл обхода.
fn process_file(
    index: &mut Index,
    embedder: &mut dyn Embedder,
    settings: &IndexSettings,
    file: &DocFile,
    stored: &HashMap<String, StoredDocument>,
    report: &mut UpdateReport,
    observer: &dyn IndexObserver,
) -> Result<(), IndexingError> {
    let known = stored.get(&file.path);
    if let Some(document) = known
        && !report.full_check
        && same_stat(document, file)
    {
        report.unchanged += 1;
        return Ok(());
    }

    let text = match read_utf8(file) {
        Ok(text) => text,
        Err(ReadError::NotUtf8) => {
            // Файл больше не индексируется: в индексе его быть не должно.
            if known.is_some() && index.delete_document(&file.path)? {
                report.removed += 1;
            }
            report.problems.push(Problem {
                path: file.path.clone(),
                reason: ReadError::NotUtf8.to_string(),
                ..Problem::default()
            });
            return Ok(());
        }
        Err(error @ ReadError::Io(_)) => {
            // Файл может быть занят: документ остаётся в индексе как был.
            report.problems.push(Problem {
                path: file.path.clone(),
                reason: error.to_string(),
                previous_version: known.is_some(),
                ..Problem::default()
            });
            return Ok(());
        }
    };

    let hash = content_hash(&text);
    if let Some(document) = known
        && !report.full_check
        && document.hash == hash
    {
        index.touch_document(&file.path, file.size, modified_ns(file))?;
        report.unchanged += 1;
        return Ok(());
    }

    index_document(
        index, embedder, settings, file, &text, &hash, report, observer,
    )?;
    if known.is_some() {
        report.changed += 1;
    } else {
        report.added += 1;
    }
    Ok(())
}

/// Убирает из индекса пропавшие документы и записывает итог проверки.
fn finish_update(
    index: &mut Index,
    walk: &Walk,
    stored: &HashMap<String, StoredDocument>,
    layout: &Layout,
    report: &mut UpdateReport,
    observer: &dyn IndexObserver,
) -> Result<(), IndexingError> {
    // Граница есть и после последнего документа: не публикуем итог прерванного прохода.
    if observer.should_stop() {
        return Err(IndexingError::Cancelled);
    }
    let on_disk: HashSet<&str> = walk.files.iter().map(|file| file.path.as_str()).collect();
    for path in stored.keys() {
        if on_disk.contains(path.as_str()) || is_covered_by_problem(path, &walk.problems) {
            continue;
        }
        if observer.should_stop() {
            return Err(IndexingError::Cancelled);
        }
        if index.delete_document(path)? {
            observer.file(path);
            report.removed += 1;
            observer.document_done(path, None);
        }
    }

    index.remove_orphan_vectors()?;
    if report.full_check
        && report.deferred == 0
        && !report.problems.iter().any(|p| p.stage == "embedding" || p.previous_version)
        // Пропущенный старый документ всё ещё требует новой нарезки после восстановления доступа.
        && !stored.keys().any(|path| is_covered_by_problem(path, &walk.problems))
    {
        index.write_layout(layout)?;
    }
    // Проверка без изменений ничего не пишет: иначе сбрасывается кэш графа (ISS-103).
    if report.added + report.changed + report.removed > 0 || index.meta("updated_at")?.is_none() {
        index.touch_updated_at()?;
    }
    Ok(())
}

/// Режет документ, добирает векторы только для чанков, которых нет в индексе, и записывает его.
#[allow(clippy::too_many_arguments)]
fn index_document(
    index: &mut Index,
    embedder: &mut dyn Embedder,
    settings: &IndexSettings,
    file: &DocFile,
    text: &str,
    hash: &str,
    report: &mut UpdateReport,
    observer: &dyn IndexObserver,
) -> Result<(), IndexingError> {
    let chunked = chunk_document(&file.path, text, &settings.chunking);

    let mut missing: Vec<(&str, &str)> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for chunk in &chunked.chunks {
        let embed_hash = chunk.embed_hash.as_str();
        if seen.insert(embed_hash) && !index.has_vector(embed_hash)? {
            missing.push((embed_hash, chunk.embed_text.as_str()));
        }
    }

    let vectors = if missing.is_empty() {
        Vec::new()
    } else {
        let texts: Vec<&str> = missing.iter().map(|(_, text)| *text).collect();
        let vectors = embed_with_retries(embedder, &texts, observer)?;
        check_vectors(&vectors, texts.len(), embedder.fingerprint().dim)?;
        check_stored_dim(index, embedder.fingerprint().dim)?;
        vectors
    };

    let new_vectors: Vec<(&str, &[f32])> = missing
        .iter()
        .zip(&vectors)
        .map(|((embed_hash, _), vector)| (*embed_hash, vector.as_slice()))
        .collect();
    let record = DocumentRecord {
        path: &file.path,
        module: &file.module,
        title: &chunked.title,
        size: file.size,
        modified_ns: modified_ns(file),
        hash,
        chunks: &chunked.chunks,
        text: Some(text),
    };
    index.replace_document(&record, &new_vectors)?;

    report.chunks_embedded += missing.len();
    report.chunks_reused += chunked.chunks.len() - missing.len();
    Ok(())
}

pub(crate) fn embed_with_retries(
    embedder: &mut dyn Embedder,
    texts: &[&str],
    observer: &dyn IndexObserver,
) -> Result<Vec<Vec<f32>>, IndexingError> {
    for attempt in 1..=4 {
        if observer.should_stop() {
            return Err(IndexingError::Cancelled);
        }
        observer.attempt(attempt);
        match embedder.embed(texts) {
            Err(EmbedError::Embed(_)) if attempt < 4 => {
                if observer.wait_retry(std::time::Duration::from_secs(1 << (attempt - 1))) {
                    return Err(IndexingError::Cancelled);
                }
            }
            Err(error @ (EmbedError::Embed(_) | EmbedError::Rejected(_))) => {
                return Err(IndexingError::DocumentFailed {
                    error,
                    attempts: attempt,
                });
            }
            result => return result.map_err(IndexingError::from),
        }
    }
    unreachable!("last attempt always returns")
}

/// Размерность внешней модели узнаётся из ответа провайдера и могла смениться при той же модели:
/// вектор другого размера в индексе портит косинусы.
pub(crate) fn check_stored_dim(index: &Index, dim: usize) -> Result<(), IndexingError> {
    match index.stored_fingerprint()? {
        Some(stored) if stored.dim != 0 && dim != 0 && stored.dim != dim => {
            Err(EmbedError::Dimension {
                expected: stored.dim,
                got: dim,
            }
            .into())
        }
        _ => Ok(()),
    }
}

pub(crate) fn check_vectors(
    vectors: &[Vec<f32>],
    expected: usize,
    dim: usize,
) -> Result<(), EmbedError> {
    if vectors.len() != expected {
        return Err(EmbedError::Count {
            expected,
            got: vectors.len(),
        });
    }
    match vectors.iter().find(|vector| vector.len() != dim) {
        Some(vector) => Err(EmbedError::Dimension {
            expected: dim,
            got: vector.len(),
        }),
        None => Ok(()),
    }
}

fn problem_from_walk(problem: &WalkProblem) -> Problem {
    Problem {
        path: problem.path.clone(),
        reason: problem.reason.clone(),
        ..Problem::default()
    }
}

/// Размер и время изменения совпали с индексом: файл не читается. Без времени изменения
/// (файловая система его не отдаёт) файл каждый раз сверяется по хешу.
fn same_stat(document: &StoredDocument, file: &DocFile) -> bool {
    document.size == file.size
        && modified_ns(file).is_some()
        && document.modified_ns == modified_ns(file)
}

/// Хеш документа: blake3 по UTF-8-тексту файла, шестнадцатеричный. Тем же хешем пользуются индексатор
/// и сверка дельты worktree (`worktree_delta`): другой способ дал бы «различия» там, где их нет.
pub fn content_hash(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

pub(crate) fn modified_ns(file: &DocFile) -> Option<i64> {
    let elapsed = file.modified?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(elapsed.as_nanos()).ok()
}

/// Документ в папке или сам файл, которые обход не смог прочитать: его отсутствие в обходе не
/// значит, что файл удалён.
fn is_covered_by_problem(path: &str, problems: &[WalkProblem]) -> bool {
    problems.iter().any(|problem| {
        path == problem.path
            || path
                .strip_prefix(problem.path.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

/// Только метаданные: после копирования снимка SQLite и замок проекта больше не нужны.
pub struct ScanSnapshot {
    stored: HashMap<String, StoredDocument>,
    layout: Option<Layout>,
}
impl ScanSnapshot {
    pub fn read(index: Option<&Index>) -> Result<Self, StorageError> {
        let _snapshot = index
            .map(|i| i.conn().unchecked_transaction())
            .transpose()
            .map_err(StorageError::from)?;
        let stored: HashMap<_, _> = index
            .map(Index::documents)
            .transpose()?
            .unwrap_or_default()
            .into_iter()
            .map(|d| (d.path.clone(), d))
            .collect();
        let layout = index.map(Index::stored_layout).transpose()?.flatten();
        Ok(Self { stored, layout })
    }
}

/// Независимая проверка диска по снимку метаданных, без модели, SQLite и записей.
/// Счётчики описывают найденные различия, а не применённые изменения (OQ-105).
pub fn scan_index(
    project: &Path,
    snapshot: &ScanSnapshot,
    settings: &IndexSettings,
    observer: &dyn IndexObserver,
) -> Result<UpdateReport, IndexingError> {
    let stored = &snapshot.stored;
    let full_check = snapshot.layout.as_ref() != Some(&settings.layout());
    if observer.should_stop() {
        return Err(IndexingError::Cancelled);
    }
    let walk = walk_docs_indexed(project, Path::new(&settings.docs_root))?;
    let mut report = UpdateReport {
        full_check,
        ..Default::default()
    };
    report
        .problems
        .extend(walk.problems.iter().map(problem_from_walk));
    observer.begin(walk.files.len());
    let mut seen = HashSet::new();
    let mut changed = Vec::new();
    for (position, file) in walk.files.iter().enumerate() {
        if observer.should_stop() {
            return Err(IndexingError::Cancelled);
        }
        seen.insert(file.path.as_str());
        let known = stored.get(&file.path);
        if !full_check && known.is_some_and(|d| same_stat(d, file)) {
            report.unchanged += 1;
        } else {
            match read_utf8(file) {
                Ok(text) => match known {
                    None => {
                        report.added += 1;
                        changed.push(file.path.clone());
                    }
                    Some(d)
                        if !full_check
                            && d.hash == blake3::hash(text.as_bytes()).to_hex().as_str() =>
                    {
                        report.unchanged += 1
                    }
                    Some(_) => {
                        report.changed += 1;
                        changed.push(file.path.clone());
                    }
                },
                Err(error) => {
                    changed.push(file.path.clone());
                    report.problems.push(Problem {
                        path: file.path.clone(),
                        reason: error.to_string(),
                        ..Default::default()
                    });
                }
            }
        }
        observer.progress(position + 1, 0);
    }
    let removed: Vec<String> = stored
        .keys()
        .filter(|p| !seen.contains(p.as_str()) && !is_covered_by_problem(p, &walk.problems))
        .cloned()
        .collect();
    report.removed = removed.len();
    changed.extend(removed);
    observer.discovered(&changed);
    Ok(report)
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// Удаляет файл индекса и его спутники SQLite; отсутствующие файлы — не ошибка.
fn remove_index_files(path: &Path) -> io::Result<()> {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        remove_if_exists(&with_suffix(path, suffix))?;
    }
    Ok(())
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn swap(index_path: &Path, new_path: &Path, old_path: &Path) -> Result<(), IndexingError> {
    swap_with(index_path, new_path, old_path, |from, to| {
        fs::rename(from, to)
    })
}

/// Подмена с заменяемым переименованием: тесты проверяют на нём неудачный откат.
fn swap_with(
    index_path: &Path,
    new_path: &Path,
    old_path: &Path,
    rename: impl Fn(&Path, &Path) -> io::Result<()>,
) -> Result<(), IndexingError> {
    let had_old = index_path.exists();
    if had_old {
        // Занятый файл на Windows не переименовывается: подмена не начинается.
        rename(index_path, old_path).map_err(|cause| IndexingError::Swap {
            cause,
            rollback: None,
        })?;
    }
    let installed = clear_sidecars(index_path).and_then(|()| rename(new_path, index_path));
    if let Err(cause) = installed {
        let rollback = if had_old {
            rename(old_path, index_path).err()
        } else {
            None
        };
        return Err(IndexingError::Swap { cause, rollback });
    }
    if had_old {
        // Старый индекс уже не нужен; не удалился — остаётся мусор, но не ошибка.
        let _ = remove_index_files(old_path);
    }
    Ok(())
}

fn swap_message(cause: &io::Error, rollback: &Option<io::Error>) -> String {
    match rollback {
        None => format!("не удалось подменить индекс, старый остался на месте: {cause}"),
        Some(rollback) => format!(
            "подмена индекса не удалась ({cause}), прежний индекс не возвращён ({rollback})"
        ),
    }
}

/// Приводит `index.sqlite`, `index.sqlite.old` и `index.sqlite.new` к одному индексу
/// (`docs/architecture/worker.md`, «Восстановление файлов индекса при старте»).
///
/// Подмена дошла до конца — лишний `.old` удаляется; прервана или откат не удался — `.old`
/// возвращается на место. Недописанный `.new` удаляется всегда. Звать только при старте воркера
/// или под замком задания проекта: идущая переиндексация этим файлом пользуется.
pub fn recover_index_files(index_path: &Path) -> io::Result<()> {
    let old_path = with_suffix(index_path, ".old");
    remove_index_files(&with_suffix(index_path, ".new"))?;
    if !old_path.exists() {
        return Ok(());
    }
    if index_path.exists() {
        return remove_index_files(&old_path);
    }
    clear_sidecars(index_path)?;
    fs::rename(&old_path, index_path)?;
    remove_index_files(&old_path)
}

/// Журнал WAL привязан к имени файла: остаток журнала старого индекса испортил бы новый.
fn clear_sidecars(index_path: &Path) -> io::Result<()> {
    for suffix in ["-wal", "-shm", "-journal"] {
        remove_if_exists(&with_suffix(index_path, suffix))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
