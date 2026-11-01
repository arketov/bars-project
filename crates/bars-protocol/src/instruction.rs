//! Текстовая инструкция по установке для агента (`bars-cli install --print-instruction` и окно, Т-21).
//! Один текст на двоих: окно показывает ровно то, что печатает CLI.

/// Текст для агента. `cli` и `mcp` — пути (или имена) `bars-cli` и `bars-mcp`.
pub fn agent_install_text(cli: &str, mcp: &str) -> String {
    format!(
        "Connect bars (local search over a project's Markdown documentation) to your client.\n\
         \n\
         bars files: bars-cli - {cli}; bars-mcp - {mcp}.\n\
         \n\
         1. Run one command for your client:\n\
         \x20  \"{cli}\" install --client claude-code\n\
         \x20  \"{cli}\" install --client codex\n\
         \x20  \"{cli}\" install --client kimi-code\n\
         \x20  \"{cli}\" install --client ouroboros\n\
         \x20  Ouroboros: local MCP only, no hook or skills. Register\n\
         \x20  projects in bars or with bars-cli register. Custom OUROBOROS_* paths are unsupported.\n\
         \x20  Other listed clients also receive the session-start hook and bars skills; before\n\
         \x20  editing other configs it makes a copy named `<file>.bars-backup`.\n\
         2. If the client is Codex, tell the user to run /hooks in Codex and approve the bars hook:\n\
         \x20  without approval the hook does not run (the MCP server already works).\n\
         3. Restart the client so it picks up the MCP server.\n\
         \n\
         For other MCP clients (for example the Claude Desktop chat), add a stdio MCP server to the\n\
         client's MCP config by hand: command {mcp}, no arguments. These clients have no hook, so\n\
         register a project with \"{cli}\" register <project folder> or in the bars window.\n\
         \n\
         Forbidden: never pass `bars-cli reindex --accept-model-change` yourself. It is the user's\n\
         consent to reindex after a model change; only a human gives it.\n"
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
        assert!(text.contains("/opt/bars/bars-cli\" install --client kimi-code"));
        assert!(text.contains("command /opt/bars/bars-mcp"));
        assert!(text.contains("Forbidden: never pass"));
    }

    #[test]
    fn mcp_config_is_valid_json_with_an_empty_args_list() {
        let value: serde_json::Value =
            serde_json::from_str(&mcp_config_json("/opt/bars-mcp")).unwrap();
        assert_eq!(value["mcpServers"]["bars"]["command"], "/opt/bars-mcp");
        assert_eq!(value["mcpServers"]["bars"]["args"], serde_json::json!([]));
    }
}
