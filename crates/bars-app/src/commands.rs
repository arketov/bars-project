//! Команды Tauri: единственная связь окна с воркером и несколько действий ОС.
//!
//! Окно и воркер — один процесс, поэтому `bars_call` вызывает обработчик методов напрямую, без канала
//! (Т-1, Т-20). Ответы и ошибки те же, что видит агент.

use std::sync::{Mutex, PoisonError};

use bars_protocol::{Call, INTERNAL_ERROR, Id, Request, RpcError};
use bars_worker::handler::WorkerHandler;
use serde_json::Value;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

/// Обработчик воркера: появляется, когда воркер поднят, и до того окну отвечает «запускается».
#[derive(Default)]
pub struct Shared {
    handler: Mutex<Option<WorkerHandler>>,
}

impl Shared {
    pub fn set(&self, handler: WorkerHandler) {
        *self.handler.lock().unwrap_or_else(PoisonError::into_inner) = Some(handler);
    }

    pub fn get(&self) -> Option<WorkerHandler> {
        self.handler
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Вызов метода воркера по имени и параметрам.
#[tauri::command]
pub async fn bars_call(
    state: State<'_, Shared>,
    method: String,
    params: Value,
) -> Result<Value, RpcError> {
    let handler = state
        .get()
        .ok_or_else(|| RpcError::standard(INTERNAL_ERROR, "воркер ещё запускается"))?;
    let request = Request::new(Id::Number(0), &method, params);
    let call = Call::from_request(&request)?;
    handler.dispatch(call).await
}

/// Диалог выбора папки; `None` — пользователь отказался.
#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .blocking_pick_folder()
            .and_then(|path| path.into_path().ok())
            .map(|path| path.display().to_string())
    })
    .await
    .map_err(|error| error.to_string())
}

/// Открывает файл или папку программой, связанной с ними в ОС.
#[tauri::command]
pub fn open_path(path: String) -> Result<(), String> {
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(|error| error.to_string())
}

/// Домашняя папка bars (`~/.bars`).
#[tauri::command]
pub fn bars_dir() -> Result<String, String> {
    bars_worker::serve::bars_home()
        .map(|path| path.display().to_string())
        .map_err(|error| error.to_string())
}

/// Перезапускает весь процесс `bars`: окно и воркер — один процесс (OQ-57).
#[tauri::command]
pub async fn restart_app(app: AppHandle, state: State<'_, Shared>) -> Result<(), String> {
    if let Some(handler) = state.get() {
        handler.worker().shutdown().await;
    }
    app.restart()
}
