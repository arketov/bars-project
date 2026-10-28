//! Внутренний API окна для ссылок документа (`docs/features/knowledge-base/link-index.md`).
//!
//! Каждый вызов сверяет индекс ссылок с файлами на диске (изменившиеся разбираются заново, остальные
//! пропускаются по размеру и времени) и отвечает по свежему состоянию. Модель не нужна.

use std::path::Path;

use bars_core::link_index::{LinkIndex, LinkIndexError};
use bars_protocol::{
    ErrorKind, INTERNAL_ERROR, IncomingLinkItem, LinkIntoItem, LinksIntoOutput, LinksIntoParams,
    LinksIntoSource, LinksOfOutput, LinksOfParams, OutgoingLinkItem, RpcError,
};

use super::WorkerHandler;
use super::errors::worker_error;

fn links_error(error: LinkIndexError) -> RpcError {
    RpcError::standard(INTERNAL_ERROR, error.to_string())
}

impl WorkerHandler {
    pub(super) async fn links_of(&self, params: LinksOfParams) -> Result<LinksOfOutput, RpcError> {
        if !Path::new(&params.project).is_absolute() {
            return Err(RpcError::bars(
                ErrorKind::InvalidParams,
                "ссылки относятся к точному проекту: нужен абсолютный путь",
            ));
        }
        let project = self.worker.resolve(&params.project).map_err(worker_error)?;
        let docs_root = self.worker.docs_root(&project).map_err(worker_error)?;
        let lock = std::sync::Arc::clone(&self.links_lock);
        let path = params.path;
        tokio::task::spawn_blocking(move || {
            // Единственный писатель индекса: параллельные вызовы окна ждут друг друга.
            let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
            let mut index = LinkIndex::open(&project)?;
            index.refresh(&project, Path::new(&docs_root))?;
            index.links_of(&path)
        })
        .await
        .map_err(|error| RpcError::standard(INTERNAL_ERROR, error.to_string()))?
        .map(|found| LinksOfOutput {
            known: found.known,
            outgoing: found
                .outgoing
                .into_iter()
                .map(|link| OutgoingLinkItem {
                    line: link.line,
                    text: link.text,
                    raw: link.raw,
                    kind: link.kind.as_str().to_string(),
                    target: link.target,
                    target_title: link.target_title,
                    broken: link.broken,
                })
                .collect(),
            incoming: found
                .incoming
                .into_iter()
                .map(|link| IncomingLinkItem {
                    path: link.path,
                    title: link.title,
                    lines: link.lines,
                })
                .collect(),
        })
        .map_err(links_error)
    }

    /// Входящие ссылки на набор документов из документов вне набора (подтверждение удаления).
    pub(super) async fn links_into(
        &self,
        params: LinksIntoParams,
    ) -> Result<LinksIntoOutput, RpcError> {
        if !Path::new(&params.project).is_absolute() {
            return Err(RpcError::bars(
                ErrorKind::InvalidParams,
                "ссылки относятся к точному проекту: нужен абсолютный путь",
            ));
        }
        let project = self.worker.resolve(&params.project).map_err(worker_error)?;
        let docs_root = self.worker.docs_root(&project).map_err(worker_error)?;
        let lock = std::sync::Arc::clone(&self.links_lock);
        let paths = params.paths;
        tokio::task::spawn_blocking(move || {
            let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
            let mut index = LinkIndex::open(&project)?;
            index.refresh(&project, Path::new(&docs_root))?;
            index.links_into(&paths)
        })
        .await
        .map_err(|error| RpcError::standard(INTERNAL_ERROR, error.to_string()))?
        .map(|sources| LinksIntoOutput {
            sources: sources
                .into_iter()
                .map(|source| LinksIntoSource {
                    path: source.path,
                    title: source.title,
                    links: source
                        .links
                        .into_iter()
                        .map(|link| LinkIntoItem {
                            line: link.line,
                            text: link.text,
                        })
                        .collect(),
                })
                .collect(),
        })
        .map_err(links_error)
    }
}
