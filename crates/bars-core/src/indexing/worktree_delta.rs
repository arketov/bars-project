//! Дельта worktree: индекс только тех документов вида, которые отличаются от индекса владельца
//! (`docs/features/projects/worktrees.md`, разделы «Хранилище дельты» и «Построение и обновление
//! дельты», Т-48).
//!
//! Файл дельты — `<владелец>/.bars/worktrees/<имя>/index.sqlite`; его открывает [`Index::open`] со
//! схемой индекса владельца плюс таблица `wt_files` ([`open_delta`]). Ядро ничего не знает про
//! замки воркера и очередь: индекс владельца ему показывает [`OwnerView`], а воркер даёт реализацию
//! под замком проекта на чтение. Сверка [`sync`] — один алгоритм для первого построения и любого
//! обновления: обход вида, хеши с пропуском по размеру и времени, сравнение с владельцем, запись
//! отличающихся документов по одному транзакцией, векторы — из дельты, затем из `vectors`
//! владельца, и только затем модель.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::chunking::chunk_document;
use super::indexer::{
    IndexObserver, IndexSettings, IndexingError, Problem, check_stored_dim, check_vectors,
    content_hash, embed_with_retries, in_update_scope, modified_ns,
};
use super::walk::{DocFile, ReadError, WalkError, read_utf8, walk_docs_excluding};
use crate::models::{EmbedError, Embedder, ModelFingerprint};
use crate::storage::{
    DocumentRecord, Index, Journal, Layout, SCHEMA_VERSION, StorageError, StoredDocument,
};

/// Версия формата дельты; дельта с большей версией не открывается и удаляется (она производная).
pub const DELTA_VERSION: u32 = 1;

/// Сколько раз сверка повторяется, если за её время владелец подменился полной переиндексацией.
const MAX_PASSES: usize = 3;

const WT_FILES_SQL: &str = "CREATE TABLE IF NOT EXISTS wt_files (
    path TEXT PRIMARY KEY,
    size INTEGER NOT NULL,
    modified_ns INTEGER,
    hash TEXT NOT NULL
) WITHOUT ROWID;";

/// `<владелец>/.bars/worktrees`: папка всех дельт владельца.
pub fn deltas_dir(owner: &Path) -> PathBuf {
    owner.join(".bars").join("worktrees")
}

/// `<владелец>/.bars/worktrees/<имя>`: папка дельты одного worktree.
pub fn delta_dir(owner: &Path, name: &str) -> PathBuf {
    deltas_dir(owner).join(name)
}

/// `<владелец>/.bars/worktrees/<имя>/index.sqlite`.
pub fn delta_path(owner: &Path, name: &str) -> PathBuf {
    delta_dir(owner, name).join("index.sqlite")
}

