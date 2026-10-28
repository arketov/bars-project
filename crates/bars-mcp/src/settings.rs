//! Единственная настройка, которую прокси читает сам: `[mcp] graph_tools` в `~/.bars/settings.toml`.
//!
//! Список инструментов отдаётся без воркера (НФТ-3), поэтому спросить его о настройке нельзя, а
//! `bars-core` прокси не подключает. Файл читается один раз при начале сессии MCP: клиент берёт
//! список инструментов при старте, и смена настройки доходит до агента со следующей сессией.

use std::path::Path;

/// Имя файла настроек пользователя в `~/.bars` (то же, что `bars_core::settings::USER_SETTINGS_FILE`).
const USER_SETTINGS_FILE: &str = "settings.toml";

/// Показывать ли шесть графовых инструментов (OQ-162). Нет файла, ошибка чтения или разбора,
/// ключ другого типа — скрыты: по умолчанию агенту показывается только работающее. Ошибку файла
/// настроек показывает окно, прокси молчит.
pub(crate) fn graph_tools_enabled(bars_home: Option<&Path>) -> bool {
    let Some(home) = bars_home else {
        return false;
    };
    let Ok(text) = std::fs::read_to_string(home.join(USER_SETTINGS_FILE)) else {
        return false;
    };
    graph_tools_in(&text)
}

fn graph_tools_in(text: &str) -> bool {
    text.parse::<toml::Table>()
        .ok()
        .and_then(|table| table.get("mcp")?.get("graph_tools")?.as_bool())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_explicit_true_shows_the_graph_tools() {
        assert!(graph_tools_in("[mcp]\ngraph_tools = true\n"));
        assert!(!graph_tools_in("[mcp]\ngraph_tools = false\n"));
        assert!(!graph_tools_in(""));
        assert!(!graph_tools_in("[search]\nresults = 7\n"));
        assert!(!graph_tools_in("[mcp]\ngraph_tools = \"true\"\n"));
        assert!(!graph_tools_in("[mcp\ngraph_tools = true\n"));
    }

    #[test]
    fn missing_home_or_file_hides_them() {
        assert!(!graph_tools_enabled(None));
        let home = tempfile::tempdir().unwrap();
        assert!(!graph_tools_enabled(Some(home.path())));
        std::fs::write(
            home.path().join(USER_SETTINGS_FILE),
            "[mcp]\ngraph_tools = true\n",
        )
        .unwrap();
        assert!(graph_tools_enabled(Some(home.path())));
    }
}
