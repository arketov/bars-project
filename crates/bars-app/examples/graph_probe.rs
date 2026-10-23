//! Проверка физического графа в продуктовом ui/dist в WebView2: изолированный воркер с FakeProvider и копиями
//! документации bars-project и overdrive-bots (читаются только `.md`, исходные папки и их индексы не трогаются).
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
use std::path::{Path, PathBuf};
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};

struct Probe {
    output: PathBuf,
    profile: PathBuf,
    projects: Vec<String>,
}

#[tauri::command]
fn graph_probe_projects(state: State<'_, Probe>) -> Vec<String> {
    state.projects.clone()
}

#[tauri::command]
fn graph_probe_finish(
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
fn graph_probe_size(window: tauri::WebviewWindow, width: f64, height: f64) -> Result<(), String> {
    window
        .set_size(tauri::LogicalSize::new(width, height))
        .map_err(|e| e.to_string())
}

/// Копирует только Markdown: структура папок и содержимое те же, служебные папки `.bars` не переносятся.
fn copy_markdown(from: &Path, to: &Path) -> std::io::Result<usize> {
    let mut copied = 0;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            copied += copy_markdown(&path, &to.join(&name))?;
        } else if path.extension().is_some_and(|e| e == "md") {
            std::fs::create_dir_all(to)?;
            std::fs::copy(&path, to.join(&name))?;
            copied += 1;
        }
    }
    Ok(copied)
}

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let _entered = runtime.enter();
    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(args.next().expect("папка стенда"));
    let sources: Vec<(String, PathBuf)> = vec![
        (
            "bars-project".into(),
            PathBuf::from(args.next().expect("docs bars-project")),
        ),
        (
            "overdrive-bots".into(),
            PathBuf::from(args.next().expect("docs overdrive-bots")),
        ),
    ];
    let temp = tempfile::Builder::new()
        .prefix("fixture-")
        .tempdir_in(&root)?;
    let worker = Worker::start(
        WorkerConfig::new(temp.path().join("home")),
        FakeProvider::new("graph-probe"),
    )?;
    let mut projects = Vec::new();
    for (name, docs) in &sources {
        let project = temp.path().join(name);
        let copied = copy_markdown(docs, &project.join("docs"))?;
        eprintln!("{name}: скопировано {copied} документов");
        worker.register(&project)?;
        projects.push(project.display().to_string());
    }
    let shared = commands::Shared::default();
    shared.set(WorkerHandler::new(worker.clone()));
    let app = tauri::Builder::default()
        .manage(shared)
        .manage(Probe {
            output: root.join("result.json"),
            profile: temp.path().join("webview"),
            projects,
        })
        .invoke_handler(tauri::generate_handler![
            commands::bars_call,
            commands::keybindings_get,
            commands::keybindings_save,
            commands::worker_logs,
            commands::mcp_history,
            commands::mcp_history_options,
            commands::mcp_history_diagnostics,
            commands::mcp_history_control,
            commands::mcp_history_legacy,
            graph_probe_projects,
            graph_probe_finish,
            graph_probe_size
        ])
        .setup(|app| {
            let state = app.state::<Probe>();
            let window =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title("bars — проверка физического графа")
                    .inner_size(1440.0, 850.0)
                    .data_directory(state.profile.clone())
                    .visible(true)
                    .disable_drag_drop_handler()
                    .on_page_load(|window, payload| {
                        if payload.event() == tauri::webview::PageLoadEvent::Finished {
                            let _ = window.eval(include_str!("graph_probe.js"));
                        }
                    })
                    .build()?;
            window::configure_shortcuts(&window)?;
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(170));
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
