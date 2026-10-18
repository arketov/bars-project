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

/// Редакторы из фиксированного каталога; путь исполняемого файла окно не задаёт.
#[tauri::command]
pub fn document_editors() -> Value {
    Value::Array(
        crate::documents::editors()
            .into_iter()
            .map(|editor| serde_json::json!({ "id": editor.id, "label": editor.label }))
            .collect(),
    )
}

/// Действия только с документом текущего индекса зарегистрированного проекта.
#[tauri::command]
pub async fn document_action(
    state: State<'_, Shared>,
    project: String,
    path: String,
    action: String,
    editor: Option<String>,
) -> Result<String, String> {
    let handler = state.get().ok_or("воркер ещё запускается")?;
    let root = handler
        .worker()
        .resolve(&project)
        .map_err(|e| e.to_string())?;
    let tree = handler
        .dispatch(Call::Tree(bars_protocol::TreeParams {
            target: bars_protocol::Target {
                cwd: project.clone(),
                project: Some(project),
            },
        }))
        .await
        .map_err(|e| e.message)?;
    let tree: bars_protocol::TreeOutput =
        serde_json::from_value(tree).map_err(|e| e.to_string())?;
    if !tree.documents.iter().any(|document| document.path == path) {
        return Err("документ отсутствует в индексе проекта; обновите дерево или выдачу".into());
    }
    let absolute = crate::documents::indexed_path(&root, &path)?;
    if action == "check" {
        handler
            .worker()
            .check_files(&root, vec![absolute.clone()])
            .map_err(|e| e.to_string())?;
    } else {
        let file = absolute.clone();
        tauri::async_runtime::spawn_blocking(move || match action.as_str() {
            "path" => Ok(()),
            "open" => crate::documents::open(&file, editor.as_deref().unwrap_or("default")),
            "reveal" => tauri_plugin_opener::reveal_item_in_dir(&file).map_err(|e| e.to_string()),
            _ => Err("неизвестное действие документа".into()),
        })
        .await
        .map_err(|e| e.to_string())??;
    }
    Ok(absolute.display().to_string())
}

/// Домашняя папка bars (`~/.bars`).
#[tauri::command]
pub fn bars_dir() -> Result<String, String> {
    bars_worker::serve::bars_home()
        .map(|path| path.display().to_string())
        .map_err(|error| error.to_string())
}

/// Статистика только воркера текущего окна; произвольный путь не принимается.
#[tauri::command]
pub async fn mcp_statistics(state: State<'_, Shared>) -> Result<Value, String> {
    let handler = state.get().ok_or("воркер ещё запускается")?;
    let home = handler.worker().bars_home().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        let rows = bars_worker::mcp_statistics::snapshot(&home)
            .map_err(|_| "Не удалось прочитать статистику MCP. Проверьте доступ к хранилищу.".to_string())?;
        serde_json::to_value(rows).map_err(|_| "Не удалось подготовить статистику MCP".to_string())
    }).await.map_err(|_| "Чтение статистики MCP прервано".to_string())?
}

/// Только журнал текущего процесса: произвольные пути из окна не принимаются.
#[tauri::command]
pub async fn worker_logs(level: bars_worker::logging::Level) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let snapshot = bars_worker::logging::snapshot(level)
            .map_err(|_| "Не удалось прочитать журнал воркера. Проверьте доступ к папке логов.".to_string())?;
        serde_json::to_value(snapshot).map_err(|_| "Не удалось подготовить журнал".to_string())
    }).await.map_err(|_| "Чтение журнала прервано".to_string())?
}

#[cfg(test)]
mod log_tests {
    #[tokio::test]
    async fn logs_command_returns_filtered_serializable_snapshot() {
        let home = tempfile::tempdir().unwrap();
        bars_worker::logging::init(home.path()).unwrap();
        log::info!("Информация для окна");
        log::warn!("Предупреждение для окна");
        let snapshot = super::worker_logs(bars_worker::logging::Level::Warn).await.unwrap();
        assert_eq!(snapshot["entries"].as_array().unwrap().len(), 1);
        assert_eq!(snapshot["entries"][0]["message"], "Предупреждение для окна");
        assert!(snapshot["directory"].as_str().unwrap().ends_with("logs"));
        assert!(serde_json::from_str::<bars_worker::logging::Level>("\"unknown\"").is_err());
    }
}

/// Перезапускает весь процесс `bars`: окно и воркер — один процесс (OQ-57).
#[tauri::command]
pub async fn restart_app(app: AppHandle, state: State<'_, Shared>) -> Result<(), String> {
    if let Some(handler) = state.get() {
        handler.worker().shutdown().await;
    }
    app.restart()
}
