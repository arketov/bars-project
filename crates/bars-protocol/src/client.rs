//! Клиент воркера для `bars-mcp` и `bars-cli` (`docs/architecture/ipc.md`).

use std::io;
use std::pin::Pin;
use std::time::Duration;

use interprocess::local_socket::tokio::Stream;
use interprocess::local_socket::traits::tokio::Stream as _;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tokio::time::Instant;

use crate::PROTOCOL_VERSION;
use crate::endpoint::Endpoint;
use crate::error::{ErrorKind, RpcError};
use crate::framing::{MAX_RESPONSE_LINE, read_line_limited, write_line};
use crate::jsonrpc::{Id, encode_line, parse_response};
use crate::methods::{ClientKind, HelloOutput, HelloParams, Method};

/// Почему вызов не получился.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// К каналу не подключиться: воркер не запущен или не отвечает. Код выхода CLI — 2.
    #[error("воркер bars недоступен: {0}")]
    Unavailable(io::Error),
    /// Соединение оборвалось или собеседник прислал не строку протокола.
    #[error("ошибка канала: {0}")]
    Io(#[from] io::Error),
    /// Серверный конец канала создал процесс чужого пользователя или его владельца не удалось
    /// узнать (OQ-38). Клиент ничего не отправил, кроме проверки владельца.
    #[error("канал bars занят чужим процессом: {0}")]
    UntrustedServer(String),
    /// Воркер ответил ошибкой.
    #[error("{0}")]
    Rpc(RpcError),
    /// Воркер ответил не тем, что ждёт клиент: не JSON-RPC, чужой `id`, ответ не по схеме метода.
    #[error("воркер ответил непонятно: {0}")]
    BadReply(String),
}

impl ClientError {
    /// Код выхода CLI.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Unavailable(_) => crate::EXIT_WORKER_UNAVAILABLE,
            Self::Rpc(error) => error.exit_code(),
            Self::Io(_) | Self::UntrustedServer(_) | Self::BadReply(_) => crate::EXIT_FAILURE,
        }
    }
}

/// Windows: имя канала предсказуемо, и чужой пользователь мог занять его раньше воркера. До `hello`
/// проверяем, что серверный процесс принадлежит тому же пользователю, что и клиент (OQ-38).
#[cfg(windows)]
fn verify_server_owner(stream: &Stream) -> Result<(), ClientError> {
    use interprocess::local_socket::traits::StreamCommon as _;
    let pid = stream
        .peer_creds()
        .ok()
        .and_then(|creds| creds.pid())
        .ok_or_else(|| ClientError::UntrustedServer("не удалось узнать процесс сервера".into()))?;
    match crate::winsid::owned_by_current_user(pid) {
        Ok(true) => Ok(()),
        Ok(false) => Err(ClientError::UntrustedServer(
            "процесс сервера принадлежит другому пользователю".into(),
        )),
        Err(error) => Err(ClientError::UntrustedServer(format!(
            "не удалось определить владельца процесса сервера: {error}"
        ))),
    }
}

type Reader = BufReader<Pin<Box<dyn AsyncRead + Send>>>;
type Writer = Pin<Box<dyn AsyncWrite + Send>>;

/// Соединение с воркером после `hello`. Вызовы идут по очереди: следующий — после ответа на предыдущий.
/// Таймаут ожидания ответа ставит вызывающий (`tokio::time::timeout`).
pub struct Client {
    reader: Reader,
    writer: Writer,
    next_id: i64,
    worker_version: String,
}

impl Client {
    /// Подключается к каналу и делает `hello`. Нет канала — [`ClientError::Unavailable`].
    pub async fn connect(endpoint: &Endpoint, kind: ClientKind) -> Result<Self, ClientError> {
        let name = endpoint.name().map_err(ClientError::Unavailable)?;
        let stream = Stream::connect(name)
            .await
            .map_err(ClientError::Unavailable)?;
        #[cfg(windows)]
        verify_server_owner(&stream)?;
        Self::over(stream, kind).await
    }

