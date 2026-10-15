//! Правка `~/.codex/config.toml`: таблица `[mcp_servers.bars]` (`toml_edit` сохраняет
//! форматирование и комментарии).

use std::path::Path;

use toml_edit::{Array, DocumentMut, Item, Table, value};

use super::fsx::{read_text, write_atomic};
use super::{ClientsError, Result};

fn parse_error(file: &Path, message: impl ToString) -> ClientsError {
    ClientsError::Parse {
        file: file.to_path_buf(),
        message: message.to_string(),
    }
}

fn load(file: &Path) -> Result<DocumentMut> {
    read_text(file)?
        .unwrap_or_default()
        .parse::<DocumentMut>()
        .map_err(|error| parse_error(file, error))
}

fn save(file: &Path, before: &str, doc: &DocumentMut) -> Result<bool> {
    let text = doc.to_string();
    if text == before {
        return Ok(false);
    }
    write_atomic(file, &text, true)?;
    Ok(true)
}

pub fn install_mcp(file: &Path, mcp_exe: &Path) -> Result<bool> {
    let mut doc = load(file)?;
    let before = doc.to_string();
    let servers = doc
        .entry("mcp_servers")
        .or_insert_with(|| {
            let mut table = Table::new();
            table.set_implicit(true);
            Item::Table(table)
        })
        .as_table_mut()
        .ok_or_else(|| parse_error(file, "«mcp_servers» должен быть таблицей"))?;
    let bars = servers
        .entry("bars")
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| parse_error(file, "«mcp_servers.bars» должен быть таблицей"))?;
    let command = mcp_exe.display().to_string();
    if bars.get("command").and_then(Item::as_str) != Some(command.as_str()) {
        bars["command"] = value(command);
    }
    if !bars
        .get("args")
        .and_then(Item::as_array)
        .is_some_and(Array::is_empty)
    {
        bars["args"] = value(Array::new());
    }
    save(file, &before, &doc)
}

pub fn uninstall_mcp(file: &Path) -> Result<bool> {
    let mut doc = load(file)?;
    let before = doc.to_string();
    if let Some(servers) = doc.get_mut("mcp_servers").and_then(Item::as_table_mut) {
        servers.remove("bars");
    }
    save(file, &before, &doc)
}

/// Команда `[mcp_servers.bars]`; `None` — записи нет.
pub fn mcp_command(file: &Path) -> Result<Option<String>> {
    Ok(load(file)?
        .get("mcp_servers")
        .and_then(|servers| servers.get("bars"))
        .and_then(|bars| bars.get("command"))
        .and_then(Item::as_str)
        .map(str::to_string))
}
