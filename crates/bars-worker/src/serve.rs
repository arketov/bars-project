//! Запуск воркера: канал, реестр, очередь (`docs/architecture/worker.md`, «Запуск воркера bars»).

use std::path::PathBuf;
use std::sync::Arc;

use bars_protocol::Endpoint;

use crate::handler::WorkerHandler;
use crate::ipc::{BindError, bind, run_server};
use crate::worker::{ModelProvider, Worker, WorkerConfig, WorkerError};

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
    dirs::home_dir()
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
    let listener = bind(endpoint).map_err(|error| match error {
        BindError::AlreadyRunning => ServeError::AlreadyRunning,
        other => ServeError::Bind(other),
    })?;
    let worker = Worker::start(config, provider)?;
    let handler = Arc::new(WorkerHandler::new(worker.clone()));
    let result = run_server(listener, handler).await;
    worker.shutdown().await;
    result.map_err(ServeError::Accept)
}
