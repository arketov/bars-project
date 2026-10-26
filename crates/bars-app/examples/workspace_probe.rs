//! Проверка продуктового ui/dist в WebView2, изолированный воркер с FakeProvider.
#![allow(dead_code)]
#[path = "../src/appearance.rs"]
mod appearance;
#[path = "../src/appearance_index.rs"]
mod appearance_index;
#[path = "../src/commands.rs"]
mod commands;
#[path = "../src/doc_window.rs"]
mod doc_window;
#[path = "../src/document_io.rs"]
mod document_io;
#[path = "../src/documents.rs"]
mod documents;
#[path = "../src/flush.rs"]
mod flush;
#[path = "../src/folders.rs"]
mod folders;
#[path = "../src/tree_io.rs"]
mod tree_io;
#[path = "../src/window.rs"]
mod window;

use bars_worker::{
    handler::WorkerHandler,
    worker::{Worker, WorkerConfig, models::fake::FakeProvider},
};
use serde_json::Value;
use std::path::PathBuf;
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};

struct Probe {
    output: PathBuf,
    profile: PathBuf,
    javascript: String,
}

#[tauri::command]
async fn keyboard_probe_accelerators(window: tauri::WebviewWindow) -> Result<bool, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    window
        .with_webview(move |webview| {
            use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
            use windows_core::Interface;
            let result = unsafe {
                webview
                    .controller()
                    .CoreWebView2()
                    .and_then(|view| view.Settings())
                    .and_then(|settings| settings.cast::<ICoreWebView2Settings3>())
                    .and_then(|settings| {
                        let mut enabled = windows_core::BOOL::default();
                        settings.AreBrowserAcceleratorKeysEnabled(&mut enabled)?;
                        Ok(enabled.as_bool())
                    })
            }
            .map_err(|error| error.to_string());
            let _ = sender.send(result);
        })
        .map_err(|error| error.to_string())?;
    receiver.await.map_err(|error| error.to_string())?
}

#[tauri::command]
fn workspace_probe_finish(
    app: tauri::AppHandle,
    state: State<'_, Probe>,
    result: Value,
) -> Result<(), String> {
    std::fs::write(&state.output, serde_json::to_vec_pretty(&result).unwrap())
        .map_err(|e| e.to_string())?;
    app.exit(if result["success"] == true { 0 } else { 1 });
    Ok(())
}

#[tauri::command]
fn workspace_probe_size(window: tauri::WebviewWindow, width: f64) -> Result<(), String> {
    window
        .set_size(tauri::LogicalSize::new(width, 850.0))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn workspace_probe_visibility(window: tauri::WebviewWindow, visible: bool) -> Result<(), String> {
    if visible {
        window.show()
    } else {
        window.hide()
    }
    .map_err(|e| e.to_string())
}

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let _entered = runtime.enter();
    let root = PathBuf::from(std::env::args_os().nth(1).expect("папка стенда"));
    let javascript = match std::env::args_os().nth(2) {
        Some(path) => std::fs::read_to_string(path)?,
        None => include_str!("workspace_probe.js").to_string(),
    };
    let temp = tempfile::Builder::new()
        .prefix("fixture-")
        .tempdir_in(&root)?;
    let project = temp.path().join("Проект");
    std::fs::create_dir_all(project.join("docs"))?;
    std::fs::write(
        project.join("docs/Тема.md"),
        "---\ntitle: Проверка панелей\n---\n\n# Проверка панелей\n\nНастоящий документ из изолированного воркера.\n",
    )?;
    let worker = Worker::start(
        WorkerConfig::new(temp.path().join("home")),
        FakeProvider::new("workspace-probe"),
    )?;
    worker.register(&project)?;
    let shared = commands::Shared::default();
    shared.set(WorkerHandler::new(worker.clone()));
    let app = tauri::Builder::default()
        .manage(shared)
        .manage(Probe {
            output: root.join("result.json"),
            profile: temp.path().join("webview"),
            javascript,
        })
        .invoke_handler(tauri::generate_handler![
            commands::bars_call,
            commands::tree_read,
            commands::entry_create_doc,
            commands::entry_create_dir,
            commands::entry_move,
            commands::entry_trash,
            commands::folder_target,
            commands::folder_action,
            commands::document_read,
            commands::document_write,
            commands::document_revision,
            commands::document_editors,
            commands::document_window_target,
            commands::appearance_read,
            commands::keybindings_get,
            commands::keybindings_save,
            keyboard_probe_accelerators,
            commands::worker_logs,
            commands::mcp_history,
            commands::mcp_history_options,
            commands::mcp_history_diagnostics,
            commands::mcp_history_control,
            commands::mcp_history_legacy,
            workspace_probe_finish,
            workspace_probe_visibility,
            workspace_probe_size
        ])
        .setup(|app| {
            let state = app.state::<Probe>();
            let window =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title("bars — проверка продуктовых панелей")
                    .inner_size(1440.0, 850.0)
                    .data_directory(state.profile.clone())
                    .visible(std::env::args_os().nth(2).is_none())
                    .disable_drag_drop_handler()
                    .on_page_load(|window, payload| {
                        if payload.event() == tauri::webview::PageLoadEvent::Finished {
                            let state = window.state::<Probe>();
                            let _ = window.eval(&state.javascript);
                        }
                    })
                    .build()?;
            window::configure_shortcuts(&window)?;
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(50));
                let state = handle.state::<Probe>();
                if !state.output.exists() {
                    let _ = std::fs::write(&state.output, r#"{"success":false,"error":"timeout"}"#);
                }
                handle.exit(2);
            });
            Ok(())
        })
        .build(tauri::generate_context!())?;
    app.run(|_, _| {});
    runtime.block_on(worker.shutdown());
    drop(temp);
    Ok(())
}
