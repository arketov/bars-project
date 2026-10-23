//! `bars`: воркер с окном и треем (Т-1). Окно и воркер — один процесс: окно вызывает обработчик
//! методов напрямую, а `bars-mcp` и `bars-cli` приходят по каналу.

mod appearance;
#[cfg(test)]
mod appearance_tests;
mod commands;
mod doc_window;
mod document_io;
#[cfg(test)]
mod document_io_tests;
mod documents;
mod flush;
#[cfg(test)]
mod folder_tests;
mod folders;
mod icons;
mod tray;
mod tree_io;
#[cfg(test)]
mod tree_io_tests;
mod window;

use std::sync::Arc;

use bars_protocol::Endpoint;
use bars_worker::serve::{self, ServeError};
use bars_worker::worker::{BuiltinProvider, WorkerConfig};
use commands::Shared;
use tauri::{Manager, RunEvent};

fn main() -> anyhow::Result<()> {
    // `--background` — так запускают клиенты: окно скрыто, пока пользователь не откроет его из трея.
    let background = std::env::args().any(|arg| arg == "--background");
    // `--quit` — так установщик просит работающий `bars` выйти перед заменой файлов.
    let quit = std::env::args().any(|arg| arg == "--quit");
    let home = serve::bars_home()?;
    let endpoint = Endpoint::current()?;
    let provider = Arc::new(BuiltinProvider::new(home.join("models")));
    let mut config = WorkerConfig::new(home.clone());
    config.install_env = bars_worker::clients::InstallEnv::detect();
    if let Ok(exe) = std::env::current_exe() {
        config.autostart = bars_worker::autostart::for_exe(&exe);
    }

    let app = tauri::Builder::default()
        // Первым: второй запуск `bars` передаётся работающему и показывает его окно (Т-1).
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if args.iter().any(|arg| arg == "--quit") {
                tray::quit_app(app);
            } else if !args.iter().any(|arg| arg == "--background") {
                window::navigate(app, "main", None);
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .manage(Shared::default())
        .invoke_handler(tauri::generate_handler![
            commands::bars_call,
            commands::keybindings_get,
            commands::keybindings_save,
            commands::pick_folder,
            commands::open_path,
            commands::document_action,
            commands::document_editors,
            commands::document_read,
            commands::document_write,
            commands::document_revision,
            commands::tree_read,
            commands::entry_create_doc,
            commands::entry_create_dir,
            commands::entry_move,
            commands::entry_trash,
            commands::appearance_read,
            commands::appearance_set,
            commands::folder_target,
            commands::folder_action,
            commands::bars_dir,
            commands::worker_logs,
            commands::mcp_statistics,
            commands::mcp_history,
            commands::mcp_history_control,
            commands::mcp_history_options,
            commands::mcp_history_legacy,
            commands::mcp_history_diagnostics,
            commands::restart_app,
            commands::flush_reply,
            commands::document_window_open,
            commands::document_window_target,
            commands::document_window_close,
            commands::open_in_main_window,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            // Запущенного `bars` не было: просить выйти некого.
            if quit {
                handle.exit(0);
                return Ok(());
            }
            if bars_worker::logging::init(&home).is_err() {
                eprintln!("bars: файловый журнал недоступен");
            }
            log::info!(
                "Приложение bars запущено, версия {}",
                env!("CARGO_PKG_VERSION")
            );
            window::create(&handle, !background)?;
            tray::build(&handle)?;
            tray::spawn_status_loop(handle.clone());
            let worker_handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                let ready = worker_handle.clone();
                let result = serve::serve_with(config, &endpoint, provider, move |handler| {
                    ready.state::<Shared>().set(handler);
                })
                .await;
                let code = match result {
                    // Воркер уже работает: второй процесс уходит (Т-1).
                    Err(ServeError::AlreadyRunning) | Ok(()) => 0,
                    Err(_) => {
                        log::error!("Не удалось запустить или обслуживать воркер");
                        eprintln!("bars: воркер завершился с ошибкой");
                        1
                    }
                };
                worker_handle.exit(code);
            });
            Ok(())
        })
        .build(tauri::generate_context!())?;
    app.run(|_, event| {
        // Закрытое окно не завершает процесс: он живёт в трее. Выход — только по коду из `exit`.
        if let RunEvent::ExitRequested {
            api, code: None, ..
        } = event
        {
            api.prevent_exit();
        }
    });
    Ok(())
}
