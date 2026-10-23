/// Роль документа по пути от корня проекта: отделяет дубль от законного повтора (3.8.2).
///
/// Ключи: `index`, `session_log`, `manual`, `feature`, `architecture`, `decision`, `research`,
/// `backlog`, `document`. Подписи на русском даёт окно.
pub fn document_role(path: &str) -> &'static str {
    let normalized = path.replace('\\', "/");
    let mut parts: Vec<&str> = normalized.split('/').filter(|p| !p.is_empty()).collect();
    let Some(file) = parts.pop() else {
        return "document";
    };
    if file.eq_ignore_ascii_case("index.md") {
        return "index";
    }
    for part in parts {
        match part {
            "chats" => return "session_log",
            "manual" => return "manual",
            "features" => return "feature",
            "architecture" => return "architecture",
            "adr" => return "decision",
            "knowledge" => return "research",
            "backlog" => return "backlog",
            _ => {}
        }
    }
    "document"
}
