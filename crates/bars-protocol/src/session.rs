//! Состояние одного соединения на стороне воркера: рукопожатие `hello` и права клиента.

use crate::PROTOCOL_VERSION;
use crate::error::{ErrorKind, METHOD_NOT_FOUND, RpcError};
use crate::jsonrpc::Request;
use crate::methods::{Call, ClientKind};

/// Соединение с клиентом. Воркер заводит по одному на соединение и пропускает через него каждый вызов.
#[derive(Debug, Default)]
pub struct Session {
    client: Option<ClientKind>,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// Кто подключился; `None` до успешного `hello`.
    pub fn client(&self) -> Option<ClientKind> {
        self.client
    }

    /// Проверяет вызов и разбирает его.
    ///
    /// - Первым должен быть `hello`; до него любой метод, даже неизвестный, — `protocol_mismatch`.
    /// - `hello` с чужой версией протокола — `protocol_mismatch` с просьбой обновить bars.
    /// - Метод, закрытый для `bars-mcp`, для него не существует: `-32601`.
    /// - У вызовов от `bars-mcp` `accept_model_change` сбрасывается: согласие на переиндексацию после
    ///   смены модели даёт только человек (Т-25), и воркер получает `false`.
    pub fn admit(&mut self, request: &Request) -> Result<Call, RpcError> {
        if self.client.is_none() && request.method != "hello" {
            return Err(RpcError::bars(
                ErrorKind::ProtocolMismatch,
                "первым вызовом должен быть hello",
            ));
        }
        let mut call = Call::from_request(request)?;
        if let Call::Hello(hello) = &call {
            if hello.protocol != PROTOCOL_VERSION {
                return Err(RpcError::bars(
                    ErrorKind::ProtocolMismatch,
                    format!(
                        "обновите bars: у клиента протокол {}, у воркера {PROTOCOL_VERSION}",
                        hello.protocol
                    ),
                ));
            }
            self.client = Some(hello.client);
            return Ok(call);
        }
        let client = self.client.expect("hello уже был");
        if !call.allowed_for(client) {
            return Err(RpcError::standard(
                METHOD_NOT_FOUND,
                format!("нет метода {}", call.method()),
            ));
        }
        if client == ClientKind::Mcp
            && let Call::Reindex(reindex) = &mut call
        {
            reindex.accept_model_change = false;
        }
        Ok(call)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonrpc::Id;
    use crate::methods::{
        HelloParams, Method, ProjectsParams, RegisterParams, ReindexParams, Target,
    };

    fn hello(protocol: u32, client: ClientKind) -> Request {
        HelloParams {
            protocol,
            client,
            client_version: "0.0.0".into(),
        }
        .request(Id::Number(1))
    }

    fn reindex(accept: bool) -> Request {
        ReindexParams {
            target: Target {
                cwd: "c".into(),
                project: None,
                wt: None,
            },
            full: false,
            accept_model_change: accept,
        }
        .request(Id::Number(2))
    }

    #[test]
    fn any_method_before_hello_is_protocol_mismatch() {
        let mut session = Session::new();
        let projects = ProjectsParams::default().request(Id::Number(1));
        let error = session.admit(&projects).unwrap_err();
        assert_eq!(error.kind(), Some(ErrorKind::ProtocolMismatch));

        let unknown = Request::new(Id::Number(1), "nope", serde_json::json!({}));
        let error = session.admit(&unknown).unwrap_err();
        assert_eq!(error.kind(), Some(ErrorKind::ProtocolMismatch));
        assert_eq!(session.client(), None);
    }

    #[test]
    fn foreign_protocol_version_is_rejected_with_update_hint() {
        let mut session = Session::new();
        let error = session
            .admit(&hello(PROTOCOL_VERSION + 1, ClientKind::Cli))
            .unwrap_err();
        assert_eq!(error.kind(), Some(ErrorKind::ProtocolMismatch));
        assert!(error.message.contains("обновите bars"));
        assert_eq!(session.client(), None);
    }

    #[test]
    fn hello_opens_the_session() {
        let mut session = Session::new();
        session
            .admit(&hello(PROTOCOL_VERSION, ClientKind::Mcp))
            .unwrap();
        assert_eq!(session.client(), Some(ClientKind::Mcp));
        let projects = ProjectsParams::default().request(Id::Number(2));
        assert!(matches!(session.admit(&projects), Ok(Call::Projects(_))));
    }

    #[test]
    fn mcp_cannot_call_cli_only_methods() {
        let mut session = Session::new();
        session
            .admit(&hello(PROTOCOL_VERSION, ClientKind::Mcp))
            .unwrap();
        let register = RegisterParams { path: "p".into() }.request(Id::Number(2));
        assert_eq!(session.admit(&register).unwrap_err().code, METHOD_NOT_FOUND);

        let mut cli = Session::new();
        cli.admit(&hello(PROTOCOL_VERSION, ClientKind::Cli))
            .unwrap();
        assert!(matches!(cli.admit(&register), Ok(Call::Register(_))));
    }

    #[test]
    fn consent_flag_survives_only_from_cli() {
        let mut mcp = Session::new();
        mcp.admit(&hello(PROTOCOL_VERSION, ClientKind::Mcp))
            .unwrap();
        let Ok(Call::Reindex(params)) = mcp.admit(&reindex(true)) else {
            panic!("reindex должен пройти");
        };
        assert!(!params.accept_model_change);

        let mut cli = Session::new();
        cli.admit(&hello(PROTOCOL_VERSION, ClientKind::Cli))
            .unwrap();
        let Ok(Call::Reindex(params)) = cli.admit(&reindex(true)) else {
            panic!("reindex должен пройти");
        };
        assert!(params.accept_model_change);
    }
}
