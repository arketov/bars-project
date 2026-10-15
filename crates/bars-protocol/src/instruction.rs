//! Текстовая инструкция по установке для агента (`bars-cli install --print-instruction` и окно, Т-21).
//! Один текст на двоих: окно показывает ровно то, что печатает CLI.

/// Текст для агента. `cli` и `mcp` — пути (или имена) `bars-cli` и `bars-mcp`.
pub fn agent_install_text(cli: &str, mcp: &str) -> String {
    format!(
        "Подключи bars (локальный поиск по Markdown-документации проектов) к своему клиенту.\n\
         \n\
         Файлы bars: bars-cli — {cli}; bars-mcp — {mcp}.\n\
         \n\
         1. Выполни одну команду для своего клиента:\n\
         \x20  \"{cli}\" install --client claude-code\n\
         \x20  \"{cli}\" install --client codex\n\
         \x20  Она пропишет MCP-сервер bars, хук начала сессии и skills bars; перед правкой чужих\n\
         \x20  конфигов сделает копию `<файл>.bars-backup`.\n\
         2. Если клиент — Codex, скажи пользователю выполнить в Codex команду /hooks и одобрить\n\
         \x20  хук bars: без одобрения хук не запустится (MCP при этом уже работает).\n\
         3. Перезапусти клиент, чтобы он подхватил MCP-сервер.\n\
         \n\
         Другим MCP-клиентам (например, чату Claude Desktop) добавь вручную в конфиг MCP stdio-сервер\n\
         с командой {mcp} и без аргументов. Хука у них нет: проект регистрируется командой\n\
         \"{cli}\" register <папка проекта> или в окне bars.\n\
         \n\
         Запрещено: самому передавать `bars-cli reindex --accept-model-change`. Это согласие\n\
         пользователя на переиндексацию после смены модели; его даёт только человек.\n"
    )
}

/// Конфиг MCP-сервера для клиентов, которых bars не умеет ставить сам.
pub fn mcp_config_json(mcp: &str) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "mcpServers": { "bars": { "command": mcp, "args": [] } }
    }))
    .expect("конфиг сериализуется в JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_names_both_executables_and_forbids_consent() {
        let text = agent_install_text("/opt/bars/bars-cli", "/opt/bars/bars-mcp");
        assert!(text.contains("/opt/bars/bars-cli\" install --client codex"));
        assert!(text.contains("с командой /opt/bars/bars-mcp"));
        assert!(text.contains("Запрещено"));
    }

    #[test]
    fn mcp_config_is_valid_json_with_an_empty_args_list() {
        let value: serde_json::Value =
            serde_json::from_str(&mcp_config_json("/opt/bars-mcp")).unwrap();
        assert_eq!(value["mcpServers"]["bars"]["command"], "/opt/bars-mcp");
        assert_eq!(value["mcpServers"]["bars"]["args"], serde_json::json!([]));
    }
}
