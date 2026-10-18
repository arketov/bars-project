use super::*;
use crate::worker::models::fake::FakeProvider;
use bars_protocol::{Client, ClientKind, InstallClient, ProjectsParams};
use std::time::Duration;
use tempfile::TempDir;

#[tokio::test]
#[allow(clippy::await_holding_lock)] // Барьер намеренно удерживает фоновую запись во время IPC.
async fn blocked_refresh_does_not_delay_ipc_and_shutdown_waits_for_it() {
    let home = TempDir::new().unwrap();
    let mut config = WorkerConfig::new(home.path().join(".bars"));
    let env = crate::clients::InstallEnv {
        home: home.path().to_path_buf(),
        mcp_exe: "/new/bars-mcp".into(),
        cli_exe: "/new/bars-cli".into(),
    };
    std::fs::write(
        home.path().join(".claude.json"),
        r#"{"mcpServers":{"bars":{"command":"old"}}}"#,
    )
    .unwrap();
    config.install_env = Some(env.clone());
    let endpoint = Endpoint::named("refresh-blocked-ready");
    // Barьер — настоящий замок операций, а не задержка по времени.
    let guard = crate::clients::lock();
    let (ready, receive) = tokio::sync::oneshot::channel();
    let worker_config = config.clone();
    let worker_endpoint = endpoint.clone();
    let server = tokio::spawn(async move {
        serve_with(
            worker_config,
            &worker_endpoint,
            FakeProvider::new("m1"),
            |handler| {
                let _ = ready.send(handler);
            },
        )
        .await
    });
    let handler = tokio::time::timeout(Duration::from_secs(5), receive)
        .await
        .unwrap()
        .unwrap();
    for kind in [ClientKind::Cli, ClientKind::Mcp] {
        let mut client = Client::connect_with_retry(&endpoint, kind, Duration::from_secs(5))
            .await
            .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(2), client.call(&ProjectsParams {}))
            .await
            .unwrap()
            .unwrap();
        assert!(result.projects.is_empty());
    }
    // Второй процесс/сервер не начинает собственный проход.
    assert!(matches!(
        serve(config, &endpoint, FakeProvider::new("m1")).await,
        Err(ServeError::AlreadyRunning)
    ));
    let worker = handler.worker().clone();
    let stopping = tokio::spawn(async move {
        worker.shutdown().await;
    });
    tokio::task::yield_now().await;
    assert!(!stopping.is_finished());
    drop(guard);
    tokio::time::timeout(Duration::from_secs(5), stopping)
        .await
        .unwrap()
        .unwrap();
    assert!(crate::clients::state(&env, InstallClient::ClaudeCode).installed);
    assert!(!crate::clients::state(&env, InstallClient::ClaudeCode).stale);
    server.abort();
    let _ = server.await;
}
