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
#[path = "../src/window_updates.rs"]
mod window_updates;

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
    corpus_hash: String,
}

#[tauri::command]
fn workspace_probe_metrics(
    state: State<'_, commands::Shared>,
    probe: State<'_, Probe>,
) -> Result<Value, String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let script = format!(
            r#"$rootId={};$all=@(Get-CimInstance Win32_Process);$ids=@($rootId);do{{$old=$ids.Count;$ids+=@($all|Where-Object {{$_.ParentProcessId -in $ids -and $_.ProcessId -notin $ids}}|ForEach-Object {{$_.ProcessId}})}}while($ids.Count -gt $old);$rows=@($all|Where-Object {{$_.ProcessId -in $ids -and ($_.ProcessId -eq $rootId -or $_.Name -eq 'msedgewebview2.exe')}}|ForEach-Object {{$p=Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue;if($p){{[pscustomobject]@{{pid=$p.Id;native=($p.Id -eq $rootId);cpu=$p.TotalProcessorTime.TotalSeconds;read=[double]$_.ReadTransferCount;private=$p.PrivateMemorySize64;working=$p.WorkingSet64}}}}}});[pscustomobject]@{{time=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds();processes=$rows;compilers=@($all|Where-Object {{$_.Name -in @('cargo.exe','rustc.exe','link.exe','cl.exe')}}|ForEach-Object {{$_.ProcessId}})}}|ConvertTo-Json -Depth 4 -Compress"#,
            std::process::id()
        );
        let output = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", &script])
            .creation_flags(0x08000000)
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err("Не удалось получить счётчики изолированного процесса".into());
        }
        let mut value: Value = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
        value["coordinator"] = state.updates.statistics();
        value["corpus_hash"] = serde_json::json!(probe.corpus_hash);
        Ok(value)
    }
    #[cfg(not(windows))]
    {
        let _ = (state, probe);
        Err("Счётчики стенда реализованы для Windows".into())
    }
}

