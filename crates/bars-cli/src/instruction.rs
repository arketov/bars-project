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
