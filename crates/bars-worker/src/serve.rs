//! Запуск воркера: канал, реестр, очередь (`docs/architecture/worker.md`, «Запуск воркера bars»).

use std::path::PathBuf;
use std::sync::Arc;

use bars_protocol::Endpoint;

use crate::handler::WorkerHandler;
use crate::ipc::{BindError, bind, run_server};
use crate::worker::{ModelProvider, Worker, WorkerConfig, WorkerError};

#[cfg(test)]
mod tests;

#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    /// Имя канала занято: воркер уже работает, новый не нужен (Т-1).
    #[error("воркер bars уже запущен")]
    AlreadyRunning,
    #[error(transparent)]
    Bind(BindError),
    #[error(transparent)]
    Worker(#[from] WorkerError),
    #[error("канал перестал принимать соединения: {0}")]
    Accept(std::io::Error),
}

/// `~/.bars`: настройки, реестр проектов, модели, логи.
pub fn bars_home() -> std::io::Result<PathBuf> {
    bars_protocol::user_home()
        .map(|home| home.join(".bars"))
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "не найдена домашняя папка")
        })
}

/// Открывает канал, поднимает воркер и обслуживает соединения, пока канал жив.
///
/// Канал открывается первым: он же защита от второго воркера, и до чтения реестра второй `bars`
/// уже знает, что опоздал. Вызовы принимаются сразу, проверка проектов идёт в очереди.
pub async fn serve(
    config: WorkerConfig,
    endpoint: &Endpoint,
    provider: Arc<dyn ModelProvider>,
) -> Result<(), ServeError> {
    serve_with(config, endpoint, provider, |_| ()).await
}

/// То же, что [`serve`], но отдаёт обработчик, как только воркер поднят: окно вызывает его
/// напрямую, без канала (Т-1, Т-20).
pub async fn serve_with(
    config: WorkerConfig,
    endpoint: &Endpoint,
    provider: Arc<dyn ModelProvider>,
    ready: impl FnOnce(WorkerHandler),
) -> Result<(), ServeError> {
    let listener = bind(endpoint).map_err(|error| match error {
        BindError::AlreadyRunning => ServeError::AlreadyRunning,
        other => ServeError::Bind(other),
    })?;
    // Приложение обновилось или переехало: запись автозапуска должна указывать на текущий `bars`.
    match crate::autostart::repair(config.autostart.as_ref()) {
        Ok(true) => log::info!("Запись автозапуска обновлена на текущий bars"),
        Ok(false) => {}
        Err(_) => log::warn!("Запись автозапуска не проверена"),
    }
    let worker = Worker::start(config, provider)?;
    log::info!("Воркер готов принимать вызовы");
    let handler = Arc::new(WorkerHandler::new(worker.clone()));
    ready(WorkerHandler::clone(&handler));
    worker.refresh_clients();
    let result = run_server(listener, handler).await;
    worker.shutdown().await;
    result.map_err(ServeError::Accept)
}
