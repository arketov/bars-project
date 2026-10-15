//! JSON-RPC 2.0, одно сообщение на строку (`docs/architecture/protocol.md`).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{INVALID_REQUEST, PARSE_ERROR, RpcError};

const VERSION: &str = "2.0";

/// Идентификатор вызова. Уведомления (вызовы без `id`) протокол не использует.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Id {
    Number(i64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: Id,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl Request {
    pub fn new(id: Id, method: &str, params: Value) -> Self {
        Self {
            jsonrpc: VERSION.to_string(),
            id,
            method: method.to_string(),
            params: Some(params),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    /// `null` только у ответа на сообщение, из которого `id` достать не удалось.
    pub id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    pub fn ok(id: Id, result: Value) -> Self {
        Self {
            jsonrpc: VERSION.to_string(),
            id: Some(id),
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: Option<Id>, error: RpcError) -> Self {
        Self {
            jsonrpc: VERSION.to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }

    /// Результат или ошибка ответа.
    pub fn into_result(self) -> Result<Value, RpcError> {
        match (self.result, self.error) {
            (_, Some(error)) => Err(error),
            (Some(result), None) => Ok(result),
            (None, None) => Err(RpcError::standard(
                INVALID_REQUEST,
                "в ответе нет ни result, ни error",
            )),
        }
    }
}

/// Строка сообщения вместе с переводом строки.
pub fn encode_line<T: Serialize>(message: &T) -> String {
    let mut line = serde_json::to_string(message).expect("типы протокола сериализуются в JSON");
    line.push('\n');
    line
}

/// Разбирает строку с вызовом. При ошибке возвращает готовый ответ, который воркер отправит клиенту:
/// `-32700` для битого JSON, `-32600` для JSON, который не является вызовом.
pub fn parse_request(line: &str) -> Result<Request, Box<Response>> {
    let value: Value = serde_json::from_str(line.trim()).map_err(|error| {
        Box::new(Response::err(
            None,
            RpcError::standard(PARSE_ERROR, format!("не JSON: {error}")),
        ))
    })?;
    let id = value
        .get("id")
        .and_then(|id| serde_json::from_value::<Id>(id.clone()).ok());
    let invalid = |reason: String| {
        Box::new(Response::err(
            id.clone(),
            RpcError::standard(INVALID_REQUEST, reason),
        ))
    };
    let request: Request = serde_json::from_value(value)
        .map_err(|error| invalid(format!("не вызов JSON-RPC: {error}")))?;
    if request.jsonrpc != VERSION {
        return Err(invalid(format!(
            "поддерживается только jsonrpc \"{VERSION}\""
        )));
    }
    if let Some(params) = &request.params
        && !params.is_object()
    {
        return Err(invalid("params должен быть объектом".to_string()));
    }
    Ok(request)
}

/// Разбирает строку с ответом (на стороне клиента).
pub fn parse_response(line: &str) -> Result<Response, RpcError> {
    serde_json::from_str(line.trim())
        .map_err(|error| RpcError::standard(PARSE_ERROR, format!("не ответ JSON-RPC: {error}")))
}

/// Пустой объект параметров.
pub(crate) fn empty_params() -> Value {
    Value::Object(Map::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use serde_json::json;

    #[test]
    fn request_is_one_line() {
        let request = Request::new(Id::Number(1), "projects", json!({}));
        let line = encode_line(&request);
        assert!(line.ends_with('\n') && !line[..line.len() - 1].contains('\n'));
        assert_eq!(parse_request(&line).unwrap(), request);
    }

    #[test]
    fn newline_inside_text_stays_escaped() {
        let request = Request::new(Id::Text("a".into()), "x", json!({ "q": "a\nb" }));
        let line = encode_line(&request);
        assert_eq!(line.matches('\n').count(), 1);
        assert_eq!(parse_request(&line).unwrap().params.unwrap()["q"], "a\nb");
    }

    #[test]
    fn broken_json_is_parse_error_with_null_id() {
        let response = parse_request("{oops").unwrap_err();
        assert!(encode_line(&response).contains("\"id\":null"));
        assert_eq!(response.id, None);
        assert_eq!(response.error.unwrap().code, PARSE_ERROR);
    }

    #[test]
    fn json_that_is_not_a_call_is_invalid_request() {
        for line in [
            "[]",
            "42",
            r#"{"jsonrpc":"2.0","id":1}"#,
            r#"{"id":1,"method":"x"}"#,
        ] {
            let response = parse_request(line).unwrap_err();
            assert_eq!(response.error.unwrap().code, INVALID_REQUEST, "{line}");
        }
    }

    #[test]
    fn invalid_request_keeps_the_id_when_it_is_readable() {
        let response = parse_request(r#"{"jsonrpc":"1.0","id":7,"method":"x"}"#).unwrap_err();
        assert_eq!(response.id, Some(Id::Number(7)));
        let response =
            parse_request(r#"{"jsonrpc":"2.0","id":7,"method":"x","params":[1]}"#).unwrap_err();
        assert_eq!(response.error.unwrap().code, INVALID_REQUEST);
    }

    #[test]
    fn missing_params_are_allowed() {
        let request = parse_request(r#"{"jsonrpc":"2.0","id":"a","method":"projects"}"#).unwrap();
        assert_eq!(request.params, None);
    }

    #[test]
    fn response_roundtrip_and_into_result() {
        let ok = Response::ok(Id::Number(2), json!({ "a": 1 }));
        let parsed = parse_response(&encode_line(&ok)).unwrap();
        assert_eq!(parsed.into_result().unwrap(), json!({ "a": 1 }));

        let error = RpcError::bars(ErrorKind::ProjectNotFound, "нет проекта");
        let failed = Response::err(Some(Id::Number(3)), error.clone());
        let parsed = parse_response(&encode_line(&failed)).unwrap();
        assert_eq!(parsed.into_result().unwrap_err(), error);
    }

    #[test]
    fn empty_response_is_an_error() {
        let response = parse_response(r#"{"jsonrpc":"2.0","id":1}"#).unwrap();
        assert!(response.into_result().is_err());
    }
}
