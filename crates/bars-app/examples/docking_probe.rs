//! Изолированная проба WebView2: без serve, single-instance, трея и профиля пользователя.
#![allow(dead_code)]
#[path = "../src/commands.rs"]
mod commands;
#[path = "../src/document_io.rs"]
mod document_io;
#[path = "../src/documents.rs"]
mod documents;
#[path = "../src/flush.rs"]
mod flush;
#[path = "../src/folders.rs"]
mod folders;

use bars_worker::{
    handler::WorkerHandler,
    worker::{Worker, WorkerConfig, models::fake::FakeProvider},
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Mutex, time::Duration};
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};

struct Probe {
    project: PathBuf,
    output: PathBuf,
    profile: PathBuf,
    child: Mutex<Option<Value>>,
}

#[tauri::command]
fn probe_trace(state: State<'_, Probe>, stage: String) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state.output.with_extension("trace.txt"))
    {
        let _ = writeln!(file, "{stage}");
    }
}

#[tauri::command]
fn probe_info(state: State<'_, Probe>) -> Value {
    json!({"project": state.project, "pid": std::process::id()})
}

#[tauri::command]
async fn probe_open_document(app: tauri::AppHandle, state: State<'_, Probe>) -> Result<(), String> {
    WebviewWindowBuilder::new(
        &app,
        "probe-doc",
        WebviewUrl::App("docking.html?document=1".into()),
    )
    .title("bars — документ стенда")
    .inner_size(700.0, 500.0)
    .data_directory(state.profile.clone())
    .visible(true)
    .disable_drag_drop_handler()
    .build()
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn probe_child_result(state: State<'_, Probe>) -> Option<Value> {
    state.child.lock().unwrap().clone()
}

#[tauri::command]
fn probe_finish(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, Probe>,
    result: Value,
) -> Result<(), String> {
    if window.label() == "probe-doc" {
        window.close().map_err(|e| e.to_string())?;
        *state.child.lock().unwrap() = Some(result);
    } else {
        let closed = app.get_webview_window("probe-doc").is_none();
        let success = result.get("error").is_none()
            && result.pointer("/childResult/error").is_none()
            && closed;
        std::fs::write(
            &state.output,
            serde_json::to_vec_pretty(
                &json!({"success": success, "childClosed": closed, "result": result}),
            )
            .unwrap(),
        )
        .map_err(|e| e.to_string())?;
        app.exit(if success { 0 } else { 1 });
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let _entered = runtime.enter();
    let output = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .expect("путь итогового JSON обязателен"),
    );
    let fixture_root = PathBuf::from(
        std::env::args_os()
            .nth(2)
            .expect("папка стенда обязательна"),
    );
    let temp = tempfile::Builder::new()
        .prefix("fixture-")
        .tempdir_in(fixture_root)?;
    let project = temp.path().join("Проект");
    std::fs::create_dir_all(project.join("docs"))?;
    std::fs::write(
        project.join("docs/Тема.md"),
        "# Тема\n\nОбщий воркер двух окон.\n",
    )?;
    let worker = Worker::start(
        WorkerConfig::new(temp.path().join("home")),
        FakeProvider::new("docking-probe"),
    )?;
    worker.register(&project)?;
    let shared = commands::Shared::default();
    shared.set(WorkerHandler::new(worker.clone()));
    let app = tauri::Builder::default()
        .manage(shared)
        .manage(Probe {
            project,
            output,
            profile: temp.path().join("webview"),
            child: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            commands::bars_call,
            probe_info,
            probe_trace,
            probe_open_document,
            probe_child_result,
            probe_finish
        ])
        .setup(|app| {
            let state = app.state::<Probe>();
            WebviewWindowBuilder::new(app, "probe", WebviewUrl::App("docking.html".into()))
                .title("bars — проба docking")
                .inner_size(1280.0, 820.0)
                .data_directory(state.profile.clone())
                .visible(true)
                .disable_drag_drop_handler()
                .on_page_load(|window, payload| {
                    let state = window.state::<Probe>();
                    let _ = std::fs::write(state.output.with_extension("page.txt"), format!("{:?} {}", payload.event(), payload.url()));
                    let _ = window.eval("window.__TAURI_INTERNALS__.invoke('probe_trace', {stage: 'page: ' + document.body.innerText.slice(0,600)});");
                })
                .build()?;
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(45));
                let state = handle.state::<Probe>();
                if !state.output.exists() {
                    let _ = std::fs::write(&state.output, "{\"success\":false,\"error\":\"timeout\"}");
                }
                handle.exit(2);
            });
            Ok(())
        })
        .build(tauri::generate_context!("docking-probe/tauri.conf.json"))?;
    app.run(|_, _| {});
    runtime.block_on(worker.shutdown());
    drop(temp);
    Ok(())
}