#[derive(Debug, thiserror::Error)]
pub enum DeltaError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Walk(#[from] WalkError),
    #[error(transparent)]
    Embed(#[from] EmbedError),
    #[error(transparent)]
    Indexing(#[from] IndexingError),
    /// Метаданные дельты не совпадают с индексом владельца: дельту нужно удалить и построить заново.
    #[error("дельта несовместима с индексом владельца: {0}")]
    Incompatible(String),
    /// У владельца нет индекса, построенного моделью: сверять не с чем.
    #[error("индекс владельца не готов: {0}")]
    OwnerNotReady(String),
    #[error("сверка остановлена")]
    Cancelled,
}

/// Индекс владельца глазами сверки: пути и хеши, отпечаток модели и нарезка.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerSnapshot {
    /// `path → hash` всех документов владельца.
    pub hashes: HashMap<String, String>,
    pub fingerprint: Option<ModelFingerprint>,
    pub layout: Option<Layout>,
    /// blake3 по отсортированным парам `(path, hash)`: меняется, когда владелец ушёл вперёд.
    pub digest: String,
}

impl OwnerSnapshot {
    pub fn read(index: &Index) -> Result<Self, StorageError> {
        let mut pairs: Vec<(String, String)> = index
            .documents()?
            .into_iter()
            .map(|document| (document.path, document.hash))
            .collect();
        pairs.sort();
        let digest = digest_of(&pairs);
        Ok(Self {
            hashes: pairs.into_iter().collect(),
            fingerprint: index.stored_fingerprint()?,
            layout: index.stored_layout()?,
            digest,
        })
    }
}

fn digest_of(sorted: &[(String, String)]) -> String {
    let mut hasher = blake3::Hasher::new();
    for (path, hash) in sorted {
        hasher.update(path.as_bytes());
        hasher.update(&[0]);
        hasher.update(hash.as_bytes());
        hasher.update(&[0]);
    }
    hasher.finalize().to_hex().to_string()
}

/// Подпись владельца для `owner_digest` дельты без чтения всей карты: для проверки после
/// перезапуска воркера.
pub fn owner_digest(index: &Index) -> Result<String, StorageError> {
    Ok(OwnerSnapshot::read(index)?.digest)
}

/// Доступ сверки к индексу владельца. Воркер реализует его под замком проекта на чтение (замок
/// держится на время одного вызова, не на всю сверку); [`IndexOwner`] — прямая реализация.
pub trait OwnerView {
    /// Согласованный снимок документов, отпечатка и нарезки владельца.
    fn snapshot(&self) -> Result<OwnerSnapshot, DeltaError>;
    /// Нормализованные векторы владельца по `embed_hash`; отсутствующие в ответ не входят.
    fn vectors(&self, embed_hashes: &[&str]) -> Result<HashMap<String, Vec<f32>>, DeltaError>;
}

/// Индекс владельца, открытый вызывающим.
pub struct IndexOwner<'a>(pub &'a Index);

impl OwnerView for IndexOwner<'_> {
    fn snapshot(&self) -> Result<OwnerSnapshot, DeltaError> {
        Ok(OwnerSnapshot::read(self.0)?)
    }

    fn vectors(&self, embed_hashes: &[&str]) -> Result<HashMap<String, Vec<f32>>, DeltaError> {
        let mut found = HashMap::new();
        for hash in embed_hashes {
            if let Some(vector) = self.0.vector(hash)? {
                found.insert((*hash).to_string(), vector);
            }
        }
        Ok(found)
    }
}

/// Открывает дельту на запись: создаёт файл и папку, если их нет; добавляет `wt_files` и ключи
/// `delta_version`, `worktree_name`.
pub fn open_delta(owner: &Path, name: &str) -> Result<Index, DeltaError> {
    let delta = Index::open(&delta_path(owner, name), Journal::Wal)?;
    delta.conn().execute_batch(WT_FILES_SQL)?;
    if delta.meta("delta_version")?.is_none() {
        delta.set_meta("delta_version", &DELTA_VERSION.to_string())?;
    }
    delta.set_meta("worktree_name", name)?;
    Ok(delta)
}

/// Подходит ли дельта индексу владельца.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeltaCheck {
    /// Новая дельта: модель и нарезка в ней ещё не записаны (построение не дошло до конца).
    Fresh,
    Compatible,
    /// Расхождение метаданных (модель, нарезка, `docs_root`, формат): дельту надо удалить.
    Mismatch(String),
}

