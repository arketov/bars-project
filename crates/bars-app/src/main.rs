//! `bars`: воркер с окном и треем (Т-1). Окно и трей — срез 5; пока процесс держит канал и очередь.

mod handler;
mod ipc;
mod serve;
// Часть API воркера (ход индексации, выгрузка моделей, проверка очереди) нужна окну и тестам.
#[allow(dead_code, unused_imports)]
mod worker;

use std::sync::Arc;

use bars_protocol::Endpoint;
use tauri::RunEvent;

use serve::ServeError;
use worker::{BuiltinProvider, WorkerConfig};

fn main() -> anyhow::Result<()> {
    // Без окна различия нет: `--background` — так запускают клиенты, и пользователю окно не нужно.
    // Когда появится окно (срез 5), без флага оно открывается.
    let _background = std::env::args().any(|arg| arg == "--background");
    let home = serve::bars_home()?;
    let endpoint = Endpoint::current()?;
    let provider = Arc::new(BuiltinProvider::new(home.join("models")));
    let config = WorkerConfig::new(home);

    let app = tauri::Builder::default()
        .setup(move |app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let code = match serve::serve(config, &endpoint, provider).await {
                    // Воркер уже работает: второй процесс уходит (Т-1).
                    Err(ServeError::AlreadyRunning) => 0,
                    Ok(()) => 0,
                    Err(error) => {
                        eprintln!("bars: {error}");
                        1
                    }
                };
                handle.exit(code);
            });
            Ok(())
        })
        .build(tauri::generate_context!())?;
    app.run(|_, event| {
        // Без окон Tauri сам не завершается; выход — только по коду из `handle.exit`.
        if let RunEvent::ExitRequested {
            api, code: None, ..
        } = event
        {
            api.prevent_exit();
        }
    });
    Ok(())
}
