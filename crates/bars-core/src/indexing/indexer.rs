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
use super::walk::{DocFile, ReadError, Walk, WalkError, WalkProblem, read_utf8, walk_docs};
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
    fn layout(&self) -> Layout {
        Layout {
            chunking: self.chunking,
            docs_root: self.docs_root.clone(),
        }
    }
}

/// Файл или папка, которые не удалось обойти или прочитать.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Путь от корня проекта.
    pub path: String,
    pub reason: String,
}

/// Итог проверки проекта.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct UpdateReport {
    pub added: usize,
    /// Документы, которые пересобраны: содержимое изменилось или шла полная проверка.
    pub changed: usize,
    pub unchanged: usize,
    pub removed: usize,
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
    /// Обход закончен: столько документов будет просмотрено.
    fn begin(&self, _documents: usize) {}
    /// Документ взят в работу.
    fn file(&self, _path: &str) {}
    /// Документ обработан: всего просмотрено документов и чанков (посчитанных и взятых из индекса).
    fn progress(&self, _documents_done: usize, _chunks_done: usize) {}
    /// Вызывается перед каждым документом. Может ждать, пока снята пауза. `true` — остановить
    /// индексацию: уже записанные документы остаются, при полной переиндексации старый индекс цел.
    fn should_stop(&self) -> bool {
        false
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
    #[error("не удалось подменить индекс, старый остался на месте: {0}")]
    Swap(io::Error),
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
            embedder.prepare()?;
            index.write_fingerprint(&embedder.fingerprint().clone())?;
        }
        Compatibility::Compatible => {}
    }
    run_update(project_root, index, settings, embedder, observer).map(UpdateOutcome::Updated)
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
        Ok(report) => {
            let _guard = before_swap();
            swap(index_path, &new_path, &old_path)?;
            Ok(report)
        }
        Err(error) => {
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
    embedder.prepare()?;
    index.write_fingerprint(&embedder.fingerprint().clone())?;
    let report = run_update(project_root, &mut index, settings, embedder, observer)?;
    index.close()?;
    Ok(report)
}

fn run_update(
    project_root: &Path,
    index: &mut Index,
    settings: &IndexSettings,
    embedder: &mut dyn Embedder,
    observer: &dyn IndexObserver,
) -> Result<UpdateReport, IndexingError> {
    let walk = walk_docs(project_root, Path::new(&settings.docs_root))?;
    let layout = settings.layout();
    let mut report = UpdateReport {
        full_check: index.stored_layout()?.as_ref() != Some(&layout),
        ..UpdateReport::default()
    };
    report
        .problems
        .extend(walk.problems.iter().map(problem_from_walk));

    let stored: HashMap<String, StoredDocument> = index
        .documents()?
        .into_iter()
        .map(|document| (document.path.clone(), document))
        .collect();

    observer.begin(walk.files.len());
    for (position, file) in walk.files.iter().enumerate() {
        if observer.should_stop() {
            return Err(IndexingError::Cancelled);
        }
        observer.file(&file.path);
        process_file(index, embedder, settings, file, &stored, &mut report)?;
        observer.progress(position + 1, report.chunks_embedded + report.chunks_reused);
    }
    finish_update(index, &walk, &stored, &layout, &mut report)?;
    Ok(report)
}

/// Приводит к виду индекса один файл обхода.
fn process_file(
    index: &mut Index,
    embedder: &mut dyn Embedder,
    settings: &IndexSettings,
    file: &DocFile,
    stored: &HashMap<String, StoredDocument>,
    report: &mut UpdateReport,
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
            });
            return Ok(());
        }
        Err(error @ ReadError::Io(_)) => {
            // Файл может быть занят: документ остаётся в индексе как был.
            report.problems.push(Problem {
                path: file.path.clone(),
                reason: error.to_string(),
            });
            return Ok(());
        }
    };

    let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
    if let Some(document) = known
        && !report.full_check
        && document.hash == hash
    {
        index.touch_document(&file.path, file.size, modified_ns(file))?;
        report.unchanged += 1;
        return Ok(());
    }

    index_document(index, embedder, settings, file, &text, &hash, report)?;
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
) -> Result<(), IndexingError> {
    let on_disk: HashSet<&str> = walk.files.iter().map(|file| file.path.as_str()).collect();
    for path in stored.keys() {
        if !on_disk.contains(path.as_str())
            && !is_covered_by_problem(path, &walk.problems)
            && index.delete_document(path)?
        {
            report.removed += 1;
        }
    }

    index.remove_orphan_vectors()?;
    if report.full_check {
        index.write_layout(layout)?;
    }
    index.touch_updated_at()?;
    Ok(())
}

/// Режет документ, добирает векторы только для чанков, которых нет в индексе, и записывает его.
fn index_document(
    index: &mut Index,
    embedder: &mut dyn Embedder,
    settings: &IndexSettings,
    file: &DocFile,
    text: &str,
    hash: &str,
    report: &mut UpdateReport,
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
        let vectors = embedder.embed(&texts)?;
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
    };
    index.replace_document(&record, &new_vectors)?;

    report.chunks_embedded += missing.len();
    report.chunks_reused += chunked.chunks.len() - missing.len();
    Ok(())
}

/// Размерность внешней модели узнаётся из ответа провайдера и могла смениться при той же модели:
/// вектор другого размера в индексе портит косинусы.
fn check_stored_dim(index: &Index, dim: usize) -> Result<(), IndexingError> {
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

fn check_vectors(vectors: &[Vec<f32>], expected: usize, dim: usize) -> Result<(), EmbedError> {
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
    }
}

/// Размер и время изменения совпали с индексом: файл не читается. Без времени изменения
/// (файловая система его не отдаёт) файл каждый раз сверяется по хешу.
fn same_stat(document: &StoredDocument, file: &DocFile) -> bool {
    document.size == file.size
        && modified_ns(file).is_some()
        && document.modified_ns == modified_ns(file)
}

fn modified_ns(file: &DocFile) -> Option<i64> {
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

/// Подменяет `index_path` готовым `new_path`. Старый индекс уходит в `old_path` и удаляется
/// только после успеха; при ошибке на любом шаге он возвращается на место.
fn swap(index_path: &Path, new_path: &Path, old_path: &Path) -> Result<(), IndexingError> {
    let had_old = index_path.exists();
    if had_old {
        // Занятый файл на Windows не переименовывается: подмена не начинается.
        fs::rename(index_path, old_path).map_err(IndexingError::Swap)?;
    }
    let installed = clear_sidecars(index_path).and_then(|()| fs::rename(new_path, index_path));
    if let Err(error) = installed {
        if had_old {
            let _ = fs::rename(old_path, index_path);
        }
        return Err(IndexingError::Swap(error));
    }
    if had_old {
        // Старый индекс уже не нужен; не удалился — остаётся мусор, но не ошибка.
        let _ = remove_index_files(old_path);
    }
    Ok(())
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