/// Сравнивает `meta` дельты с владельцем и настройками: отпечаток модели, `chunking_version`,
/// параметры нарезки, `docs_root`, версия формата. Читает только `meta`, подходит и для
/// `Index::open_read_only`.
pub fn check_delta(
    delta: &Index,
    owner: &OwnerSnapshot,
    settings: &IndexSettings,
) -> Result<DeltaCheck, StorageError> {
    let version = delta
        .meta("delta_version")?
        .and_then(|value| value.parse::<u32>().ok());
    match version {
        None => return Ok(DeltaCheck::Mismatch("нет версии формата дельты".into())),
        Some(version) if version > DELTA_VERSION => {
            return Ok(DeltaCheck::Mismatch(format!(
                "формат дельты {version} новее известного {DELTA_VERSION}"
            )));
        }
        Some(_) => {}
    }
    let schema = delta
        .meta("schema_version")?
        .and_then(|value| value.parse::<u32>().ok());
    if schema.is_none_or(|schema| schema > SCHEMA_VERSION) {
        return Ok(DeltaCheck::Mismatch("схема дельты не открывается".into()));
    }
    let Some(stored) = delta.stored_fingerprint()? else {
        return Ok(DeltaCheck::Fresh);
    };
    if owner
        .fingerprint
        .as_ref()
        .is_none_or(|fingerprint| !stored.matches(fingerprint))
    {
        return Ok(DeltaCheck::Mismatch("другая модель embed".into()));
    }
    match delta.stored_layout()? {
        None => Ok(DeltaCheck::Mismatch("нарезка дельты не записана".into())),
        Some(layout)
            if layout != settings.layout()
                || owner.layout.as_ref().is_some_and(|owner| *owner != layout) =>
        {
            Ok(DeltaCheck::Mismatch(
                "другая нарезка или корень документации".into(),
            ))
        }
        Some(_) => Ok(DeltaCheck::Compatible),
    }
}

/// Что сверять: все документы вида или перечисленные пути (от корня проекта через `/`; папка
/// охватывает документы под ней).
#[derive(Debug, Clone, Copy)]
pub enum SyncScope<'a> {
    All,
    Paths(&'a HashSet<String>),
}

/// Входные данные сверки.
pub struct SyncRequest<'a> {
    /// Корень вида: `W/<смещение владельца>`.
    pub view_root: &'a Path,
    pub settings: &'a IndexSettings,
    /// Исключённые области (вложенные проекты владельца), пути от корня проекта через `/`.
    pub excluded: &'a [String],
    pub scope: SyncScope<'a>,
    /// Имя worktree: пишется в `meta`.
    pub name: &'a str,
}

/// Итог сверки.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncReport {
    /// Документов в виде (в области сверки), для которых известен хеш.
    pub view_documents: usize,
    /// Документов, чьи размер и время совпали с `wt_files`: файл не читался.
    pub skipped_by_stat: usize,
    /// Документов, прочитанных и захешированных.
    pub hashed: usize,
    /// Документов дельты, записанных этой сверкой.
    pub written: usize,
    /// Документов, ушедших из дельты: сошлись с владельцем или пропали из вида.
    pub removed: usize,
    /// Документов в дельте после сверки (все, не только в области).
    pub delta_documents: usize,
    /// Векторы, которые посчитала модель.
    pub chunks_embedded: usize,
    /// Векторы, скопированные из индекса владельца.
    pub chunks_copied: usize,
    /// Чанки, у которых вектор уже был в дельте или у такого же чанка документа.
    pub chunks_reused: usize,
    /// Сколько раз сверка начиналась заново: владелец подменился за её время.
    pub restarts: usize,
    pub problems: Vec<Problem>,
    /// Подпись владельца, под которой сверка завершена (полная сверка записывает её в `meta`).
    pub owner_digest: String,
}

#[derive(Debug, Clone)]
struct ViewEntry {
    size: u64,
    modified_ns: Option<i64>,
    hash: String,
}

