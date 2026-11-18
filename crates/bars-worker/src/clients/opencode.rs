//! OpenCode подключается MCP-сервером в `~/.config/opencode/opencode.json` (или `opencode.jsonc`)
//! и skills в `~/.config/opencode/skills/`. Хука нет — проект регистрируется окном или CLI.
use std::path::{Path, PathBuf};

use super::{InstallEnv, Result, fsx};

/// Путь к конфигу OpenCode: `XDG_CONFIG_HOME/opencode/opencode.json` или `~/.config/opencode/opencode.json`.
/// Если `.json` нет, но есть `.jsonc` — правим `.jsonc`.
fn opencode_config(env: &InstallEnv) -> PathBuf {
    let config_dir = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| env.home.join(".config"))
        .join("opencode");

    let json = config_dir.join("opencode.json");
    let jsonc = config_dir.join("opencode.jsonc");
    if json.exists() {
        json
    } else {
        jsonc
    }
}

/// Путь к папке skills OpenCode: `XDG_CONFIG_HOME/opencode/skills` или `~/.config/opencode/skills`.
fn opencode_skills_dir(env: &InstallEnv) -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| env.home.join(".config"))
        .join("opencode")
        .join("skills")
}

/// Получает команду MCP-сервера bars из конфига OpenCode; `None`, если записи нет.
pub fn mcp_command(file: &Path) -> Result<Option<String>> {
    let Some(text) = fsx::read_text(file)? else {
        return Ok(None);
    };
    // Парсим JSON/JSONC — пробуем как JSON, иначе как JSONC (с комментариями).
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => match jsonc_parser::parse_to_serde_value(&text, &Default::default()) {
            Ok(v) => v,
            Err(_) => return Ok(None),
        },
    };
    Ok(value
        .pointer("/mcp/servers/bars/command")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .map(str::to_string))
}

/// Запись bars установлена, если есть `mcp.servers.bars`.
pub fn has_mcp(file: &Path) -> Result<bool> {
    Ok(mcp_command(file)?.is_some())
}

/// Пишет/обновляет запись `mcp.servers.bars`; уже правильный файл не переписывается.
pub fn install_mcp(file: &Path, exe: &Path) -> Result<bool> {
    let existing = fsx::read_text(file)?;
    let target_command = exe.display().to_string();
    let target = serde_json::json!({
        "type": "local",
        "command": [target_command.clone()],
        "args": [],
    });

    let mut value: serde_json::Value = match &existing {
        Some(text) => match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => match jsonc_parser::parse_to_serde_value(text, &Default::default()) {
                Ok(v) => v,
                Err(_) => serde_json::json!({}),
            },
        },
        None => serde_json::json!({}),
    };

    // Убедимся, что mcp.servers — объект
    let mcp = value
        .entry("mcp")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .expect("mcp должен быть объектом");
    let servers = mcp
        .entry("servers")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .expect("mcp.servers должен быть объектом");

    let bars_entry = servers
        .entry("bars")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .expect("mcp.servers.bars должен быть объектом");

    // Проверяем, изменилось ли что-то
    let needs_write = match (
        bars_entry.get("type").and_then(|v| v.as_str()),
        bars_entry.get("command").and_then(|v| v.as_array()).and_then(|a| a.first()).and_then(|v| v.as_str()),
    ) {
        (Some("local"), Some(cmd)) if cmd == target_command => false,
        _ => true,
    };

    if !needs_write {
        return Ok(false);
    }

    bars_entry.insert("type".into(), serde_json::json!("local"));
    bars_entry.insert("command".into(), serde_json::json!([target_command]));
    bars_entry.insert("args".into(), serde_json::json!([]));

    // Сериализуем обратно: сохраняем формат (JSON или JSONC)
    let out = if existing.as_deref().map(|s| s.trim().starts_with('{')).unwrap_or(true) {
        serde_json::to_string_pretty(&value).expect("JSON сериализуется")
    } else {
        // Для JSONC комментариев не сохраняем — пишем чистый JSON (как у iterion).
        serde_json::to_string_pretty(&value).expect("JSON сериализуется")
    };

    fsx::write_atomic(file, &out, false)?;
    Ok(true)
}

/// Убирает запись `mcp.servers.bars`; чужие записи и настройки не трогает.
pub fn uninstall_mcp(file: &Path) -> Result<bool> {
    let Some(text) = fsx::read_text(file)? else {
        return Ok(false);
    };
    let mut value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => match jsonc_parser::parse_to_serde_value(&text, &Default::default()) {
            Ok(v) => v,
            Err(_) => return Ok(false),
        },
    };

    let removed = value
        .pointer_mut("/mcp/servers/bars")
        .map(|v| {
            if let Some(obj) = v.as_object_mut() {
                obj.clear();
            }
            true
        })
        .unwrap_or(false);

    // Удаляем пустой ключ bars
    if let Some(servers) = value.pointer_mut("/mcp/servers").and_then(|v| v.as_object_mut()) {
        servers.shift_remove("bars");
    }

    if !removed {
        return Ok(false);
    }

    let out = serde_json::to_string_pretty(&value).expect("JSON сериализуется");
    fsx::write_atomic(file, &out, false)?;
    Ok(true)
}