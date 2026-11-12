//! Поиск и чтение по виду worktree (`docs/features/projects/worktrees.md`, «Поиск по worktree»,
//! «Чтение, структура, точный поиск и содержимое папки», «Чего в worktree нет»).
//!
//! Поиск по готовой дельте сливает индекс владельца и дельту до rerank (`IndexView`); пока дельта
//! строится или упала, ответ идёт по владельцу с пометкой. Чтение, структура, точный поиск и папка
//! работают по диску W без индекса и без дельты. Граф, ссылки и аудиты — у основного checkout.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bars_core::indexing::chunking::document_title;
use bars_core::indexing::walk::{read_utf8_path, walk_docs_bounded};
use bars_core::indexing::worktree_delta::{self, ExcludedPaths};
use bars_core::storage::{Index, StorageError, index_path};
use bars_core::worktree::LinkedWorktree;
use bars_protocol::{DeltaState, ErrorKind, RpcError, SearchWorktree};
use serde_json::{Map, Value, json};

use super::{ViewSelector, WorkerHandler};

/// Подпись файлов баз, по которой кэш исключённых путей понимает, что владелец или дельта
/// изменились: длина и время изменения `index.sqlite` и `index.sqlite-wal` обеих баз. «Поколения»
/// индекса у воркера нет, а подпись стоит четыре `stat`, а не чтения таблиц.
type Stamp = [(u64, u128); 4];

/// Кэш исключённых путей по `(владелец, имя worktree)`; пересчитывается, когда подпись изменилась.
pub(super) type ExcludedCache = Arc<Mutex<HashMap<(PathBuf, String), (Stamp, Arc<ExcludedPaths>)>>>;

fn file_stamp(path: &Path) -> (u64, u128) {
    std::fs::metadata(path).map_or((0, 0), |meta| {
        let modified = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |elapsed| elapsed.as_nanos());
        (meta.len(), modified)
    })
}

fn stamp(owner_db: &Path, delta_db: &Path) -> Stamp {
    let wal = |path: &Path| {
        let mut name = path.as_os_str().to_owned();
        name.push("-wal");
        PathBuf::from(name)
    };
    [
        file_stamp(owner_db),
        file_stamp(&wal(owner_db)),
        file_stamp(delta_db),
        file_stamp(&wal(delta_db)),
    ]
}

/// Исключённые пути вида; список пересчитывается, только если изменился владелец или дельта.
pub(super) fn excluded_for(
    cache: &ExcludedCache,
    project: &Path,
    name: &str,
    owner: &Index,
    delta: &Index,
) -> Result<Arc<ExcludedPaths>, StorageError> {
    let key = (project.to_path_buf(), name.to_string());
    // Подпись берётся до чтения таблиц: правка во время пересчёта сделает её устаревшей.
    let current = stamp(
        &index_path(project),
        &worktree_delta::delta_path(project, name),
    );
    let hit = cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
        .filter(|(cached, _)| *cached == current)
        .map(|(_, list)| Arc::clone(list));
    if let Some(list) = hit {
        return Ok(list);
    }
    let fresh = Arc::new(worktree_delta::excluded_paths(owner, delta)?);
    cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, (current, Arc::clone(&fresh)));
    Ok(fresh)
}

/// Как поиск использует вид worktree: слияние владельца и дельты (дельта готова) или ответ по
/// владельцу с пометкой.
pub(super) struct WtSearchPlan {
    /// Искать по владельцу и дельте.
    pub merge: bool,
    /// Часть `applied.scope.worktree`; цифры слияния дописывает поиск.
    pub info: SearchWorktree,
}

impl WtSearchPlan {
    /// Вид после поиска: цифры слияния и ошибка открытия дельты.
    pub(super) fn finish(
        mut self,
        stats: Option<(usize, usize)>,
        open_error: Option<String>,
    ) -> SearchWorktree {
        if let Some((pending, documents)) = stats {
            self.info.pending = pending as u32;
            self.info.delta_documents = documents as u32;
            if pending > 0 {
                self.info.note = Some(format!(
                    "{pending} документов worktree пересчитываются: пока их не видно в выдаче"
                ));
            }
        }
        if let Some(error) = open_error {
            self.info.state = DeltaState::Error;
            self.info.note = Some(format!(
                "worktree `{}`: индекс недоступен ({error}); ответ по основному checkout",
                self.info.name
            ));
        }
        self.info
    }
}

/// Строка пометки для графа, ссылок и аудитов по виду worktree.
pub(super) fn owner_only_note(name: &str) -> String {
    format!(
        "worktree `{name}`: граф, аудиты и ссылки есть только у основного checkout; \
         показан основной checkout"
    )
}

/// Ошибка `like`, `similar` и `duplicates` по worktree: `invalid_params`, `data.kind =
/// "worktree_unsupported"`, подсказка `wt="."`.
pub(super) fn worktree_unsupported(name: &str, what: &str) -> RpcError {
    let mut extra = Map::new();
    extra.insert("kind".into(), json!("worktree_unsupported"));
    extra.insert("worktree".into(), json!(name));
    RpcError::bars_with(
        ErrorKind::InvalidParams,
        format!(
            "{what} не работает по worktree «{name}»: он строится на ссылках и векторах основного \
             индекса. Основной checkout — wt=\".\""
        ),
        extra,
    )
}

