//! Ошибки протокола: стандартные коды JSON-RPC и девять ошибок bars (`docs/architecture/protocol.md`).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const PARSE_ERROR: i32 = -32700;
pub const INVALID_REQUEST: i32 = -32600;
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INVALID_PARAMS: i32 = -32602;
pub const INTERNAL_ERROR: i32 = -32603;

/// Код выхода `bars-cli` для ошибок, которым не назначен свой.
pub const EXIT_FAILURE: i32 = 1;
/// Код выхода `bars-cli`, когда воркер недоступен: до протокола дело не дошло.
pub const EXIT_WORKER_UNAVAILABLE: i32 = 2;
/// Код выхода `bars-cli` для `reindex_required` и `reindex_needs_consent`.
pub const EXIT_REINDEX: i32 = 3;

/// Ошибка bars: код и машинное имя `data.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    ProtocolMismatch,
    ProjectNotFound,
    ProjectUnavailable,
    ReindexRequired,
    ReindexNeedsConsent,
    FileNotFound,
    InvalidParams,
    ProviderError,
    ModelLoading,
    GraphNotReady,
    GraphCapabilityUnavailable,
    StaleRef,
    SnapshotExpired,
    CursorMismatch,
    Cancelled,
}

impl ErrorKind {
    pub const ALL: [ErrorKind; 15] = [
        ErrorKind::ProtocolMismatch,
        ErrorKind::ProjectNotFound,
        ErrorKind::ProjectUnavailable,
        ErrorKind::ReindexRequired,
        ErrorKind::ReindexNeedsConsent,
        ErrorKind::FileNotFound,
        ErrorKind::InvalidParams,
        ErrorKind::ProviderError,
        ErrorKind::ModelLoading,
        ErrorKind::GraphNotReady,
        ErrorKind::GraphCapabilityUnavailable,
        ErrorKind::StaleRef,
        ErrorKind::SnapshotExpired,
        ErrorKind::CursorMismatch,
        ErrorKind::Cancelled,
    ];

    pub fn code(self) -> i32 {
        match self {
            ErrorKind::ProtocolMismatch => -32001,
            ErrorKind::ProjectNotFound => -32002,
            ErrorKind::ProjectUnavailable => -32003,
            ErrorKind::ReindexRequired => -32004,
            ErrorKind::ReindexNeedsConsent => -32005,
            ErrorKind::FileNotFound => -32006,
            ErrorKind::InvalidParams => -32007,
            ErrorKind::ProviderError => -32008,
            ErrorKind::ModelLoading => -32009,
            ErrorKind::GraphNotReady => -32010,
            ErrorKind::GraphCapabilityUnavailable => -32011,
            ErrorKind::StaleRef => -32012,
            ErrorKind::SnapshotExpired => -32013,
            ErrorKind::CursorMismatch => -32014,
            ErrorKind::Cancelled => -32015,
        }
    }

    /// Значение `data.kind`.
    pub fn name(self) -> &'static str {
        match self {
            ErrorKind::ProtocolMismatch => "protocol_mismatch",
            ErrorKind::ProjectNotFound => "project_not_found",
            ErrorKind::ProjectUnavailable => "project_unavailable",
            ErrorKind::ReindexRequired => "reindex_required",
            ErrorKind::ReindexNeedsConsent => "reindex_needs_consent",
            ErrorKind::FileNotFound => "file_not_found",
            ErrorKind::InvalidParams => "invalid_params",
            ErrorKind::ProviderError => "provider_error",
            ErrorKind::ModelLoading => "model_loading",
            ErrorKind::GraphNotReady => "graph_not_ready",
            ErrorKind::GraphCapabilityUnavailable => "graph_capability_unavailable",
            ErrorKind::StaleRef => "stale_ref",
            ErrorKind::SnapshotExpired => "snapshot_expired",
            ErrorKind::CursorMismatch => "cursor_mismatch",
            ErrorKind::Cancelled => "cancelled",
        }
    }

    pub fn from_code(code: i32) -> Option<ErrorKind> {
        Self::ALL.into_iter().find(|kind| kind.code() == code)
    }

    /// Код выхода `bars-cli`.
    pub fn exit_code(self) -> i32 {
        match self {
            ErrorKind::ReindexRequired | ErrorKind::ReindexNeedsConsent => EXIT_REINDEX,
            _ => EXIT_FAILURE,
        }
    }
}

