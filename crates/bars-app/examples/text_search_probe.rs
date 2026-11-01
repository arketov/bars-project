//! Приёмочный стенд дерева: настоящий Tauri/WebView2, изолированные проект и профиль, FakeProvider.
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
#[path = "../src/window_updates.rs"]
mod window_updates;

use bars_worker::{
    handler::WorkerHandler,
    worker::{Worker, WorkerConfig, models::fake::FakeProvider},
};
use std::path::PathBuf;
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};

struct Probe {
    output: PathBuf,
    project: PathBuf,
}
#[tauri::command]
fn text_search_probe_external(state: State<'_, Probe>) -> Result<(), String> {
    std::fs::create_dir(state.project.join("docs/Внешняя")).map_err(|e| e.to_string())
}
#[tauri::command]
fn text_search_probe_finish(
    app: tauri::AppHandle,
    state: State<'_, Probe>,
    result: serde_json::Value,
) {
    std::fs::write(&state.output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    app.exit(if result["success"] == true { 0 } else { 1 });
}
#[tauri::command]
fn text_search_probe_size(window: tauri::WebviewWindow, width: f64) -> Result<(), String> {
    window
        .set_size(tauri::LogicalSize::new(width, 850.0))
        .map_err(|e| e.to_string())
}
fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let _entered = runtime.enter();
    let root = PathBuf::from(std::env::args_os().nth(1).expect("каталог результатов"));
    let fixture = tempfile::Builder::new()
        .prefix("fixture-")
        .tempdir_in(&root)?;
    let project = fixture.path().join("Проект");
    std::fs::create_dir_all(project.join("docs"))?;
    std::fs::write(
        project.join("docs/Тема.md"),
        "---\ntitle: Проверка текста\n---\n\n# Проверка текста\n😀 ПрИвЕт привет\n",
    )?;
    let worker = Worker::start(
        WorkerConfig::new(fixture.path().join("home")),
        FakeProvider::new("text-search-probe"),
    )?;
    worker.register(&project)?;
    let shared = commands::Shared::default();
    shared.set(WorkerHandler::new(worker.clone()));
    let app = tauri::Builder::default()
        .manage(shared)
        .manage(Probe {
            output: root.join("result.json"),
            project,
        })
        .invoke_handler(tauri::generate_handler![
            commands::bars_call,
            commands::tree_read,
            commands::entry_create_doc,
            commands::entry_create_dir,
            commands::entry_plan_move,
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
            text_search_probe_external,
            text_search_probe_finish,
            text_search_probe_size
        ])
        .setup(move |app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("bars — проверка текстового поиска")
                .inner_size(1024.0, 850.0)
                .data_directory(fixture.path().join("webview"))
                .disable_drag_drop_handler()
                .visible(true)
                .on_page_load(|window, payload| {
                    if payload.event() == tauri::webview::PageLoadEvent::Finished {
                        let _ = window.eval(include_str!("text_search_probe.js"));
                    }
                })
                .build()?;
            // Держим временный проект до завершения приложения.
            app.manage(fixture);
            Ok(())
        })
        .build(tauri::generate_context!())?;
    app.run(|_, _| {});
    runtime.block_on(worker.shutdown());
    Ok(())
}
