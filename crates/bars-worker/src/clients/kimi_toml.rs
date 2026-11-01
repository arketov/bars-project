//! Правка `~/.kimi-code/config.toml`: запись хука начала сессии в массиве `[[hooks]]`
//! (`toml_edit` сохраняет форматирование и комментарии). Файл чувствительный: в нём providers,
//! модели и ключи Kimi Code. Меняются только элементы `hooks`, чья команда — хук bars; пишутся
//! только разрешённые Kimi поля (`event`, `matcher`, `command` — иначе конфиг не загружается).

use std::path::Path;

use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, value};

use super::fsx::{read_text, update_text};
use super::{ClientsError, HOOK_MATCHER, Result};

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

fn is_bars_hook(entry: &Table) -> bool {
    entry
        .get("command")
        .and_then(Item::as_str)
        .is_some_and(|command| {
            command.contains("bars-cli") && command.contains("hook session-start")
        })
}

/// Массив `[[hooks]]`; отсутствующий ключ создаётся. Другой тип у существующего ключа — ошибка.
fn hooks_array<'a>(file: &Path, doc: &'a mut DocumentMut) -> Result<&'a mut ArrayOfTables> {
    let item = doc
        .entry("hooks")
        .or_insert_with(|| Item::ArrayOfTables(ArrayOfTables::new()));
    // TOML допускает также пустой массив и массив inline-таблиц.
    if item.as_array().is_some_and(|array| array.is_empty()) {
        *item = Item::ArrayOfTables(ArrayOfTables::new());
    } else if item.is_array() {
        *item = Item::ArrayOfTables(
            item.clone()
                .into_array_of_tables()
                .map_err(|_| parse_error(file, "«hooks» должен быть массивом таблиц"))?,
        );
    }
    item.as_array_of_tables_mut()
        .ok_or_else(|| parse_error(file, "«hooks» должен быть массивом таблиц [[hooks]]"))
}

/// Каноническая запись: только разрешённые Kimi поля, в его порядке.
fn canonical_entry(command: &str) -> Table {
    let mut entry = Table::new();
    entry.insert("event", value("SessionStart"));
    entry.insert("matcher", value(HOOK_MATCHER));
    entry.insert("command", value(command));
    entry
}

fn set_hook(file: &Path, doc: &mut DocumentMut, command: &str) -> Result<()> {
    let hooks = hooks_array(file, doc)?;
    hooks.retain(|entry| !is_bars_hook(entry));
    hooks.push(canonical_entry(command));
    Ok(())
}

pub fn validate_hook(file: &Path, command: &str) -> Result<()> {
    set_hook(file, &mut load(file)?, command)
}

pub fn install_hook(file: &Path, command: &str) -> Result<bool> {
    update(file, |doc| set_hook(file, doc, command))
}

pub fn uninstall_hook(file: &Path) -> Result<bool> {
    update(file, |doc| {
        if !doc.contains_key("hooks") {
            return Ok(());
        }
        let mut normalized = doc.clone();
        let hooks = hooks_array(file, &mut normalized)?;
        if !hooks.iter().any(is_bars_hook) {
            return Ok(());
        }
        hooks.retain(|entry| !is_bars_hook(entry));
        let emptied = hooks.is_empty();
        if emptied {
            normalized.as_table_mut().remove("hooks");
        }
        *doc = normalized;
        Ok(())
    })
}

/// Команды хуков bars в `[[hooks]]`; файл без ключа `hooks` даёт пустой список.
pub fn hook_commands(file: &Path) -> Result<Vec<String>> {
    let mut doc = load(file)?;
    if !doc.contains_key("hooks") {
        return Ok(Vec::new());
    }
    Ok(hooks_array(file, &mut doc)?
        .iter()
        .filter(|entry| is_bars_hook(entry))
        .filter_map(|entry| entry.get("command").and_then(Item::as_str))
        .map(str::to_string)
        .collect())
}
