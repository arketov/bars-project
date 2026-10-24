//! Цикл соединения на стороне воркера, не зависящий от транспорта (`docs/architecture/ipc.md`).

use std::io;
use std::sync::Arc;

use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};

use crate::PROTOCOL_VERSION;
use crate::error::{INVALID_REQUEST, RpcError};
use crate::framing::{MAX_REQUEST_LINE, read_line_limited, write_line};
use crate::jsonrpc::{Response, encode_line, parse_request};
use crate::methods::{Call, ClientKind, HelloOutput, output_value};
use crate::session::Session;

/// Выполняет вызовы, которые пережили проверку сессии. `hello` обрабатывает сам цикл соединения.
pub trait Handler: Send + Sync + 'static {
    fn handle_observed(
        &self,
        session: &str,
        client: ClientKind,
        call: Call,
        _context: Option<Value>,
    ) -> impl Future<Output = Result<Value, RpcError>> + Send {
        self.handle_session(session, client, call)
    }
    /// Контекст одного соединения; реализация по умолчанию не собирает статистику.
    fn handle_session(
        &self,
        _session: &str,
        client: ClientKind,
        call: Call,
    ) -> impl Future<Output = Result<Value, RpcError>> + Send {
        self.handle(client, call)
    }

    fn handle(
        &self,
        client: ClientKind,
        call: Call,
    ) -> impl Future<Output = Result<Value, RpcError>> + Send;
}