    /// Как [`Client::connect`], но пока воркер не отвечает, повторяет попытки с нарастающей паузой
    /// (50 мс, 100, 200, … до 500 мс) до `deadline`. Для запуска воркера по требованию.
    pub async fn connect_with_retry(
        endpoint: &Endpoint,
        kind: ClientKind,
        deadline: Duration,
    ) -> Result<Self, ClientError> {
        let stop = Instant::now() + deadline;
        let mut pause = Duration::from_millis(50);
        loop {
            match Self::connect(endpoint, kind).await {
                Err(ClientError::Unavailable(error)) => {
                    let now = Instant::now();
                    if now >= stop {
                        return Err(ClientError::Unavailable(error));
                    }
                    tokio::time::sleep(pause.min(stop - now)).await;
                    pause = (pause * 2).min(Duration::from_millis(500));
                }
                other => return other,
            }
        }
    }

    /// `hello` по уже открытому потоку: для тестов и для транспортов, кроме локального канала.
    pub async fn over<S>(stream: S, kind: ClientKind) -> Result<Self, ClientError>
    where
        S: AsyncRead + AsyncWrite + Send + 'static,
    {
        let (read_half, write_half) = tokio::io::split(stream);
        let mut client = Self {
            reader: BufReader::new(Box::pin(read_half)),
            writer: Box::pin(write_half),
            next_id: 1,
            worker_version: String::new(),
        };
        let hello = client
            .call(&HelloParams {
                protocol: PROTOCOL_VERSION,
                client: kind,
                client_version: env!("CARGO_PKG_VERSION").to_string(),
            })
            .await?;
        let HelloOutput {
            protocol,
            worker_version,
        } = hello;
        if protocol != PROTOCOL_VERSION {
            return Err(ClientError::Rpc(RpcError::bars(
                ErrorKind::ProtocolMismatch,
                format!(
                    "обновите bars: у клиента протокол {PROTOCOL_VERSION}, у воркера {protocol}"
                ),
            )));
        }
        client.worker_version = worker_version;
        Ok(client)
    }

    /// Версия воркера из `hello`.
    pub fn worker_version(&self) -> &str {
        &self.worker_version
    }

    /// Вызывает метод и ждёт ответ.
    pub async fn call<M: Method>(&mut self, params: &M) -> Result<M::Output, ClientError> {
        self.call_observed(params, None).await
    }

    /// Исходный MCP-вход; worker сохраняет только разрешённые поля.
    pub async fn call_observed<M: Method>(
        &mut self,
        params: &M,
        context: Option<&serde_json::Value>,
    ) -> Result<M::Output, ClientError> {
        let id = Id::Number(self.next_id);
        self.next_id += 1;
        let mut request = params.request(id.clone());
        if let (Some(context), Some(serde_json::Value::Object(fields))) =
            (context, request.params.as_mut())
        {
            fields.insert("_mcp".into(), context.clone());
        }
        // Наблюдение не превращает допустимый большой вызов в ошибку framing.
        if encode_line(&request).len() > crate::MAX_REQUEST_LINE
            && let Some(serde_json::Value::Object(fields)) = request.params.as_mut()
        {
            fields.remove("_mcp");
        }
        write_line(&mut self.writer, &encode_line(&request)).await?;
        let line = read_line_limited(&mut self.reader, MAX_RESPONSE_LINE)
            .await?
            .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))?;
        let response =
            parse_response(&line).map_err(|error| ClientError::BadReply(error.message))?;
        if response.id.is_some() && response.id != Some(id) {
            return Err(ClientError::BadReply(
                "id ответа не совпал с id вызова".into(),
            ));
        }
        let value = response.into_result().map_err(ClientError::Rpc)?;
        serde_json::from_value(value).map_err(|error| ClientError::BadReply(error.to_string()))
    }
}
