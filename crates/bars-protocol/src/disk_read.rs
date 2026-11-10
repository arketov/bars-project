//! Ограниченные агентские операции чтения; оконные идентификаторы сюда не входят.
use crate::{Target, TextSearchOutput};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskTextParams {
    #[serde(flatten)]
    pub target: Target,
    pub query: String,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub path_filter: String,
    #[serde(default)]
    pub modules: Vec<String>,
    #[serde(default)]
    pub files: Vec<String>,
    /// Пути-префиксы области (папка любой глубины или файл, от корня документации или проекта);
    /// объединяются — агентский параметр `in` инструмента `bars_search`.
    #[serde(default)]
    pub paths: Vec<String>,
    pub limit_docs: Option<usize>,
    pub limit_hits: Option<usize>,
    pub path: Option<String>,
    #[serde(default)]
    pub offset: usize,
    pub revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskTextOutput {
    #[serde(flatten)]
    pub result: TextSearchOutput,
    pub requested: serde_json::Value,
    pub applied: serde_json::Value,
    pub clamped: bool,
    pub limit_reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskLinksParams {
    #[serde(flatten)]
    pub target: Target,
    pub path: String,
    #[serde(default = "outgoing")]
    pub direction: String,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}
fn outgoing() -> String {
    "outgoing".into()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskLinksOutput {
    pub known: bool,
    pub direction: String,
    pub items: Vec<serde_json::Value>,
    pub applied_limit: usize,
    pub clamped: bool,
    pub snapshot: String,
    pub has_more: bool,
    pub next_cursor: Option<String>,
    pub truncated: bool,
    pub limit_reasons: Vec<String>,
    pub elapsed_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Call, ClientKind, Id, Method};
    #[test]
    fn disk_read_admission_does_not_open_window_methods() {
        let p: DiskTextParams =
            serde_json::from_value(serde_json::json!({"cwd":"C:/p","query":"x"})).unwrap();
        let call = Call::from_request(&p.request(Id::Number(1))).unwrap();
        assert!(call.allowed_for(ClientKind::Mcp));
        assert!(!call.window_only());
        let links: DiskLinksParams =
            serde_json::from_value(serde_json::json!({"cwd":"C:/p","path":"docs/a.md"})).unwrap();
        assert_eq!(links.direction, "outgoing");
        assert!(
            Call::from_request(&links.request(Id::Number(2)))
                .unwrap()
                .allowed_for(ClientKind::Mcp)
        );
        assert!(
            !Call::LinksOf(crate::LinksOfParams {
                project: "C:/p".into(),
                path: "docs/a.md".into()
            })
            .allowed_for(ClientKind::Mcp)
        );
        assert!(!Call::TextSearch(crate::TextSearchParams::default()).allowed_for(ClientKind::Mcp));
    }
}
