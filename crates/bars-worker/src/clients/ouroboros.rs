//! Ouroboros хранит MCP в массиве общего settings.json, без SessionStart.
use std::path::Path;

use serde_json::{Value, json};

use super::{ClientsError, Result, fsx};

struct SettingsLock {
    file: Option<std::fs::File>,
    path: std::path::PathBuf,
}

impl Drop for SettingsLock {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = std::fs::remove_file(&self.path);
    }
}

fn lock_settings(file: &Path) -> Result<SettingsLock> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(super::io(parent))?;
    }
    let mut path = file.as_os_str().to_os_string();
    path.push(".lock");
    let path = std::path::PathBuf::from(path);
    // Тот же O_CREAT|O_EXCL-протокол, что у config._acquire_settings_lock upstream.
    let lock = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::AlreadyExists {
                ClientsError::Conflict {
                    file: file.to_path_buf(),
                }
            } else {
                ClientsError::Io {
                    file: path.clone(),
                    source,
                }
            }
        })?;
    Ok(SettingsLock {
        file: Some(lock),
        path,
    })
}

fn error(file: &Path, message: &str) -> ClientsError {
    ClientsError::Parse {
        file: file.to_path_buf(),
        message: message.into(),
    }
}

fn parse(file: &Path, text: Option<&str>) -> Result<Value> {
    let root: Value = serde_json::from_str(text.unwrap_or("{}"))
        .map_err(|_| error(file, "ожидался объект JSON настроек Ouroboros"))?;
    if !root.is_object() {
        return Err(error(file, "ожидался объект JSON настроек Ouroboros"));
    }
    if let Some(servers) = root.get("MCP_SERVERS")
        && !servers.is_array()
    {
        return Err(error(file, "MCP_SERVERS должен быть массивом"));
    }
    Ok(root)
}

fn is_bars(server: &Value) -> bool {
    // Upstream нормализует id/slug/name в нижний регистр, убирая крайние разделители.
    let identity = ["id", "slug", "name"]
        .iter()
        .filter_map(|key| server.get(key))
        .find(|value| match value {
            Value::Null => false,
            Value::Bool(value) => *value,
            Value::String(value) => !value.is_empty(),
            Value::Number(value) => value.as_f64() != Some(0.0),
            Value::Array(value) => !value.is_empty(),
            Value::Object(value) => !value.is_empty(),
        });
    let Some(name) = identity.and_then(Value::as_str) else {
        return false;
    };
    name.trim()
        .trim_matches(|c: char| !c.is_ascii_alphanumeric())
        .eq_ignore_ascii_case("bars")
}

fn set(root: &mut Value, exe: &Path, activate: bool) {
    let servers = root
        .as_object_mut()
        .expect("parse проверил объект")
        .entry("MCP_SERVERS")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .expect("parse проверил массив");
    let first = servers.iter().position(is_bars);
    let mut entry = first
        .map(|i| servers[i].clone())
        .unwrap_or_else(|| json!({}));
    entry["id"] = json!("bars");
    entry["name"] = json!("bars");
    entry["transport"] = json!("stdio");
    entry["command"] = json!(exe.display().to_string());
    entry["args"] = json!([]);
    if activate {
        entry["enabled"] = json!(true);
    }
    // HTTP-поля делают stdio-запись невалидной в upstream.
    for key in ["url", "headers", "auth_token", "auth_header"] {
        entry
            .as_object_mut()
            .expect("сервер — объект")
            .shift_remove(key);
    }
    if let Some(index) = first {
        servers[index] = entry;
        let mut seen = false;
        servers.retain(|server| {
            if !is_bars(server) {
                return true;
            }
            let keep = !seen;
            seen = true;
            keep
        });
    } else {
        servers.push(entry);
    }
    if activate {
        root["MCP_ENABLED"] = json!(true);
    }
}

fn update(file: &Path, edit: impl Fn(&mut Value)) -> Result<bool> {
    let _lock = lock_settings(file)?;
    fsx::update_text(file, |text| {
        let before = parse(file, text)?;
        let mut after = before.clone();
        edit(&mut after);
        if before == after {
            return Ok(None);
        }
        Ok(Some(format!(
            "{}\n",
            serde_json::to_string_pretty(&after).expect("JSON")
        )))
    })
}

pub fn validate_mcp(file: &Path) -> Result<()> {
    parse(file, fsx::read_text(file)?.as_deref()).map(|_| ())
}

pub fn has_mcp(file: &Path) -> Result<bool> {
    let root = parse(file, fsx::read_text(file)?.as_deref())?;
    Ok(root
        .get("MCP_SERVERS")
        .and_then(Value::as_array)
        .is_some_and(|servers| servers.iter().any(is_bars)))
}

pub fn mcp_command(file: &Path) -> Result<Option<String>> {
    let root = parse(file, fsx::read_text(file)?.as_deref())?;
    Ok(root
        .get("MCP_SERVERS")
        .and_then(Value::as_array)
        .and_then(|servers| servers.iter().find(|server| is_bars(server)))
        .and_then(|server| server.get("command"))
        .and_then(Value::as_str)
        .map(str::to_owned))
}

pub fn install_mcp(file: &Path, exe: &Path, activate: bool) -> Result<bool> {
    update(file, |root| set(root, exe, activate))
}

pub fn uninstall_mcp(file: &Path) -> Result<bool> {
    if fsx::read_text(file)?.is_none() {
        return Ok(false);
    }
    update(file, |root| {
        if let Some(servers) = root.get_mut("MCP_SERVERS").and_then(Value::as_array_mut) {
            servers.retain(|server| !is_bars(server));
        }
    })
}