/// Сверка дельты с видом. Первое построение и обновление — один алгоритм
/// (`worktrees.md`, «Первое построение и любая сверка»). Дельта открыта вызывающим на запись
/// ([`open_delta`]); индекс владельца только читается через `owner`.
///
/// Полная сверка по завершении пишет `owner_digest`, `synced_at` и `ready = 1`; сверка по путям
/// `ready` не ставит. Ошибка и остановка оставляют записанные документы: поиск видит прежнюю или
/// новую версию документа целиком, а `ready` не появляется, пока полная сверка не дошла до конца.
pub fn sync(
    delta: &mut Index,
    owner: &dyn OwnerView,
    embedder: &mut dyn Embedder,
    request: &SyncRequest<'_>,
    observer: &dyn IndexObserver,
) -> Result<SyncReport, DeltaError> {
    let mut restarts = 0;
    let mut problems: Vec<Problem> = Vec::new();
    loop {
        if observer.should_stop() {
            return Err(DeltaError::Cancelled);
        }
        let snapshot = owner.snapshot()?;
        prepare(delta, &snapshot, request, embedder)?;
        let mut report = pass(delta, owner, embedder, &snapshot, request, observer)?;
        // Владелец мог подмениться полной переиндексацией: тогда прежний снимок устарел.
        let after = owner.snapshot()?;
        if after.digest != snapshot.digest && restarts + 1 < MAX_PASSES {
            restarts += 1;
            problems.extend(std::mem::take(&mut report.problems));
            continue;
        }
        if matches!(request.scope, SyncScope::All) {
            delta.set_meta("owner_digest", &after.digest)?;
            delta.set_meta("synced_at", &unix_seconds().to_string())?;
            delta.set_meta("ready", "1")?;
        }
        problems.extend(std::mem::take(&mut report.problems));
        report.problems = problems;
        report.restarts = restarts;
        report.owner_digest = after.digest;
        report.delta_documents = delta.document_count()?;
        return Ok(report);
    }
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Проверяет совместимость и при первой сверке записывает модель и нарезку владельца.
fn prepare(
    delta: &mut Index,
    owner: &OwnerSnapshot,
    request: &SyncRequest<'_>,
    embedder: &dyn Embedder,
) -> Result<(), DeltaError> {
    let Some(fingerprint) = owner.fingerprint.clone() else {
        return Err(DeltaError::OwnerNotReady(
            "в индексе владельца нет отпечатка модели".into(),
        ));
    };
    if !fingerprint.matches(embedder.fingerprint()) {
        return Err(DeltaError::Incompatible(
            "модель embed в настройках не совпадает с индексом владельца".into(),
        ));
    }
    match check_delta(delta, owner, request.settings)? {
        DeltaCheck::Mismatch(reason) => return Err(DeltaError::Incompatible(reason)),
        DeltaCheck::Fresh => {
            delta.write_fingerprint(&fingerprint)?;
            delta.write_layout(&request.settings.layout())?;
        }
        DeltaCheck::Compatible => {}
    }
    delta.set_meta("delta_version", &DELTA_VERSION.to_string())?;
    delta.set_meta("worktree_name", request.name)?;
    Ok(())
}

fn pass(
    delta: &mut Index,
    owner: &dyn OwnerView,
    embedder: &mut dyn Embedder,
    snapshot: &OwnerSnapshot,
    request: &SyncRequest<'_>,
    observer: &dyn IndexObserver,
) -> Result<SyncReport, DeltaError> {
    let mut report = SyncReport::default();
    let mut walk = walk_docs_excluding(
        request.view_root,
        Path::new(&request.settings.docs_root),
        || !observer.should_stop(),
        usize::MAX,
        request.excluded,
    )?;
    if observer.should_stop() {
        return Err(DeltaError::Cancelled);
    }
    let in_scope = |path: &str| match request.scope {
        SyncScope::All => true,
        SyncScope::Paths(paths) => in_update_scope(paths, path),
    };
    walk.files.retain(|file| in_scope(&file.path));
    // Папка, которую обход не прочитал, не значит «документы удалены».
    let unreadable: Vec<String> = walk
        .problems
        .iter()
        .map(|problem| problem.path.clone())
        .collect();
    report.problems.extend(walk.problems.iter().map(|problem| Problem {
        path: problem.path.clone(),
        reason: problem.reason.clone(),
        ..Problem::default()
    }));

    let mut known = read_wt_files(delta)?;
    let mut delta_documents: HashMap<String, StoredDocument> = delta
        .documents()?
        .into_iter()
        .map(|document| (document.path.clone(), document))
        .collect();

    observer.begin(walk.files.len());
    // Шаги 3–4: хеши. Текст остаётся в памяти только у документов, которые отличаются от владельца.
    let mut view: HashMap<String, ViewEntry> = HashMap::new();
    let mut texts: HashMap<String, String> = HashMap::new();
    let mut busy: HashSet<String> = HashSet::new();
    let mut gone: Vec<String> = Vec::new();
    for (position, file) in walk.files.iter().enumerate() {
        if observer.should_stop() {
            return Err(DeltaError::Cancelled);
        }
        let modified = modified_ns(file);
        let stored = known.get(&file.path);
        if let Some(entry) = stored
            && modified.is_some()
            && entry.size == file.size
            && entry.modified_ns == modified
        {
            report.skipped_by_stat += 1;
            view.insert(file.path.clone(), entry.clone());
        } else {
            match read_utf8(file) {
                Ok(text) => {
                    let hash = content_hash(&text);
                    report.hashed += 1;
                    if snapshot.hashes.get(&file.path) != Some(&hash) {
                        texts.insert(file.path.clone(), text);
                    }
                    view.insert(
                        file.path.clone(),
                        ViewEntry {
                            size: file.size,
                            modified_ns: modified,
                            hash,
                        },
                    );
                }
                Err(ReadError::NotUtf8) => {
                    // Как у индексатора: файл больше не индексируется, из вида он выпадает.
                    gone.push(file.path.clone());
                    report.problems.push(Problem {
                        path: file.path.clone(),
                        reason: ReadError::NotUtf8.to_string(),
                        ..Problem::default()
                    });
                }
                Err(error @ ReadError::Io(_)) => {
                    // Файл может быть занят: документ остаётся таким, каким был.
                    busy.insert(file.path.clone());
                    report.problems.push(Problem {
                        path: file.path.clone(),
                        reason: error.to_string(),
                        previous_version: stored.is_some(),
                        ..Problem::default()
                    });
                }
            }
        }
        observer.progress(position + 1, 0);
    }
    report.view_documents = view.len();

    // wt_files: обход в области заменяет записи, пропавшие файлы области удаляются.
    let on_disk: HashSet<&str> = walk.files.iter().map(|file| file.path.as_str()).collect();
    let missing: Vec<String> = known
        .keys()
        .filter(|path| {
            in_scope(path)
                && !on_disk.contains(path.as_str())
                && !covered_by(path, &unreadable)
                && !busy.contains(path.as_str())
        })
        .cloned()
        .chain(gone.iter().cloned())
        .collect();
    write_wt_files(delta, &view, &missing)?;
    for path in &missing {
        known.remove(path);
    }

    // Шаг 4: что отличается. Документ дельты, который больше не отличается или пропал, уходит.
    let mut to_write: Vec<String> = Vec::new();
    for (path, entry) in &view {
        let differs = snapshot.hashes.get(path) != Some(&entry.hash);
        let current = delta_documents.get(path).map(|document| &document.hash);
        if differs && current != Some(&entry.hash) {
            to_write.push(path.clone());
        }
        if !differs && current.is_some() {
            remove_document(delta, path)?;
            delta_documents.remove(path);
            report.removed += 1;
        }
    }
    for path in &missing {
        if delta_documents.remove(path).is_some() {
            remove_document(delta, path)?;
            report.removed += 1;
        }
    }
    // Из дельты уходят и документы области, которых нет в виде (не занятые и не под ошибкой обхода).
    let stale: Vec<String> = delta_documents
        .keys()
        .filter(|path| {
            in_scope(path)
                && !view.contains_key(path.as_str())
                && !busy.contains(path.as_str())
                && !covered_by(path, &unreadable)
        })
        .cloned()
        .collect();
    for path in stale {
        remove_document(delta, &path)?;
        delta_documents.remove(&path);
        report.removed += 1;
    }

    // Шаг 5: запись по одному документу.
    to_write.sort();
    let files: HashMap<&str, &DocFile> = walk
        .files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    for path in &to_write {
        if observer.should_stop() {
            return Err(DeltaError::Cancelled);
        }
        let file = files[path.as_str()];
        let (text, hash) = match texts.remove(path) {
            Some(text) => {
                let hash = view[path].hash.clone();
                (text, hash)
            }
            // Хеш пришёл из `wt_files`, текст ещё не читался.
            None => match read_utf8(file) {
                Ok(text) => {
                    let hash = content_hash(&text);
                    (text, hash)
                }
                Err(error) => {
                    report.problems.push(Problem {
                        path: path.clone(),
                        reason: error.to_string(),
                        previous_version: delta_documents.contains_key(path),
                        ..Problem::default()
                    });
                    continue;
                }
            },
        };
        // Файл мог измениться между хешем и чтением: пишется то, что прочитано сейчас.
        if snapshot.hashes.get(path) == Some(&hash) {
            continue;
        }
        write_document(
            delta,
            owner,
            embedder,
            request.settings,
            file,
            &text,
            &hash,
            &mut report,
            observer,
        )?;
        report.written += 1;
    }
    delta.remove_orphan_vectors()?;
    Ok(report)
}

fn covered_by(path: &str, problems: &[String]) -> bool {
    problems.iter().any(|problem| {
        path == problem
            || path
                .strip_prefix(problem.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

fn read_wt_files(delta: &Index) -> Result<HashMap<String, ViewEntry>, StorageError> {
    let mut statement = delta
        .conn()
        .prepare("SELECT path, size, modified_ns, hash FROM wt_files")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            ViewEntry {
                size: row.get::<_, i64>(1)? as u64,
                modified_ns: row.get(2)?,
                hash: row.get(3)?,
            },
        ))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Все документы вида: `(path, size, modified_ns, hash)` из `wt_files`.
pub fn view_files(delta: &Index) -> Result<Vec<StoredDocument>, StorageError> {
    let mut statement = delta
        .conn()
        .prepare("SELECT path, size, modified_ns, hash FROM wt_files ORDER BY path")?;
    let rows = statement.query_map([], |row| {
        Ok(StoredDocument {
            path: row.get(0)?,
            size: row.get::<_, i64>(1)? as u64,
            modified_ns: row.get(2)?,
            hash: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn write_wt_files(
    delta: &mut Index,
    view: &HashMap<String, ViewEntry>,
    missing: &[String],
) -> Result<(), StorageError> {
    let tx = delta.conn_mut().transaction()?;
    {
        let mut upsert = tx.prepare_cached(
            "INSERT INTO wt_files(path, size, modified_ns, hash) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET size = excluded.size,
                 modified_ns = excluded.modified_ns, hash = excluded.hash",
        )?;
        for (path, entry) in view {
            upsert.execute(rusqlite::params![
                path,
                entry.size as i64,
                entry.modified_ns,
                entry.hash
            ])?;
        }
        let mut delete = tx.prepare_cached("DELETE FROM wt_files WHERE path = ?1")?;
        for path in missing {
            delete.execute([path])?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn remove_document(delta: &mut Index, path: &str) -> Result<(), StorageError> {
    delta.delete_document(path)?;
    Ok(())
}

/// Режет документ и пишет его в дельту: вектор чанка берётся из дельты, затем у владельца,
/// иначе считается моделью. Запись документа — одна транзакция.
#[allow(clippy::too_many_arguments)]
fn write_document(
    delta: &mut Index,
    owner: &dyn OwnerView,
    embedder: &mut dyn Embedder,
    settings: &IndexSettings,
    file: &DocFile,
    text: &str,
    hash: &str,
    report: &mut SyncReport,
    observer: &dyn IndexObserver,
) -> Result<(), DeltaError> {
    let chunked = chunk_document(&file.path, text, &settings.chunking);

    let mut needed: Vec<(&str, &str)> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for chunk in &chunked.chunks {
        let embed_hash = chunk.embed_hash.as_str();
        if seen.insert(embed_hash) && !delta.has_vector(embed_hash)? {
            needed.push((embed_hash, chunk.embed_text.as_str()));
        }
    }
    let wanted: Vec<&str> = needed.iter().map(|(hash, _)| *hash).collect();
    let copied = if wanted.is_empty() {
        HashMap::new()
    } else {
        owner.vectors(&wanted)?
    };
    let to_embed: Vec<(&str, &str)> = needed
        .iter()
        .filter(|(embed_hash, _)| !copied.contains_key(*embed_hash))
        .copied()
        .collect();
    let computed = if to_embed.is_empty() {
        Vec::new()
    } else {
        let texts: Vec<&str> = to_embed.iter().map(|(_, text)| *text).collect();
        let vectors = embed_with_retries(embedder, &texts, observer)?;
        check_vectors(&vectors, texts.len(), embedder.fingerprint().dim)?;
        check_stored_dim(delta, embedder.fingerprint().dim)?;
        vectors
    };

    let mut new_vectors: Vec<(&str, &[f32])> = Vec::with_capacity(needed.len());
    for (embed_hash, _) in &needed {
        if let Some(vector) = copied.get(*embed_hash) {
            new_vectors.push((*embed_hash, vector.as_slice()));
        }
    }
    for ((embed_hash, _), vector) in to_embed.iter().zip(&computed) {
        new_vectors.push((*embed_hash, vector.as_slice()));
    }
    let record = DocumentRecord {
        path: &file.path,
        module: &file.module,
        title: &chunked.title,
        size: file.size,
        modified_ns: modified_ns(file),
        hash,
        chunks: &chunked.chunks,
        // Графа и аудитов у worktree нет: текст не хранится.
        text: None,
    };
    delta.replace_document(&record, &new_vectors)?;

    report.chunks_embedded += to_embed.len();
    report.chunks_copied += copied.len();
    report.chunks_reused += chunked.chunks.len() - to_embed.len() - copied.len();
    Ok(())
}

/// Состояние готовности дельты из файла: `ready = 1`.
pub fn is_ready(delta: &Index) -> Result<bool, StorageError> {
    Ok(delta.meta("ready")?.as_deref() == Some("1"))
}

/// Подпись владельца и время последней полной сверки из `meta`.
pub fn stored_sync(delta: &Index) -> Result<(Option<String>, Option<u64>), StorageError> {
    Ok((
        delta.meta("owner_digest")?,
        delta
            .meta("synced_at")?
            .and_then(|value| value.parse().ok()),
    ))
}

/// Пути владельца, которых поиск по виду не использует (`worktrees.md`, «Допустимые документы
/// владельца»).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExcludedPaths {
    /// Документы владельца, которых в виде нет, чью версию вида даёт дельта или где владелец успел
    /// измениться, а дельта ещё не пересчитана; по возрастанию пути.
    pub paths: Vec<String>,
    /// Документы, версия которых в виде сейчас не видна или устарела: идёт пересчёт дельты.
    pub pending: usize,
    /// Документов в дельте.
    pub delta_documents: usize,
}

/// Исключённые пути из трёх таблиц: `documents` владельца, `wt_files` и `documents` дельты.
/// Документ владельца допустим в виде, если его `path` есть в `wt_files`, `hash` равен хешу вида и
/// `path` отсутствует в `documents` дельты.
pub fn excluded_paths(owner: &Index, delta: &Index) -> Result<ExcludedPaths, StorageError> {
    let view: HashMap<String, String> = view_files(delta)?
        .into_iter()
        .map(|document| (document.path, document.hash))
        .collect();
    let in_delta: HashMap<String, String> = delta
        .documents()?
        .into_iter()
        .map(|document| (document.path, document.hash))
        .collect();
    let mut excluded = ExcludedPaths {
        delta_documents: in_delta.len(),
        ..ExcludedPaths::default()
    };
    for document in owner.documents()? {
        let seen = view.get(&document.path);
        let admitted = seen == Some(&document.hash) && !in_delta.contains_key(&document.path);
        if admitted {
            continue;
        }
        // Версии вида владелец не отдаёт и дельта ещё не записала: документ временно не виден.
        if seen.is_some_and(|hash| *hash != document.hash) && !in_delta.contains_key(&document.path)
        {
            excluded.pending += 1;
        }
        excluded.paths.push(document.path);
    }
    // Дельта хранит прежнюю версию документа: вид уже изменился, пересчёт не закончен.
    excluded.pending += in_delta
        .iter()
        .filter(|(path, hash)| view.get(*path) != Some(*hash))
        .count();
    excluded.paths.sort_unstable();
    Ok(excluded)
}

#[cfg(test)]
mod tests;
