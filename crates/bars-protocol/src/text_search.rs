//! Поиск вхождений на диске: параметры окна и общий результат, без индекса и моделей.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextSearchParams {
    pub project: String,
    pub query: String,
    pub case_sensitive: bool,
    pub regex: bool,
    pub path_filter: String,
    pub modules: Vec<String>,
    pub files: Vec<String>,
    /// Пути-префиксы области (папка любой глубины или файл, от корня документации или проекта);
    /// объединяются — агентский параметр `in` дискового поиска MCP.
    pub paths: Vec<String>,
    pub limit_hits: Option<usize>,
    pub limit_docs: Option<usize>,
    pub request_id: String,
    /// Подставляется командой Tauri, не берётся из JavaScript.
    pub window_id: String,
    /// Продолжение одного документа; путь от корня проекта.
    pub path: Option<String>,
    pub offset: usize,
    pub revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextHit {
    pub line: u32,
    /// UTF-16, с 1; длина и диапазоны сниппета также UTF-16.
    pub column: u32,
    pub length: u32,
    pub text: String,
    pub ranges: Vec<[u32; 2]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextDocument {
    pub path: String,
    pub module: String,
    pub title: String,
    pub revision: String,
    pub hits_total: usize,
    pub hits: Vec<TextHit>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextSkipped {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextSearchOutput {
    pub documents: Vec<TextDocument>,
    pub docs_total: usize,
    pub hits_total: usize,
    /// `docs`, `hits`, `page` или null. При усечении итоги — нижняя граница.
    pub truncated: Option<String>,
    pub skipped: Vec<TextSkipped>,
    #[serde(default)]
    pub skipped_total: usize,
    #[serde(default)]
    pub skipped_truncated: bool,
    pub elapsed_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Call, ClientKind, Id, Method};
    #[test]
    fn text_search_is_window_only_and_defaults_roundtrip() {
        let p = TextSearchParams {
            project: "C:/p".into(),
            query: "текст".into(),
            ..Default::default()
        };
        let call = Call::from_request(&p.request(Id::Number(1))).unwrap();
        assert_eq!(call, Call::TextSearch(p));
        assert!(call.window_only());
        assert!(!call.allowed_for(ClientKind::Cli));
        assert!(!call.allowed_for(ClientKind::Mcp));
        let minimal: TextSearchParams =
            serde_json::from_value(serde_json::json!({"project":"C:/p","query":"x"})).unwrap();
        assert!(!minimal.case_sensitive);
        assert!(!minimal.regex);
        assert!(minimal.modules.is_empty());
    }
}
