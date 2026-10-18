//! Изолированная сессия настоящего воркера для приёмки интеграций без окна.
//! Запускать только с BARS_USER_HOME; остановка — файл stop в этой папке.
use bars_protocol::Endpoint;
use bars_worker::serve::{bars_home, serve_with};
use bars_worker::worker::{BuiltinProvider, ModelProvider, WorkerConfig};
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(not(feature = "test-support"))]
    if std::env::var("BARS_TEST_FAKE_MODELS").as_deref() == Ok("1") {
        return Err("BARS_TEST_FAKE_MODELS требует сборку --features test-support".into());
    }
    let home = std::env::var_os("BARS_USER_HOME")
        .filter(|s| !s.is_empty())
        .ok_or("client_refresh_session требует BARS_USER_HOME")?;
    let home = std::path::PathBuf::from(home);
    let stop = home.join("stop");
    if stop.exists() {
        return Err("удалите stop перед новым запуском".into());
    }
    let mut config = WorkerConfig::new(bars_home()?);
    // Exe примера лежит в examples/, CLI и MCP — на уровень выше.
    let bin = std::env::current_exe()?
        .parent()
        .ok_or("нет папки exe")?
        .parent()
        .ok_or("нет папки bin")?
        .to_path_buf();
    if let Some(env) = &mut config.install_env {
        env.mcp_exe = bin.join(format!("bars-mcp{}", std::env::consts::EXE_SUFFIX));
        env.cli_exe = bin.join(format!("bars-cli{}", std::env::consts::EXE_SUFFIX));
    }
    let endpoint = Endpoint::current()?;
    let provider: Arc<dyn ModelProvider> =
        Arc::new(BuiltinProvider::new(home.join(".bars/models")));
    // Для проверки доставки документации инференс заменяется только в test-support сборке.
    #[cfg(feature = "test-support")]
    let provider: Arc<dyn ModelProvider> =
        if std::env::var("BARS_TEST_FAKE_MODELS").as_deref() == Ok("1") {
            bars_worker::worker::models::fake::FakeProvider::new("docs-fixture")
        } else {
            provider
        };
    let (ready, receive) = tokio::sync::oneshot::channel();
    let mut server = tokio::spawn(async move {
        serve_with(config, &endpoint, provider, |handler| {
            let _ = ready.send(handler);
        })
        .await
    });
    let handler = tokio::select! {
        handler = receive => handler?,
        result = &mut server => { result??; return Ok(()); }
    };
    eprintln!("session: ready");
    loop {
        tokio::select! {
            result = &mut server => { result??; return Ok(()); }
            _ = tokio::time::sleep(Duration::from_millis(30)) => {
                if stop.exists() { break; }
            }
        }
    }
    handler.worker().shutdown().await;
    server.abort();
    let _ = server.await;
    eprintln!("session: stopped");
    Ok(())
}
