//! Главное окно: создаётся один раз, закрытие его прячет (Т-1).

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

pub const LABEL: &str = "main";
/// Событие окну: открыть экран (`view`: `main`, `settings`, `indexing`) и, если нужно, проект.
pub const NAVIGATE: &str = "bars://navigate";

/// Создаёт окно. С `visible = false` оно загружается скрытым: так `bars --background` держит трей,
/// а показ окна потом не ждёт загрузки интерфейса.
pub fn create(app: &AppHandle, visible: bool) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("bars")
        .inner_size(1280.0, 820.0)
        .min_inner_size(900.0, 600.0)
        .visible(visible)
        .build()?;
    let hidden = window.clone();
    window.on_window_event(move |event| {
        // Закрытие прячет окно; воркер остаётся в трее до пункта «Выход».
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = hidden.hide();
        }
    });
    Ok(())
}

/// Показывает окно и выводит его на передний план.
pub fn show(app: &AppHandle) {
    match app.get_webview_window(LABEL) {
        Some(window) => {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
        None => {
            let _ = create(app, true);
        }
    }
}

/// Открывает окно на нужном экране.
pub fn navigate(app: &AppHandle, view: &str, project: Option<&str>) {
    show(app);
    let _ = app.emit_to(LABEL, NAVIGATE, json!({ "view": view, "project": project }));
}
