//! Главное окно: создаётся один раз, закрытие его прячет (Т-1).

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

pub const LABEL: &str = "main";
/// Событие окну: открыть экран (`view`: `main`, `settings`, `indexing`) и, если нужно, проект.
pub const NAVIGATE: &str = "bars://navigate";
pub const VISIBILITY: &str = "bars://window-visibility";

pub fn publish_visibility(window: &tauri::WebviewWindow) {
    let visible = window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(false);
    let _ = window.emit_to(window.label(), VISIBILITY, visible);
}

/// Создаёт окно. С `visible = false` оно загружается скрытым: так `bars --background` держит трей,
/// а показ окна потом не ждёт загрузки интерфейса.
pub fn create(app: &AppHandle, visible: bool) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("bars")
        .inner_size(1280.0, 820.0)
        .min_inner_size(900.0, 600.0)
        .visible(visible)
        // Обработчик файлов Tauri на Windows перехватывает HTML5 drag-and-drop WebView2 (ISS-21).
        .disable_drag_drop_handler()
        .build()?;
    configure_shortcuts(&window)?;
    let hidden = window.clone();
    window.on_window_event(move |event| {
        // Закрытие прячет окно; воркер остаётся в трее до пункта «Выход».
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = hidden.hide();
            publish_visibility(&hidden);
            if let Some(state) = hidden.try_state::<crate::commands::Shared>()
                && let Some(handler) = state.get()
            {
                let _ = handler.worker().observe_window(hidden.label(), None);
                state.updates.release(hidden.label());
            }
        }
    });
    Ok(())
}

/// Единая host-политика продукта и изолированного стенда.
pub fn configure_shortcuts(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let focused = window.clone();
    window.on_window_event(move |event| {
        // WebView2 не всегда сообщает visibilitychange после показа или восстановления.
        if matches!(event, WindowEvent::Focused(_) | WindowEvent::Resized(_)) {
            publish_visibility(&focused);
        }
    });
    #[cfg(all(windows, not(debug_assertions)))]
    window.with_webview(|webview| {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
        use windows_core::Interface;
        let result = unsafe {
            webview
                .controller()
                .CoreWebView2()
                .and_then(|view| view.Settings())
                .and_then(|settings| settings.cast::<ICoreWebView2Settings3>())
                .and_then(|settings| settings.SetAreBrowserAcceleratorKeysEnabled(false))
        };
        if let Err(error) = result {
            log::error!("Не отключены сочетания браузера WebView2: {error}");
        }
    })?;
    #[cfg(not(all(windows, not(debug_assertions))))]
    let _ = window;
    Ok(())
}

/// Показывает окно и выводит его на передний план.
pub fn show(app: &AppHandle) {
    match app.get_webview_window(LABEL) {
        Some(window) => {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
            publish_visibility(&window);
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
