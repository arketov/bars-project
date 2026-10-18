//! Правка `~/.codex/config.toml`: таблица `[mcp_servers.bars]` (`toml_edit` сохраняет
//! форматирование и комментарии).

use std::path::Path;

use toml_edit::{Array, DocumentMut, Item, Table, value};

use super::fsx::{read_text, update_text};
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
        .map_err(|error| parse_error(file, error.message()))
}

fn update(file: &Path, edit: impl Fn(&mut DocumentMut) -> Result<()>) -> Result<bool> {
    update_text(file, |text| {
        let mut doc = text
            .unwrap_or_default()
            .parse::<DocumentMut>()
            .map_err(|error| parse_error(file, error.message()))?;
        let before = doc.to_string();
        edit(&mut doc)?;
        let after = doc.to_string();
        Ok((after != before).then_some(after))
    })
}

fn set_mcp(file: &Path, doc: &mut DocumentMut, mcp_exe: &Path) -> Result<()> {
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
    Ok(())
}

pub fn validate_mcp(file: &Path, exe: &Path) -> Result<()> {
    set_mcp(file, &mut load(file)?, exe)
}

pub fn has_mcp(file: &Path) -> Result<bool> {
    let doc = load(file)?;
    match doc.get("mcp_servers") {
        None => Ok(false),
        Some(servers) => servers
            .as_table_like()
            .map(|table| table.contains_key("bars"))
            .ok_or_else(|| parse_error(file, "«mcp_servers» должен быть таблицей")),
    }
}

pub fn install_mcp(file: &Path, exe: &Path) -> Result<bool> {
    update(file, |doc| set_mcp(file, doc, exe))
}

pub fn uninstall_mcp(file: &Path) -> Result<bool> {
    update(file, |doc| {
        if let Some(servers) = doc.get_mut("mcp_servers").and_then(Item::as_table_mut) {
            servers.remove("bars");
        }
        Ok(())
    })
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