/// Обслуживает одно соединение до его закрытия: строка → `Session::admit` → обработчик → строка.
///
/// Ошибка вызова уходит клиенту ответом, соединение остаётся. Строка длиннее [`MAX_REQUEST_LINE`]
/// получает ответ `-32600`, после чего соединение закрывается: продолжение такой строки
/// невозможно отличить от следующего вызова. Возвращает `Ok`, когда клиент закрыл соединение.
pub async fn serve_connection<S, H>(stream: S, handler: Arc<H>) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
    H: Handler,
{
    let (read_half, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(read_half);
    let mut session = Session::new();
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let connection_id = format!(
        "{}-{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    loop {
        let line = match read_line_limited(&mut reader, MAX_REQUEST_LINE).await {
            Ok(Some(line)) => line,
            Ok(None) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                let response = Response::err(
                    None,
                    RpcError::standard(INVALID_REQUEST, format!("не строка вызова: {error}")),
                );
                // Клиент мог уже уйти: ответ необязателен.
                let _ = write_line(&mut writer, &encode_line(&response)).await;
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        let request = match parse_request(&line) {
            Ok(request) => request,
            Err(response) => {
                write_line(&mut writer, &encode_line(&*response)).await?;
                continue;
            }
        };
        let id = request.id.clone();
        let response = match session.admit(&request) {
            Err(error) => Response::err(Some(id), error),
            Ok(Call::Hello(_)) => Response::ok(
                id,
                output_value(&HelloOutput {
                    protocol: PROTOCOL_VERSION,
                    worker_version: env!("CARGO_PKG_VERSION").to_string(),
                    worker_build: crate::BUILD_ID.to_string(),
                }),
            ),
            Ok(call) => {
                let client = session.client().expect("после hello клиент известен");
                let context = request.params.as_ref().and_then(|p| p.get("_mcp")).cloned();
                match handler
                    .handle_observed(&connection_id, client, call, context)
                    .await
                {
                    Ok(result) => Response::ok(id, result),
                    Err(error) => Response::err(Some(id), error),
                }
            }
        };
        write_line(&mut writer, &encode_line(&response)).await?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use crate::jsonrpc::parse_response;
    use serde_json::json;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, duplex};

    struct Echo;

    impl Handler for Echo {
        async fn handle(&self, _client: ClientKind, call: Call) -> Result<Value, RpcError> {
            Ok(json!({ "method": call.method() }))
        }
    }

    /// Поднимает цикл соединения и отдаёт сторону клиента.
    fn connect() -> (
        BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
        tokio::io::WriteHalf<tokio::io::DuplexStream>,
        tokio::task::JoinHandle<io::Result<()>>,
    ) {
        let (client, server) = duplex(1 << 16);
        let task = tokio::spawn(serve_connection(server, Arc::new(Echo)));
        let (read, write) = tokio::io::split(client);
        (BufReader::new(read), write, task)
    }

    async fn ask(
        reader: &mut BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
        writer: &mut tokio::io::WriteHalf<tokio::io::DuplexStream>,
        line: &str,
    ) -> Response {
        write_line(writer, line).await.unwrap();
        let mut reply = String::new();
        reader.read_line(&mut reply).await.unwrap();
        parse_response(&reply).unwrap()
    }

    const HELLO: &str = r#"{"jsonrpc":"2.0","id":1,"method":"hello","params":{"protocol":1,"client":"cli","client_version":"t"}}"#;

    #[tokio::test]
    async fn method_before_hello_is_protocol_mismatch_and_connection_stays() {
        let (mut reader, mut writer, _task) = connect();
        let reply = ask(
            &mut reader,
            &mut writer,
            r#"{"jsonrpc":"2.0","id":9,"method":"projects"}"#,
        )
        .await;
        assert_eq!(
            reply.error.unwrap().kind(),
            Some(ErrorKind::ProtocolMismatch)
        );
        let reply = ask(&mut reader, &mut writer, HELLO).await;
        assert!(reply.error.is_none());
        let reply = ask(
            &mut reader,
            &mut writer,
            r#"{"jsonrpc":"2.0","id":2,"method":"projects"}"#,
        )
        .await;
        assert_eq!(reply.result.unwrap()["method"], "projects");
    }

    #[tokio::test]
    async fn broken_json_gets_an_error_and_the_next_call_still_works() {
        let (mut reader, mut writer, _task) = connect();
        let reply = ask(&mut reader, &mut writer, "{oops").await;
        assert_eq!(reply.error.unwrap().code, crate::error::PARSE_ERROR);
        assert!(ask(&mut reader, &mut writer, HELLO).await.error.is_none());
    }

    #[tokio::test]
    async fn oversized_line_gets_invalid_request_and_the_connection_closes() {
        let (mut reader, mut writer, task) = connect();
        let big = "x".repeat(MAX_REQUEST_LINE + 10);
        writer.write_all(big.as_bytes()).await.unwrap();
        writer.write_all(b"\n").await.unwrap();
        let mut reply = String::new();
        reader.read_line(&mut reply).await.unwrap();
        assert_eq!(
            parse_response(&reply).unwrap().error.unwrap().code,
            INVALID_REQUEST
        );
        let error = task.await.unwrap().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn clean_close_between_lines_is_ok() {
        let (mut reader, mut writer, task) = connect();
        assert!(ask(&mut reader, &mut writer, HELLO).await.error.is_none());
        drop(writer);
        drop(reader);
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn original_context_survives_but_never_breaks_request_size() {
        struct Observed(std::sync::Mutex<Vec<Option<Value>>>);
        impl Handler for Observed {
            async fn handle_observed(
                &self,
                _: &str,
                client: ClientKind,
                call: Call,
                context: Option<Value>,
            ) -> Result<Value, RpcError> {
                self.0.lock().unwrap().push(context);
                self.handle(client, call).await
            }
            async fn handle(&self, _: ClientKind, _: Call) -> Result<Value, RpcError> {
                Ok(json!({"title":"t","text":""}))
            }
        }
        let handler = Arc::new(Observed(std::sync::Mutex::new(Vec::new())));
        let (client, server) = duplex(1 << 16);
        let h = handler.clone();
        tokio::spawn(serve_connection(server, h));
        let mut client = crate::Client::over(client, ClientKind::Mcp).await.unwrap();
        let mut params = crate::ReadParams {
            target: crate::Target {
                cwd: "/p".into(),
                project: None,
            },
            path: "a.md".into(),
        };
        client
            .call_observed(&params, Some(&json!({"path":"a.md","format":null})))
            .await
            .unwrap();
        params.path = "a".repeat(MAX_REQUEST_LINE - 1024);
        client
            .call_observed(&params, Some(&json!({"path":params.path})))
            .await
            .unwrap();
        let observations = handler.0.lock().unwrap();
        assert_eq!(observations[0], Some(json!({"path":"a.md","format":null})));
        assert!(observations[1].is_none());
    }
}