/// Дописывает в объект ответа строку пометки (`worktree_note`); не объект — без изменений.
pub(super) fn with_note(result: Result<Value, RpcError>, note: Option<String>) -> Result<Value, RpcError> {
    match (result, note) {
        (Ok(Value::Object(mut map)), Some(note)) => {
            map.insert("worktree_note".to_string(), Value::String(note));
            Ok(Value::Object(map))
        }
        (result, _) => result,
    }
}

/// Документы базы индекса (владельца или дельты) для дерева окна: путь, модуль, заголовок и число
/// чанков.
pub(super) fn tree_documents(
    db: &Path,
) -> Result<Vec<bars_protocol::TreeDocument>, StorageError> {
    let index = Index::open_read_only(db)?;
    let mut statement = index.conn().prepare(
        "SELECT d.path, d.module, d.title, \
                (SELECT count(*) FROM chunks c WHERE c.document_id = d.id) \
         FROM documents d ORDER BY d.path",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(bars_protocol::TreeDocument {
            path: row.get(0)?,
            module: row.get(1)?,
            title: row.get(2)?,
            chunks: row.get(3)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(StorageError::from)
}

/// Корень вида: корень проекта или корень worktree.
pub(super) fn view_root(project: &Path, view: &ViewSelector) -> PathBuf {
    match view {
        ViewSelector::Main => project.to_path_buf(),
        ViewSelector::Worktree(item) => item.view_root.clone(),
    }
}

/// Документы вида по диску: `(путь, заголовок)` для содержимого папки.
pub(super) fn disk_documents(
    root: &Path,
    docs_root: &str,
) -> Result<Vec<(String, String)>, RpcError> {
    let walk = walk_docs_bounded(root, Path::new(docs_root), || true, usize::MAX)
        .map_err(|error| RpcError::bars(ErrorKind::InvalidParams, error.to_string()))?;
    let mut docs: Vec<(String, String)> = walk
        .files
        .into_iter()
        .map(|file| {
            let title = read_utf8_path(&file.fs_path)
                .map(|text| document_title(&file.path, &text))
                .unwrap_or_default();
            (file.path, title)
        })
        .collect();
    docs.sort();
    Ok(docs)
}

impl WorkerHandler {
    /// Проект и вид вызова, папка проекта которого на месте.
    pub(super) fn usable_view(
        &self,
        target: &bars_protocol::Target,
    ) -> Result<(PathBuf, ViewSelector), RpcError> {
        let (path, view) = self.resolve_view(target)?;
        self.check_available(&path)?;
        Ok((path, view))
    }

    /// План поиска по worktree: первое обращение ставит построение дельты, состояние решает, идёт
    /// ли слияние или ответ по владельцу с пометкой.
    pub(super) fn plan_worktree_search(
        &self,
        project: &Path,
        item: &LinkedWorktree,
    ) -> WtSearchPlan {
        self.worker.ensure_worktree_delta(project, &item.name);
        let status = self.worker.worktree_delta_status(project, &item.name);
        let name = &item.name;
        let (merge, note) = match status.state {
            DeltaState::Ready => (true, None),
            DeltaState::Building => {
                let progress = match (status.done, status.total) {
                    (Some(done), Some(total)) => format!(" ({done} из {total} документов)"),
                    _ => String::new(),
                };
                (
                    false,
                    Some(format!(
                        "worktree `{name}`: индекс строится{progress}; ответ по основному checkout"
                    )),
                )
            }
            DeltaState::None => (
                false,
                Some(format!(
                    "worktree `{name}`: индекс ещё не построен; ответ по основному checkout"
                )),
            ),
            DeltaState::Error => (
                false,
                Some(format!(
                    "worktree `{name}`: индекс не построен ({}); ответ по основному checkout",
                    status.error.as_deref().unwrap_or("ошибка сверки")
                )),
            ),
        };
        WtSearchPlan {
            merge,
            info: SearchWorktree {
                name: item.name.clone(),
                branch: item.branch.clone(),
                state: status.state,
                delta_documents: status.documents,
                pending: status.pending,
                note,
            },
        }
    }

    /// Папка проекта должна быть на месте (`project_unavailable` иначе).
    pub(super) fn check_available(&self, project: &Path) -> Result<(), RpcError> {
        let status = self
            .worker
            .status(project)
            .map_err(super::errors::worker_error)?;
        if status.state == bars_protocol::ProjectState::Unavailable {
            return Err(RpcError::bars(
                ErrorKind::ProjectUnavailable,
                format!("папки проекта {} нет на диске", project.display()),
            ));
        }
        Ok(())
    }

    /// Пометка графа, ссылок и аудитов для вызова по worktree; `None` — вызов по основному.
    pub(super) fn owner_only_note_for(&self, target: &bars_protocol::Target) -> Option<String> {
        match self.resolve_view(target) {
            Ok((_, ViewSelector::Worktree(item))) => Some(owner_only_note(&item.name)),
            _ => None,
        }
    }
}
