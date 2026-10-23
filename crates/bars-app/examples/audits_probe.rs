//! 3.8.3: продуктовое ui/dist в WebView2 с настоящим IPC и изолированным FakeProvider.
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
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};

struct Probe {
    root: PathBuf,
    project: PathBuf,
    profile: PathBuf,
    endpoint: bars_protocol::Endpoint,
    real: bool,
}
#[tauri::command]
fn audits_probe_real(state: State<'_, Probe>) -> bool {
    state.real
}
#[tauri::command]
async fn audits_probe_mcp_denied(state: State<'_, Probe>) -> Result<bool, String> {
    let mut client =
        bars_protocol::Client::connect(&state.endpoint, bars_protocol::ClientKind::Mcp)
            .await
            .map_err(|e| e.to_string())?;
    match client
        .call(&bars_protocol::NewTopicsListParams {
            project: state.project.display().to_string(),
        })
        .await
    {
        Err(bars_protocol::ClientError::Rpc(error))
            if error.code == bars_protocol::METHOD_NOT_FOUND =>
        {
            Ok(true)
        }
        Err(error) => Err(error.to_string()),
        Ok(_) => Err("список тем оказался доступен MCP".into()),
    }
}
#[tauri::command]
async fn audits_probe_mcp_search(state: State<'_, Probe>, query: String) -> Result<Value, String> {
    let mut client =
        bars_protocol::Client::connect(&state.endpoint, bars_protocol::ClientKind::Mcp)
            .await
            .map_err(|e| e.to_string())?;
    let params: bars_protocol::SearchParams = serde_json::from_value(
        json!({"cwd":state.project,"query":query,"min_score":1.0,"rerank":false}),
    )
    .map_err(|e| e.to_string())?;
    let out = client.call(&params).await.map_err(|e| e.to_string())?;
    serde_json::to_value(out).map_err(|e| e.to_string())
}
#[tauri::command]
fn audits_probe_add_knowledge(state: State<'_, Probe>) -> Result<(), String> {
    std::fs::write(state.project.join("docs/knowledge.md"),"# Журналы возврата в боксы\n\nАудит журналов возврата в боксы и предыдущей попытки входа результаты сессий.\nЖурналы возврата в боксы содержат результаты предыдущей попытки входа. Аудит анализирует журналы по всем сессиям.\n").map_err(|e|e.to_string())
}
#[tauri::command]
fn audits_probe_project(state: State<'_, Probe>) -> String {
    state.project.display().to_string()
}
#[tauri::command]
fn audits_probe_add_link(state: State<'_, Probe>) -> Result<(), String> {
    // Только fixture текущего процесса, путь из JS не принимается.
    std::fs::write(
        state.project.join("docs/new.md"),
        "# Новое\n\n[[Ещё отсутствует]]\n",
    )
    .map_err(|e| e.to_string())
}
#[tauri::command]
fn audits_probe_size(window: tauri::WebviewWindow, width: f64) -> Result<(), String> {
    window
        .set_size(tauri::LogicalSize::new(width, 850.0))
        .map_err(|e| e.to_string())
}
#[tauri::command]
fn audits_probe_finish(
    app: tauri::AppHandle,
    state: State<'_, Probe>,
    result: Value,
) -> Result<(), String> {
    std::fs::write(
        state.root.join("result.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    app.exit(if result["success"] == true { 0 } else { 1 });
    Ok(())
}
fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let _entered = runtime.enter();
    let root = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .expect("собственная папка результатов"),
    );
    let temp = tempfile::Builder::new()
        .prefix("audits-")
        .tempdir_in(&root)?;
    let project = temp.path().join("project");
    std::fs::create_dir_all(project.join("docs/manual"))?;
    for (name, text) in [
        (
            "a.md",
            "# Настройки\n\nВремя ожидания ответа сервера в текущих настройках приложения составляет 30 с. Это значение задано по умолчанию для всех проектов.\n\n[[Нет файла]]\n",
        ),
        (
            "manual/b.md",
            "# Настройки\n\nВремя ожидания ответа сервера в текущих настройках приложения составляет 60 секунд. Это значение задано по умолчанию для всех проектов.\n",
        ),
    ] {
        std::fs::write(project.join("docs").join(name), text)?;
    }
    let models = std::env::args_os().nth(2).map(PathBuf::from);
    let real = models.is_some();
    let home = temp.path().join("home");
    let provider: Arc<dyn bars_worker::worker::ModelProvider> = if let Some(models) = models {
        use bars_core::models::{Accelerator, BGE_M3_ID, is_downloaded, reranker_is_downloaded};
        anyhow::ensure!(
            is_downloaded(&models, BGE_M3_ID) && reranker_is_downloaded(&models, Accelerator::Cpu),
            "нужен полный локальный кэш embed/rerank; стенд не скачивает модели"
        );
        std::fs::create_dir_all(&home)?;
        std::fs::write(
            home.join("settings.toml"),
            "[embed]\nmodel=\"BAAI/bge-m3\"\naccelerator=\"auto\"\n[rerank]\nenabled=false\naccelerator=\"cpu\"\n",
        )?;
        Arc::new(bars_worker::worker::ConfiguredProvider::new(models))
    } else {
        FakeProvider::new("audits-probe")
    };
    let worker = Worker::start(WorkerConfig::new(home), provider)?;
    worker.register(&project)?;
    let handler = WorkerHandler::new(worker.clone());
    let endpoint = bars_protocol::Endpoint::named(&format!("audits-probe-{}", std::process::id()));
    let listener = bars_worker::ipc::bind(&endpoint)?;
    let server = runtime.spawn(bars_worker::ipc::run_server(
        listener,
        Arc::new(handler.clone()),
    ));
    let shared = commands::Shared::default();
    shared.set(handler);
    let app = tauri::Builder::default()
        .manage(shared)
        .manage(Probe {
            root: root.clone(),
            project,
            profile: temp.path().join("webview"),
            endpoint,
            real,
        })
        .invoke_handler(tauri::generate_handler![
            commands::bars_call,
            commands::keybindings_get,
            commands::keybindings_save,
            commands::appearance_read,
            commands::document_read,
            commands::document_write,
            commands::document_revision,
            commands::flush_reply,
            audits_probe_project,
            audits_probe_real,
            audits_probe_mcp_denied,
            audits_probe_mcp_search,
            audits_probe_add_knowledge,
            audits_probe_add_link,
            audits_probe_size,
            audits_probe_finish
        ])
        .setup(|app| {
            let state = app.state::<Probe>();
            let view = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("bars — аудиты 3.8.3")
                .inner_size(1440.0, 850.0)
                .data_directory(state.profile.clone())
                .disable_drag_drop_handler()
                .visible(true)
                .on_page_load(|window, payload| {
                    if payload.event() == tauri::webview::PageLoadEvent::Finished {
                        let _ = window.eval(include_str!("audits_probe.js"));
                    }
                })
                .build()?;
            window::configure_shortcuts(&view)?;
            let handle = app.handle().clone();
            let timeout = if state.real { 150 } else { 55 };
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(timeout));
                let state = handle.state::<Probe>();
                if !state.root.join("result.json").exists() {
                    let _ = std::fs::write(
                        state.root.join("result.json"),
                        json!({"success":false,"error":"timeout"}).to_string(),
                    );
                }
                handle.exit(2);
            });
            Ok(())
        })
        .build(tauri::generate_context!())?;
    app.run(|_, _| {});
    server.abort();
    runtime.block_on(worker.shutdown());
    drop(temp);
    Ok(())
}
