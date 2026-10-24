//! Серверная часть локального канала воркера (`docs/architecture/ipc.md`).

#[cfg(windows)]
mod acl_windows;
mod listener;

use std::io;
use std::sync::Arc;

use bars_protocol::{Handler, serve_connection};
use interprocess::local_socket::tokio::Listener;
use interprocess::local_socket::traits::tokio::Listener as _;

pub use listener::{BindError, bind};

/// Принимает соединения и обслуживает каждое в своей задаче. Возвращается только с ошибкой `accept`.
pub async fn run_server<H: Handler>(listener: Listener, handler: Arc<H>) -> io::Result<()> {
    loop {
        let stream = listener.accept().await?;
        let handler = Arc::clone(&handler);
        tokio::spawn(async move {
            // Обрыв клиента посреди вызова не касается воркера и остальных соединений.
            let _ = serve_connection(stream, handler).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bars_protocol::{
        Call, Client, ClientError, ClientKind, Endpoint, ErrorKind, HelloParams, METHOD_NOT_FOUND,
        PROTOCOL_VERSION, ProjectsOutput, ProjectsParams, RegisterParams, RpcError, output_value,
    };
    use serde_json::Value;
    use std::time::{Duration, Instant};

    struct Fake;

    impl Handler for Fake {
        async fn handle(&self, _client: ClientKind, call: Call) -> Result<Value, RpcError> {
            match call {
                Call::Projects(_) => Ok(output_value(&ProjectsOutput { projects: vec![] })),
                other => Err(RpcError::bars(
                    ErrorKind::ProjectNotFound,
                    format!("заглушка не знает {}", other.method()),
                )),
            }
        }
    }

    fn start(name: &str) -> Endpoint {
        let endpoint = Endpoint::named(name);
        let listener = bind(&endpoint).unwrap();
        tokio::spawn(run_server(listener, Arc::new(Fake)));
        endpoint
    }

    #[tokio::test]
    async fn hello_then_call_over_the_real_channel() {
        let endpoint = start("it-hello");
        let mut client = Client::connect(&endpoint, ClientKind::Cli).await.unwrap();
        assert_eq!(client.worker_version(), env!("CARGO_PKG_VERSION"));
        assert_eq!(client.build_warning(), None, "клиент и воркер одной сборки");
        let out = client.call(&ProjectsParams {}).await.unwrap();
        assert!(out.projects.is_empty());
        let error = client
            .call(&RegisterParams { path: "x".into() })
            .await
            .unwrap_err();
        assert!(
            matches!(error, ClientError::Rpc(e) if e.kind() == Some(ErrorKind::ProjectNotFound))
        );
    }

    #[tokio::test]
    async fn mcp_cannot_call_cli_only_methods() {
        let endpoint = start("it-mcp");
        let mut client = Client::connect(&endpoint, ClientKind::Mcp).await.unwrap();
        let error = client
            .call(&RegisterParams { path: "x".into() })
            .await
            .unwrap_err();
        assert!(matches!(error, ClientError::Rpc(e) if e.code == METHOD_NOT_FOUND));
    }

    #[tokio::test]
    async fn foreign_protocol_version_is_refused_with_an_update_hint() {
        let endpoint = start("it-version");
        let mut client = Client::connect(&endpoint, ClientKind::Cli).await.unwrap();
        let error = client
            .call(&HelloParams {
                protocol: PROTOCOL_VERSION + 1,
                client: ClientKind::Cli,
                client_version: "x".into(),
            })
            .await
            .unwrap_err();
        let ClientError::Rpc(error) = error else {
            panic!("ждали ошибку воркера")
        };
        assert_eq!(error.kind(), Some(ErrorKind::ProtocolMismatch));
        assert!(error.message.contains("обновите bars"));
    }

    #[tokio::test]
    async fn no_worker_is_unavailable() {
        let endpoint = Endpoint::named("it-nobody");
        let error = Client::connect(&endpoint, ClientKind::Cli)
            .await
            .err()
            .unwrap();
        assert!(matches!(error, ClientError::Unavailable(_)));
        assert_eq!(error.exit_code(), bars_protocol::EXIT_WORKER_UNAVAILABLE);

        let begin = Instant::now();
        let error =
            Client::connect_with_retry(&endpoint, ClientKind::Cli, Duration::from_millis(300))
                .await
                .err()
                .unwrap();
        assert!(matches!(error, ClientError::Unavailable(_)));
        let spent = begin.elapsed();
        assert!(
            spent >= Duration::from_millis(280) && spent < Duration::from_secs(3),
            "{spent:?}"
        );
    }

    #[tokio::test]
    async fn retry_connects_when_the_worker_comes_up_late() {
        let endpoint = Endpoint::named("it-late");
        let late = endpoint.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let listener = bind(&late).unwrap();
            run_server(listener, Arc::new(Fake)).await.unwrap();
        });
        let mut client =
            Client::connect_with_retry(&endpoint, ClientKind::Cli, Duration::from_secs(5))
                .await
                .unwrap();
        client.call(&ProjectsParams {}).await.unwrap();
    }

    #[tokio::test]
    async fn ten_clients_at_once_and_a_dropped_one_changes_nothing() {
        let endpoint = start("it-many");
        // Клиент, оборвавший соединение сразу после hello.
        drop(Client::connect(&endpoint, ClientKind::Cli).await.unwrap());
        let mut tasks = Vec::new();
        for _ in 0..10 {
            let endpoint = endpoint.clone();
            tasks.push(tokio::spawn(async move {
                let mut client = Client::connect(&endpoint, ClientKind::Cli).await.unwrap();
                for _ in 0..5 {
                    client.call(&ProjectsParams {}).await.unwrap();
                }
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
    }
}
