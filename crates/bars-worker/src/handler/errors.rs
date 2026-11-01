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
        WorkerError::Resolve(ResolveError::NotFound(_)) | WorkerError::NotRegistered(_) => {
            RpcError::bars(
                ErrorKind::ProjectNotFound,
                format!("{text}: bars-cli register <папка проекта>"),
            )
        }
        WorkerError::Resolve(ResolveError::Ambiguous { .. })
        | WorkerError::NotADirectory(_)
        | WorkerError::Registry(RegistryError::NotAbsolute(_)) => {
            RpcError::bars(ErrorKind::InvalidParams, text)
        }
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
        format!("{message}; bars_reindex не ускоряет загрузку модели"),
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
