//! Текстовая инструкция по установке для агента (`bars-cli install --print-instruction`, Т-21).

use std::path::Path;

/// Текст для агента. `exe` — путь к `bars-cli`, от него считается расположение `bars-mcp`.
pub fn text(exe: Option<&Path>) -> String {
    let suffix = std::env::consts::EXE_SUFFIX;
    let (cli, mcp) = match exe.and_then(|exe| Some((exe, exe.parent()?))) {
        Some((exe, dir)) => (
            exe.display().to_string(),
            dir.join(format!("bars-mcp{suffix}")).display().to_string(),
        ),
        None => (format!("bars-cli{suffix}"), format!("bars-mcp{suffix}")),
    };
    bars_protocol::agent_install_text(&cli, &mcp)
}