#[tauri::command]
async fn workspace_probe_windows(
    app: tauri::AppHandle,
    state: State<'_, Probe>,
    shared: State<'_, commands::Shared>,
    count: usize,
    visible: bool,
) -> Result<(), String> {
    let handler = shared.get().ok_or("Воркер не готов")?;
    let project = handler
        .worker()
        .projects()
        .into_iter()
        .next()
        .ok_or("Нет проекта")?
        .path;
    for i in 1..count.min(4) {
        let label = format!("doc-probe-{i}");
        if app.get_webview_window(&label).is_none() {
            shared
                .documents
                .set(&label, &project.display().to_string(), "docs/Тема.md");
            let document =
                WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("index.html".into()))
                    .title("bars — окно проверки подписки")
                    .data_directory(state.profile.join(&label))
                    .visible(visible)
                    .build()
                    .map_err(|e| e.to_string())?;
            window::configure_shortcuts(&document).map_err(|e| e.to_string())?;
        }
    }
    for (label, window) in app.webview_windows() {
        let requested = label == "main"
            || label
                .strip_prefix("doc-probe-")
                .and_then(|n| n.parse::<usize>().ok())
                .is_some_and(|n| n < count.min(4));
        if visible && requested {
            window.show()
        } else {
            window.hide()
        }
        .map_err(|e| e.to_string())?;
        window::publish_visibility(&window);
    }
    Ok(())
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
fn workspace_probe_progress(state: State<'_, Probe>, result: Value) -> Result<(), String> {
    std::fs::write(
        state.output.with_file_name("progress.json"),
        serde_json::to_vec_pretty(&result).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn workspace_probe_gate(state: State<'_, Probe>) -> bool {
    state
        .output
        .with_file_name("start-measurement.flag")
        .exists()
}

#[tauri::command]
fn workspace_probe_external(
    state: State<'_, Probe>,
    shared: State<'_, commands::Shared>,
    action: String,
) -> Result<(), String> {
    let handler = shared.get().ok_or("Воркер не готов")?;
    let project = handler
        .worker()
        .projects()
        .into_iter()
        .next()
        .ok_or("Нет проекта")?
        .path;
    let owned = state.profile.parent().ok_or("Нет корня стенда")?;
    if !project.starts_with(owned) {
        return Err("Операция вне собственного стенда".into());
    }
    let io = |e: std::io::Error| e.to_string();
    match action.as_str() {
        "create" => {
            std::fs::create_dir_all(project.join("docs/External.v2")).map_err(io)?;
            std::fs::write(
                project.join("docs/External.v2/External.md"),
                "# External\n\nВнешний документ.",
            )
            .map_err(io)
        }
        "rename" => std::fs::rename(
            project.join("docs/External.v2"),
            project.join("docs/Renamed.v3"),
        )
        .map_err(io),
        "remove" => {
            std::fs::remove_file(project.join("docs/Renamed.v3/External.md")).map_err(io)?;
            std::fs::remove_dir(project.join("docs/Renamed.v3")).map_err(io)
        }
        "break_root" => {
            std::fs::rename(project.join("docs"), project.join("docs-unavailable")).map_err(io)
        }
        "restore_root" => {
            std::fs::rename(project.join("docs-unavailable"), project.join("docs")).map_err(io)
        }
        _ => Err("Неизвестная операция стенда".into()),
    }
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
    .map_err(|e| e.to_string())?;
    window::publish_visibility(&window);
    Ok(())
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
    let seed = std::env::var_os("BARS_PROBE_SEED").map(PathBuf::from);
    if let Some(seed) = &seed {
        let seed = seed.canonicalize()?;
        let owner = root
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Нет корня стенда"))?
            .canonicalize()?;
        anyhow::ensure!(seed.starts_with(owner), "Seed вне собственного стенда");
        fn copy(source: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
            std::fs::create_dir_all(target)?;
            for entry in std::fs::read_dir(source)? {
                let entry = entry?;
                let kind = entry.file_type()?;
                if kind.is_symlink() {
                    return Err(std::io::Error::other("Ссылка в seed стенда"));
                }
                if kind.is_dir() {
                    copy(&entry.path(), &target.join(entry.file_name()))?;
                } else if kind.is_file() {
                    std::fs::copy(entry.path(), target.join(entry.file_name()))?;
                }
            }
            Ok(())
        }
        copy(&seed, &project)?;
    }
    std::fs::create_dir_all(project.join("docs"))?;
    if seed.is_none() {
        std::fs::write(
            project.join("docs/Тема.md"),
            "---\ntitle: Проверка панелей\n---\n\n# Проверка панелей\n\nНастоящий документ из изолированного воркера.\n",
        )?;
    }
    let document_count = std::env::var("BARS_PROBE_DOCUMENTS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1)
        .min(10_000);
    for i in if seed.is_none() {
        1..document_count
    } else {
        0..0
    } {
        let directory = project.join(format!("docs/m{:03}", i / 100));
        std::fs::create_dir_all(&directory)?;
        std::fs::write(
            directory.join(format!("d{i:05}.md")),
            format!(
                "---\ntitle: Документ {i}\n---\n# Документ {i}\n\nТекст контрольного корпуса {i}.\n"
            ),
        )?;
    }
    let mut corpus = blake3::Hasher::new();
    corpus.update(&std::fs::read(project.join("docs/Тема.md"))?);
    for i in 1..document_count {
        corpus.update(format!("m{:03}/d{i:05}.md", i / 100).as_bytes());
        corpus.update(&std::fs::read(
            project.join(format!("docs/m{:03}/d{i:05}.md", i / 100)),
        )?);
    }
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
            corpus_hash: corpus.finalize().to_hex().to_string(),
        })
        .invoke_handler(tauri::generate_handler![
            commands::bars_call,
            commands::tree_read,
            commands::window_observe,
            commands::window_snapshot,
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
            workspace_probe_progress,
            workspace_probe_gate,
            workspace_probe_external,
            workspace_probe_metrics,
            workspace_probe_windows,
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
                    .visible(
                        std::env::args_os().nth(2).is_none()
                            || std::env::var_os("BARS_PROBE_VISIBLE").is_some(),
                    )
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
                let timeout = std::env::var("BARS_PROBE_TIMEOUT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(50);
                std::thread::sleep(std::time::Duration::from_secs(timeout));
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
    if std::env::var_os("BARS_PROBE_RETAIN").is_some() {
        let _ = temp.keep();
    } else {
        drop(temp);
    }
    Ok(())
}
