//! Правка JSON-файлов клиентов: `~/.claude.json` (MCP), `settings.json` и `hooks.json` (хук).
//!
//! Файл читается прямо перед записью, порядок ключей сохраняется (`serde_json` с
//! `preserve_order`). JSON с комментариями не разбирается и не меняется.

use std::path::Path;

use serde_json::{Map, Value, json};

use super::fsx::{read_text, write_atomic};
use super::{ClientsError, HOOK_MATCHER, Result};

fn parse_error(file: &Path, message: impl ToString) -> ClientsError {
    ClientsError::Parse {
        file: file.to_path_buf(),
        message: message.to_string(),
    }
}

/// Корневой объект файла; пустой или отсутствующий файл — пустой объект.
fn load(file: &Path) -> Result<Value> {
    let Some(text) = read_text(file)? else {
        return Ok(Value::Object(Map::new()));
    };
    if text.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    let value: Value = serde_json::from_str(&text).map_err(|error| parse_error(file, error))?;
    if value.is_object() {
        Ok(value)
    } else {
        Err(parse_error(file, "ожидался объект JSON"))
    }
}

/// Пишет файл, если значение изменилось. `true` — записан.
fn save(file: &Path, before: &Value, after: &Value) -> Result<bool> {
    if before == after {
        return Ok(false);
    }
    let mut text = serde_json::to_string_pretty(after).expect("JSON сериализуется");
    text.push('\n');
    write_atomic(file, &text, true)?;
    Ok(true)
}

/// Вложенный объект по ключу; отсутствующий создаётся, не объект — ошибка.
fn object_at<'a>(
    file: &Path,
    parent: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>> {
    parent
        .entry(key)
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| parse_error(file, format!("«{key}» должен быть объектом")))
}

fn root_mut<'a>(file: &Path, value: &'a mut Value) -> Result<&'a mut Map<String, Value>> {
    value
        .as_object_mut()
        .ok_or_else(|| parse_error(file, "ожидался объект JSON"))
}

/// Запись `mcpServers.bars` в `~/.claude.json`.
pub fn install_mcp(file: &Path, mcp_exe: &Path) -> Result<bool> {
    let before = load(file)?;
    let mut after = before.clone();
    let servers = object_at(file, root_mut(file, &mut after)?, "mcpServers")?;
    let command = Value::String(mcp_exe.display().to_string());
    match servers.get_mut("bars").and_then(Value::as_object_mut) {
        Some(entry) => {
            entry.insert("type".into(), json!("stdio"));
            entry.insert("command".into(), command);
            entry.insert("args".into(), json!([]));
        }
        None => {
            servers.insert(
                "bars".into(),
                json!({ "type": "stdio", "command": command, "args": [], "env": {} }),
            );
        }
    }
    save(file, &before, &after)
}

pub fn uninstall_mcp(file: &Path) -> Result<bool> {
    let before = load(file)?;
    let mut after = before.clone();
    if let Some(servers) = after.get_mut("mcpServers").and_then(Value::as_object_mut) {
        servers.shift_remove("bars");
    }
    save(file, &before, &after)
}

/// Хук bars: команда с `bars-cli` и `hook session-start`.
fn is_bars_hook(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(|command| {
            command.contains("bars-cli") && command.contains("hook session-start")
        })
}

/// Группы `hooks.SessionStart`; отсутствующие создаются.
fn session_start<'a>(file: &Path, root: &'a mut Value) -> Result<&'a mut Vec<Value>> {
    let hooks = object_at(file, root_mut(file, root)?, "hooks")?;
    hooks
        .entry("SessionStart")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| parse_error(file, "«SessionStart» должен быть массивом"))
}

/// Хук начала сессии в `hooks.SessionStart`: обновляет команду своей записи или добавляет группу.
pub fn install_hook(file: &Path, command: &str) -> Result<bool> {
    let before = load(file)?;
    let mut after = before.clone();
    let groups = session_start(file, &mut after)?;
    let mut found = false;
    for group in groups.iter_mut() {
        let Some(hooks) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
            continue;
        };
        let only_bars = hooks.len() == 1;
        let mut hit = false;
        for hook in hooks.iter_mut().filter(|hook| is_bars_hook(hook)) {
            if let Some(entry) = hook.as_object_mut() {
                entry.insert("command".into(), json!(command));
                hit = true;
            }
        }
        if hit {
            found = true;
            if only_bars && let Some(group) = group.as_object_mut() {
                group.insert("matcher".into(), json!(HOOK_MATCHER));
            }
        }
    }
    if !found {
        groups.push(json!({
            "matcher": HOOK_MATCHER,
            "hooks": [{ "type": "command", "command": command }],
        }));
    }
    save(file, &before, &after)
}

/// Убирает хуки bars; опустевшие группы и пустые `SessionStart` и `hooks` исчезают.
pub fn uninstall_hook(file: &Path) -> Result<bool> {
    let before = load(file)?;
    let mut after = before.clone();
    let Some(hooks) = after.get_mut("hooks").and_then(Value::as_object_mut) else {
        return Ok(false);
    };
    if let Some(groups) = hooks.get_mut("SessionStart").and_then(Value::as_array_mut) {
        groups.retain_mut(|group| {
            let Some(list) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            let had = list.len();
            list.retain(|hook| !is_bars_hook(hook));
            // Группа, где остались чужие хуки или не было хуков bars, остаётся.
            !(list.is_empty() && had > 0)
        });
        if groups.is_empty() {
            hooks.shift_remove("SessionStart");
        }
    }
    if hooks.is_empty() {
        after
            .as_object_mut()
            .expect("корень — объект")
            .shift_remove("hooks");
    }
    save(file, &before, &after)
}

/// Команда `mcpServers.bars`; `None` — записи нет.
pub fn mcp_command(file: &Path) -> Result<Option<String>> {
    Ok(load(file)?
        .pointer("/mcpServers/bars/command")
        .and_then(Value::as_str)
        .map(str::to_string))
}

/// Команды всех хуков bars в `hooks.SessionStart`.
pub fn hook_commands(file: &Path) -> Result<Vec<String>> {
    let value = load(file)?;
    let groups = value
        .pointer("/hooks/SessionStart")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    Ok(groups
        .iter()
        .filter_map(|group| group.get("hooks").and_then(Value::as_array))
        .flatten()
        .filter(|hook| is_bars_hook(hook))
        .filter_map(|hook| hook.get("command").and_then(Value::as_str))
        .map(str::to_string)
        .collect())
}