/// Ошибка JSON-RPC: стандартная или bars.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[error("{message} ({code})")]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    /// Стандартная ошибка JSON-RPC (`-32700`…`-32603`), без `data`.
    pub fn standard(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Ошибка bars с `data.kind`.
    pub fn bars(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self::bars_with(kind, message, Map::new())
    }

    /// Ошибка bars с `data.kind` и дополнительными полями в `data` (например, ход загрузки модели).
    pub fn bars_with(
        kind: ErrorKind,
        message: impl Into<String>,
        extra: Map<String, Value>,
    ) -> Self {
        let mut data = Map::new();
        data.insert("kind".to_string(), Value::String(kind.name().to_string()));
        data.extend(extra);
        Self {
            code: kind.code(),
            message: message.into(),
            data: Some(Value::Object(data)),
        }
    }

    /// Ошибка bars, если код принадлежит одной из них.
    pub fn kind(&self) -> Option<ErrorKind> {
        ErrorKind::from_code(self.code)
    }

    /// Код выхода `bars-cli` для этой ошибки.
    pub fn exit_code(&self) -> i32 {
        self.kind().map_or(EXIT_FAILURE, ErrorKind::exit_code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_and_names_follow_the_table() {
        let expected = [
            (-32001, "protocol_mismatch"),
            (-32002, "project_not_found"),
            (-32003, "project_unavailable"),
            (-32004, "reindex_required"),
            (-32005, "reindex_needs_consent"),
            (-32006, "file_not_found"),
            (-32007, "invalid_params"),
            (-32008, "provider_error"),
            (-32009, "model_loading"),
            (-32010, "graph_not_ready"),
            (-32011, "graph_capability_unavailable"),
            (-32012, "stale_ref"),
            (-32013, "snapshot_expired"),
            (-32014, "cursor_mismatch"),
            (-32015, "cancelled"),
        ];
        for (kind, (code, name)) in ErrorKind::ALL.into_iter().zip(expected) {
            assert_eq!((kind.code(), kind.name()), (code, name));
            assert_eq!(ErrorKind::from_code(code), Some(kind));
        }
        assert_eq!(ErrorKind::from_code(INVALID_PARAMS), None);
    }

    #[test]
    fn exit_codes_follow_the_table() {
        assert_eq!(
            RpcError::bars(ErrorKind::ReindexRequired, "x").exit_code(),
            3
        );
        assert_eq!(
            RpcError::bars(ErrorKind::ReindexNeedsConsent, "x").exit_code(),
            3
        );
        assert_eq!(RpcError::bars(ErrorKind::FileNotFound, "x").exit_code(), 1);
        assert_eq!(RpcError::standard(INTERNAL_ERROR, "x").exit_code(), 1);
        assert_eq!(EXIT_WORKER_UNAVAILABLE, 2);
    }

    #[test]
    fn bars_error_carries_kind_and_extra_data() {
        let mut extra = Map::new();
        extra.insert("progress".to_string(), serde_json::json!(0.5));
        let error = RpcError::bars_with(ErrorKind::ModelLoading, "модель грузится", extra);
        let data = error.data.as_ref().unwrap();
        assert_eq!(data["kind"], "model_loading");
        assert_eq!(data["progress"], 0.5);
        assert_eq!(error.kind(), Some(ErrorKind::ModelLoading));
    }

    #[test]
    fn standard_error_has_no_data() {
        let json = serde_json::to_value(RpcError::standard(PARSE_ERROR, "x")).unwrap();
        assert_eq!(json, serde_json::json!({ "code": -32700, "message": "x" }));
    }
}
