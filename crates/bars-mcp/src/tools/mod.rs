//! Четыре агентских инструмента вместо семнадцати (шаг 5.2.1, IDEA-21): `bars_search`,
//! `bars_read`, `bars_status`, `bars_audit`. Описания и параметры — на английском (НФТ-4).

use super::*;

pub(super) mod audit;
pub(super) mod read;
pub(super) mod search;
pub(super) mod status;

/// Базовый набор: read, status, audit. Роутер `bars_search` выбирается в `BarsMcp::new`:
/// с настройкой `[mcp] graph_tools` регистрируется графовая форма, без неё — обычная.
pub(super) fn core_router() -> ToolRouter<BarsMcp> {
    read::router() + status::router() + audit::router()
}

/// Вид выдачи поиска: ступенчатая автоматика, полный текст всех результатов или только строки.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Detail {
    #[default]
    Auto,
    Full,
    Brief,
}

/// Продолжение дискового поиска вхождений: непрозрачная для агента связка path/offset/revision.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct OccurrencesCursor {
    /// Путь документа от корня проекта; продолжение вхождений одного документа.
    pub path: Option<String>,
    #[serde(default)]
    pub offset: usize,
    pub revision: Option<String>,
}

/// Проверяет, что префиксы области — непустые строки.
pub(super) fn scope_paths(r#in: &[String]) -> Result<Vec<String>, &'static str> {
    let paths: Vec<String> = r#in.iter().map(|path| path.trim().to_string()).collect();
    if paths.iter().any(|path| path.is_empty()) {
        return Err("invalid_params: in contains an empty path");
    }
    Ok(paths)
}
