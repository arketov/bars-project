//! Перевод ошибок воркера и ядра в ошибки протокола (`docs/architecture/protocol.md`, «Ошибки»).

use std::io;

use bars_core::doc_path::DocPathError;
use bars_core::indexing::walk::ReadError;
use bars_core::models::{EmbedError, ModelFailure};
use bars_core::outline::OutlineError;
use bars_core::search::SearchError;
use bars_protocol::{ErrorKind, INTERNAL_ERROR, RpcError};

use crate::worker::{RegistryError, ResolveError, WorkerError};

pub fn reindex_required() -> RpcError {
    RpcError::bars(
        ErrorKind::ReindexRequired,
        "индекс построен другой моделью или старой схемой: нужна переиндексация, \
         её запускает пользователь (bars-cli reindex --accept-model-change или кнопка в окне)",
    )
}

pub fn worker_error(error: WorkerError) -> RpcError {
    let text = error.to_string();
    match error {
        WorkerError::Resolve(ResolveError::NotFound(_)) => RpcError::bars(
            ErrorKind::ProjectNotFound,
            format!(
                "{text}: bars-cli register <папка проекта>; список — bars_status(all=true), \
                 по части имени — bars_status(filter=\"...\")"
            ),
        ),
        WorkerError::Resolve(ResolveError::UnknownSubproject { .. }) => {
            RpcError::bars(ErrorKind::ProjectNotFound, text)
        }
        WorkerError::NotRegistered(_) => RpcError::bars(
            ErrorKind::ProjectNotFound,
            format!("{text}: bars-cli register <папка проекта>"),
        ),
        WorkerError::Resolve(ResolveError::Ambiguous { .. })
        | WorkerError::NotADirectory(_)
        | WorkerError::Registry(
            RegistryError::NotAbsolute(_)
            | RegistryError::DiscoveredRename
            | RegistryError::InsideWorktree { .. },
        ) => RpcError::bars(ErrorKind::InvalidParams, text),
        WorkerError::Unavailable(_) => RpcError::bars(ErrorKind::ProjectUnavailable, text),
        WorkerError::ConsentRequired(_) => RpcError::bars(
            ErrorKind::ReindexNeedsConsent,
            format!(
                "{text}; пользователь соглашается в окне или командой \
                 bars-cli reindex --accept-model-change"
            ),
        ),
        WorkerError::Registry(_) | WorkerError::Io(_) => RpcError::standard(INTERNAL_ERROR, text),
    }
}

/// Встроенная модель скачивается или грузится: ход скачивания в `data.percent`.
pub fn model_loading(percent: u8) -> RpcError {
    let message = if percent >= 100 {
        "модель загружается в память: повторите поиск через несколько секунд".to_string()
    } else {
        format!("модель скачивается, {percent} %: повторите поиск позже")
    };
    let mut extra = serde_json::Map::new();
    extra.insert("percent".to_string(), percent.into());
    RpcError::bars_with(
        ErrorKind::ModelLoading,
        format!(
            "{message}; проверка актуальности индекса (bars_status refresh=true) загрузку модели не ускоряет"
        ),
        extra,
    )
}

/// Ошибка фоновой загрузки встроенной embed-модели: вид в `data.reason` выбирает экран окна.
pub fn builtin_model_error(failure: ModelFailure) -> RpcError {
    let mut extra = serde_json::Map::new();
    extra.insert("reason".to_string(), failure.kind.as_str().into());
    RpcError::bars_with(ErrorKind::ProviderError, failure.message, extra)
}

pub fn embed_error(error: EmbedError) -> RpcError {
    RpcError::bars(ErrorKind::ProviderError, error.to_string())
}

pub fn search_error(error: SearchError) -> RpcError {
    match error {
        SearchError::InvalidParams(message) => RpcError::bars(ErrorKind::InvalidParams, message),
        SearchError::ReindexRequired => reindex_required(),
        chunk @ SearchError::ChunkNotFound(_) => {
            RpcError::bars(ErrorKind::ChunkNotFound, chunk.to_string())
        }
        SearchError::Embed(error) => embed_error(error),
        other => RpcError::standard(INTERNAL_ERROR, other.to_string()),
    }
}

pub fn doc_path_error(error: DocPathError) -> RpcError {
    let kind = match error {
        DocPathError::NotMarkdown(_) | DocPathError::OutsideProject(_) => ErrorKind::InvalidParams,
        DocPathError::NotFound(_) => ErrorKind::FileNotFound,
    };
    RpcError::bars(kind, error.to_string())
}

pub fn read_error(error: ReadError) -> RpcError {
    match error {
        ReadError::Io(error) if error.kind() == io::ErrorKind::NotFound => {
            RpcError::bars(ErrorKind::FileNotFound, error.to_string())
        }
        ReadError::Io(error) => RpcError::standard(INTERNAL_ERROR, error.to_string()),
        ReadError::NotUtf8 => RpcError::bars(ErrorKind::InvalidParams, error.to_string()),
    }
}

pub fn outline_error(error: OutlineError) -> RpcError {
    RpcError::bars(ErrorKind::InvalidParams, error.to_string())
}

/// `wt` не назвал worktree проекта: текст перечисляет worktree с ветками (агент чаще всего
/// передаёт имя ветки), в `data.worktrees` — `{name, path, branch, locked}`
/// (`docs/features/projects/worktrees.md`, «Ошибка неизвестного wt»).
pub fn worktree_not_found(
    project: &str,
    wanted: &str,
    available: &[bars_core::worktree::LinkedWorktree],
) -> RpcError {
    let message = if available.is_empty() {
        format!("У проекта «{project}» нет worktree; основной checkout — wt=\".\"")
    } else {
        let list = available
            .iter()
            .map(|item| {
                let branch = item.branch.as_deref().map_or_else(
                    || "detached".to_string(),
                    |branch| format!("ветка {branch}"),
                );
                format!("«{}» ({branch}, {})", item.name, item.path.display())
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "У проекта «{project}» нет worktree «{wanted}». Есть: {list}. \
             Основной checkout — wt=\".\""
        )
    };
    let items: Vec<bars_protocol::WorktreeBrief> = available
        .iter()
        .map(|item| bars_protocol::WorktreeBrief {
            name: item.name.clone(),
            path: item.path.display().to_string(),
            branch: item.branch.clone(),
            locked: item.locked.clone(),
        })
        .collect();
    let mut extra = serde_json::Map::new();
    extra.insert(
        "worktrees".to_string(),
        serde_json::to_value(items).unwrap_or_default(),
    );
    RpcError::bars_with(ErrorKind::WorktreeNotFound, message, extra)
}
