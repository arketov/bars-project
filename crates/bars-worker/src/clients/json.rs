//! Правки JSON-конфигов: только записи bars, повторное чтение при конфликте.
use super::fsx::{read_text, update_text};
use super::{ClientsError, GATE_MATCHER, HOOK_MATCHER, Result};
use serde_json::{Map, Value, json};
use std::path::Path;

fn parse_error(file: &Path, message: impl ToString) -> ClientsError {
    ClientsError::Parse {
        file: file.to_path_buf(),
        message: message.to_string(),
    }
}
fn parse(file: &Path, text: Option<&str>) -> Result<Value> {
    let text = text.unwrap_or_default();
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    let value: Value = serde_json::from_str(text).map_err(|e| parse_error(file, e))?;
    if !value.is_object() {
        return Err(parse_error(file, "ожидался объект JSON"));
    }
    Ok(value)
}
fn load(file: &Path) -> Result<Value> {
    parse(file, read_text(file)?.as_deref())
}
fn update(file: &Path, edit: impl Fn(&mut Value) -> Result<()>) -> Result<bool> {
    update_text(file, |text| {
        let before = parse(file, text)?;
        let mut after = before.clone();
        edit(&mut after)?;
        if before == after {
            return Ok(None);
        }
        Ok(Some(format!(
            "{}\n",
            serde_json::to_string_pretty(&after).expect("JSON")
        )))
    })
}
fn object_at<'a>(
    file: &Path,
    parent: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>> {
    parent
        .entry(key)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| parse_error(file, format!("«{key}» должен быть объектом")))
}
fn root_mut(value: &mut Value) -> &mut Map<String, Value> {
    value.as_object_mut().expect("parse проверил объект")
}
fn set_mcp(file: &Path, root: &mut Value, exe: &Path) -> Result<()> {
    let servers = object_at(file, root_mut(root), "mcpServers")?;
    let entry = servers.entry("bars").or_insert_with(|| json!({}));
    let entry = entry
        .as_object_mut()
        .ok_or_else(|| parse_error(file, "«mcpServers.bars» должен быть объектом"))?;
    entry.insert("type".into(), json!("stdio"));
    entry.insert("command".into(), json!(exe.display().to_string()));
    entry.insert("args".into(), json!([]));
    entry.entry("env").or_insert_with(|| json!({}));
    Ok(())
}
pub fn validate_mcp(file: &Path, exe: &Path) -> Result<()> {
    set_mcp(file, &mut load(file)?, exe)
}
pub fn has_mcp(file: &Path) -> Result<bool> {
    let root = load(file)?;
    match root.get("mcpServers") {
        None => Ok(false),
        Some(servers) => servers
            .as_object()
            .map(|m| m.contains_key("bars"))
            .ok_or_else(|| parse_error(file, "«mcpServers» должен быть объектом")),
    }
}
pub fn install_mcp(file: &Path, exe: &Path) -> Result<bool> {
    update(file, |root| set_mcp(file, root, exe))
}
pub fn uninstall_mcp(file: &Path) -> Result<bool> {
    update(file, |root| {
        if let Some(servers) = root.get_mut("mcpServers").and_then(Value::as_object_mut) {
            servers.shift_remove("bars");
        }
        Ok(())
    })
}
fn is_bars_hook(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| c.contains("bars-cli") && c.contains("hook session-start"))
}
fn is_bars_gate(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| c.contains("bars-cli") && c.contains("hook pre-tool-use"))
}
fn event_groups<'a>(file: &Path, root: &'a mut Value, event: &str) -> Result<&'a mut Vec<Value>> {
    let hooks = object_at(file, root_mut(root), "hooks")?;
    let groups = hooks
        .entry(event)
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| parse_error(file, format!("«{event}» должен быть массивом")))?;
    for group in groups.iter() {
        if !group.is_object() || !group.get("hooks").is_some_and(Value::is_array) {
            return Err(parse_error(
                file,
                format!("группа {event} должна содержать массив hooks"),
            ));
        }
    }
    Ok(groups)
}
fn session_start<'a>(file: &Path, root: &'a mut Value) -> Result<&'a mut Vec<Value>> {
    event_groups(file, root, "SessionStart")
}
/// Ставит одну каноническую группу хука bars в событие; свои дубли сводит к ней.
fn set_group(
    file: &Path,
    root: &mut Value,
    event: &str,
    matcher: &str,
    command: &str,
    is_ours: fn(&Value) -> bool,
) -> Result<()> {
    let groups = event_groups(file, root, event)?;
    let canonical = json!({"matcher": matcher, "hooks": [{"type":"command", "command":command}]});
    let count = groups
        .iter()
        .flat_map(|g| g["hooks"].as_array().expect("проверено"))
        .filter(|h| is_ours(h))
        .count();
    // Не переставлять канонический хук: иначе сбрасывается одобрение Codex.
    if count == 1 && groups.contains(&canonical) {
        return Ok(());
    }
    groups.retain_mut(|group| {
        let hooks = group["hooks"].as_array_mut().expect("проверено");
        let had = hooks.len();
        hooks.retain(|h| !is_ours(h));
        !(had > 0 && hooks.is_empty())
    });
    groups.push(canonical);
    Ok(())
}
/// Убирает записи bars из события и опустевшие контейнеры.
fn remove_group(root: &mut Value, event: &str, is_ours: fn(&Value) -> bool) {
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return;
    };
    if let Some(groups) = hooks.get_mut(event).and_then(Value::as_array_mut) {
        groups.retain_mut(|group| {
            let Some(list) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            let had = list.len();
            list.retain(|hook| !is_ours(hook));
            !(list.is_empty() && had > 0)
        });
        if groups.is_empty() {
            hooks.shift_remove(event);
        }
    }
    if hooks.is_empty() {
        root_mut(root).shift_remove("hooks");
    }
}
fn set_hook(file: &Path, root: &mut Value, command: &str) -> Result<()> {
    set_group(
        file,
        root,
        "SessionStart",
        HOOK_MATCHER,
        command,
        is_bars_hook,
    )
}
pub fn validate_hook(file: &Path, command: &str) -> Result<()> {
    set_hook(file, &mut load(file)?, command)
}
pub fn install_hook(file: &Path, command: &str) -> Result<bool> {
    update(file, |root| set_hook(file, root, command))
}
pub fn uninstall_hook(file: &Path) -> Result<bool> {
    update(file, |root| {
        remove_group(root, "SessionStart", is_bars_hook);
        Ok(())
    })
}
/// Гейт чтения документации в файле настроек проекта: группа `PreToolUse`.
pub fn install_gate(file: &Path, command: &str) -> Result<bool> {
    update(file, |root| {
        set_group(
            file,
            root,
            "PreToolUse",
            GATE_MATCHER,
            command,
            is_bars_gate,
        )
    })
}
pub fn uninstall_gate(file: &Path) -> Result<bool> {
    update(file, |root| {
        remove_group(root, "PreToolUse", is_bars_gate);
        Ok(())
    })
}
/// Проекты, которые знает Claude Code: ключи `projects` в `~/.claude.json`.
pub fn claude_projects(file: &Path) -> Result<Vec<String>> {
    Ok(load(file)?
        .get("projects")
        .and_then(Value::as_object)
        .map(|projects| projects.keys().cloned().collect())
        .unwrap_or_default())
}
pub fn mcp_command(file: &Path) -> Result<Option<String>> {
    Ok(load(file)?
        .pointer("/mcpServers/bars/command")
        .and_then(Value::as_str)
        .map(str::to_string))
}
pub fn hook_commands(file: &Path) -> Result<Vec<String>> {
    let mut value = load(file)?;
    if value.get("hooks").is_none() {
        return Ok(Vec::new());
    }
    let groups = session_start(file, &mut value)?;
    Ok(groups
        .iter()
        .flat_map(|g| g["hooks"].as_array().expect("проверено"))
        .filter(|h| is_bars_hook(h))
        .filter_map(|h| h["command"].as_str())
        .map(str::to_string)
        .collect())
}
